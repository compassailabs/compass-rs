use std::str::FromStr;
use std::time::Duration;

use alloy::primitives::{Address, Bytes, U256};
use alloy::providers::ProviderBuilder;
use alloy::sol_types::SolCall;
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use chrono::Utc;
use serde::Serialize;
use serde_json::json;

use crate::account::predict_address;
use crate::api::session::authorize_arc_gateway_delegate;
use crate::automation::audit::{AuditStore, EventType, NewAuditEvent};
use crate::automation::evaluator::{Action, ActionPlan};
use crate::automation::policy::{ChainId, ProtocolId};
use crate::contracts::IAaveFacet;
use crate::gateway::contracts::IGatewayMinter;
use crate::gateway::domains;
use crate::gateway::intent::{IntentArgs, SignedBurnIntent, build_intent, sign_burn_intent};
use crate::gateway::api::TransferAttestation;
use crate::state::AppState;
use crate::userop::{PaymasterConfig, build_userop, sign_and_submit};

const MINT_TO_SUPPLY_WAIT_SECS: u64 = 8;
const BRIDGE_FEE_BUFFER_USDC_6DEC: u64 = 10_000;
const CIRCLE_INDEXER_RETRY_BUDGET: Duration = Duration::from_secs(60);
#[derive(Debug, Clone, Serialize)]
pub struct TxStep {
    pub label: String,
    pub chain: ChainId,
    pub tx_hash: String,
}

#[async_trait]
pub trait PlanExecutor: Send + Sync {
    async fn execute(
        &self,
        user: Address,
        policy_version: u32,
        plan: &ActionPlan,
    ) -> Result<ExecutionOutcome>;
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExecutionOutcome {
    Completed { tx_hashes: Vec<String> },
    PartialFailure { tx_hashes: Vec<String>, error: String },
}

pub struct OnchainPlanExecutor {
    pub state: AppState,
}

#[async_trait]
impl PlanExecutor for OnchainPlanExecutor {
    async fn execute(
        &self,
        user: Address,
        policy_version: u32,
        plan: &ActionPlan,
    ) -> Result<ExecutionOutcome> {
        execute_plan(&self.state, user, policy_version, plan).await
    }
}

pub async fn execute_plan(
    state: &AppState,
    user: Address,
    policy_version: u32,
    plan: &ActionPlan,
) -> Result<ExecutionOutcome> {
    let mut all_hashes: Vec<String> = Vec::new();
    for action in &plan.actions {
        let start_id = state
            .audit
            .append(
                NewAuditEvent::new(
                    user,
                    EventType::ExecutorActionStart,
                    serde_json::to_value(action)?,
                    Utc::now(),
                )
                .with_policy_version(policy_version)
                .with_chain(action.to.chain),
            )
            .await?;

        match dispatch_action(state, user, action).await {
            Ok(steps) => {
                for step in &steps {
                    state
                        .audit
                        .append(
                            NewAuditEvent::new(
                                user,
                                EventType::ExecutorSubstep,
                                json!({
                                    "label": step.label,
                                    "start_event_id": start_id,
                                }),
                                Utc::now(),
                            )
                            .with_policy_version(policy_version)
                            .with_tx_hash(step.tx_hash.clone())
                            .with_chain(step.chain),
                        )
                        .await?;
                }
                state
                    .audit
                    .append(
                        NewAuditEvent::new(
                            user,
                            EventType::ExecutorActionDone,
                            json!({
                                "action": action,
                                "steps": steps,
                                "start_event_id": start_id,
                            }),
                            Utc::now(),
                        )
                        .with_policy_version(policy_version)
                        .with_chain(action.to.chain),
                    )
                    .await?;
                for s in &steps {
                    all_hashes.push(s.tx_hash.clone());
                }
            }
            Err(e) => {
                state
                    .audit
                    .append(
                        NewAuditEvent::new(
                            user,
                            EventType::ExecutorActionDone,
                            json!({
                                "action": action,
                                "error": e.to_string(),
                                "start_event_id": start_id,
                            }),
                            Utc::now(),
                        )
                        .with_policy_version(policy_version)
                        .with_chain(action.to.chain),
                    )
                    .await?;
                return Ok(ExecutionOutcome::PartialFailure {
                    tx_hashes: all_hashes,
                    error: e.to_string(),
                });
            }
        }
    }
    Ok(ExecutionOutcome::Completed {
        tx_hashes: all_hashes,
    })
}

async fn dispatch_action(
    state: &AppState,
    user: Address,
    action: &Action,
) -> Result<Vec<TxStep>> {
    match (action.from.protocol, action.to.protocol) {
        (ProtocolId::Idle, ProtocolId::AaveV3) if action.from.chain == action.to.chain => {
            let call: Bytes = IAaveFacet::supplyAaveCall {
                amount: U256::MAX,
            }
            .abi_encode()
            .into();
            let tx = submit_userop(state, user, action.to.chain, call).await?;
            Ok(vec![TxStep {
                label: "aave_supply".into(),
                chain: action.to.chain,
                tx_hash: tx,
            }])
        }
        (ProtocolId::AaveV3, ProtocolId::Idle) if action.from.chain == action.to.chain => {
            let call: Bytes = IAaveFacet::withdrawAaveCall {
                amount: action.amount,
            }
            .abi_encode()
            .into();
            let tx = submit_userop(state, user, action.from.chain, call).await?;
            Ok(vec![TxStep {
                label: "aave_withdraw".into(),
                chain: action.from.chain,
                tx_hash: tx,
            }])
        }
        (ProtocolId::Idle, ProtocolId::AaveV3)
            if action.from.chain == ChainId::Arc
                && action.to.chain == ChainId::ArbitrumSepolia =>
        {
            cross_chain_arc_idle_to_arbitrum_aave(state, user, action.amount).await
        }
        _ => Err(anyhow!(
            "unsupported action: {:?} → {:?}",
            action.from,
            action.to,
        )),
    }
}

async fn cross_chain_arc_idle_to_arbitrum_aave(
    state: &AppState,
    user: Address,
    amount: U256,
) -> Result<Vec<TxStep>> {
    use crate::contracts::IGatewayFacet;
    let mut steps = Vec::with_capacity(4);

    let deposit_amount = amount + U256::from(BRIDGE_FEE_BUFFER_USDC_6DEC);
    let deposit_call: Bytes = IGatewayFacet::depositToGatewayCall { amount: deposit_amount }
        .abi_encode()
        .into();
    let deposit_tx = submit_userop(state, user, ChainId::Arc, deposit_call).await?;
    steps.push(TxStep {
        label: "gateway_deposit".into(),
        chain: ChainId::Arc,
        tx_hash: deposit_tx,
    });

    let arc_factory = Address::from_str(&state.cfg.arc_factory)?;
    let arb_factory = Address::from_str(&state.cfg.arbitrum_sepolia_factory)?;
    let arc_diamond =
        predict_address(&state.cfg.arc_rpc_url, arc_factory, user).await?;
    let arb_diamond =
        predict_address(&state.cfg.arbitrum_sepolia_rpc_url, arb_factory, user).await?;

    let gateway_wallet = Address::from_str(&state.cfg.gateway_wallet)?;
    let gateway_minter = Address::from_str(&state.cfg.gateway_minter)?;
    let intent = build_intent(IntentArgs {
        source_depositor: arc_diamond,
        destination_recipient: arb_diamond,
        source_token: state.arc.usdc,
        destination_token: Address::from_str(&state.cfg.arbitrum_sepolia_usdc)?,
        source_contract: gateway_wallet,
        destination_contract: gateway_minter,
        source_signer: state.agent_address,
        destination_caller: Address::ZERO,
        source_domain: domains::ARC_TESTNET,
        destination_domain: domains::ARBITRUM_SEPOLIA,
        amount,
    });
    let signed = sign_burn_intent(state.agent_signer.as_ref(), intent).await?;
    let attestation = transfer_with_delegate_retry(state, arc_diamond, &signed).await?;
    steps.push(TxStep {
        label: "burn_intent_attested".into(),
        chain: ChainId::Arc,
        tx_hash: format!("0x{}", hex::encode(signed.digest.as_slice())),
    });

    let minter_addr = Address::from_str(&state.cfg.gateway_minter)?;
    let att_bytes: Bytes =
        hex::decode(attestation.attestation.trim_start_matches("0x"))?.into();
    let sig_bytes: Bytes =
        hex::decode(attestation.signature.trim_start_matches("0x"))?.into();
    let provider = ProviderBuilder::new()
        .wallet(state.agent_signer.as_ref().clone())
        .connect(&state.cfg.arbitrum_sepolia_rpc_url)
        .await?;
    let minter = IGatewayMinter::new(minter_addr, provider);
    let pending = minter.gatewayMint(att_bytes, sig_bytes).send().await?;
    let mint_tx = format!("{:?}", pending.tx_hash());
    steps.push(TxStep {
        label: "mint_destination".into(),
        chain: ChainId::ArbitrumSepolia,
        tx_hash: mint_tx,
    });

    tokio::time::sleep(Duration::from_secs(MINT_TO_SUPPLY_WAIT_SECS)).await;
.
    let supply_call: Bytes = IAaveFacet::supplyAaveCall { amount: U256::MAX }
        .abi_encode()
        .into();
    let supply_tx = submit_userop(state, user, ChainId::ArbitrumSepolia, supply_call).await?;
    steps.push(TxStep {
        label: "aave_supply".into(),
        chain: ChainId::ArbitrumSepolia,
        tx_hash: supply_tx,
    });

    Ok(steps)
}

async fn transfer_with_delegate_retry(
    state: &AppState,
    arc_diamond: Address,
    signed: &SignedBurnIntent,
) -> Result<TransferAttestation> {
    match state.gateway.transfer(signed).await {
        Ok(att) => return Ok(att),
        Err(e) if is_delegate_not_authorized(&e) => {
            tracing::warn!(
                ?arc_diamond,
                "circle gateway rejected agent signature — auto-running addDelegate then retrying",
            );
            authorize_arc_gateway_delegate(state, arc_diamond).await?;
        }
        Err(e) if !is_insufficient_balance(&e) => return Err(e),
        Err(_) => {}
    }

    let start = std::time::Instant::now();
    let mut backoff = Duration::from_secs(2);
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        match state.gateway.transfer(signed).await {
            Ok(att) => return Ok(att),
            Err(e) if is_insufficient_balance(&e) => {
                if start.elapsed() + backoff > CIRCLE_INDEXER_RETRY_BUDGET {
                    return Err(e);
                }
                tracing::warn!(
                    attempt,
                    backoff_secs = backoff.as_secs(),
                    "circle indexer lag — retrying transfer after backoff",
                );
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(15));
            }
            Err(e) => return Err(e),
        }
    }
}

fn is_delegate_not_authorized(e: &anyhow::Error) -> bool {
    e.to_string().contains("Signer is not authorized")
}

fn is_insufficient_balance(e: &anyhow::Error) -> bool {
    e.to_string().contains("Insufficient balance for depositor")
}

async fn submit_userop(
    state: &AppState,
    user: Address,
    chain: ChainId,
    call_data: Bytes,
) -> Result<String> {
    let (rpc, factory, entry_point) = chain_addrs(state, chain)?;
    let diamond = predict_address(&rpc, factory, user).await?;
    let userop = build_userop(
        &rpc,
        state.agent_signer.as_ref().clone(),
        entry_point,
        diamond,
        call_data,
        chain_paymaster(state, chain),
    )
    .await?;
    sign_and_submit(
        &rpc,
        state.agent_signer.as_ref().clone(),
        state.agent_signer.as_ref(),
        entry_point,
        userop,
        state.agent_address,
    )
    .await
}

fn chain_addrs(state: &AppState, chain: ChainId) -> Result<(String, Address, Address)> {
    match chain {
        ChainId::ArbitrumSepolia => Ok((
            state.cfg.arbitrum_sepolia_rpc_url.clone(),
            Address::from_str(&state.cfg.arbitrum_sepolia_factory)?,
            Address::from_str(&state.cfg.arbitrum_sepolia_entry_point)?,
        )),
        ChainId::Arc => Ok((
            state.cfg.arc_rpc_url.clone(),
            Address::from_str(&state.cfg.arc_factory)?,
            Address::from_str(&state.cfg.arc_entry_point)?,
        )),
    }
}

fn chain_paymaster(state: &AppState, chain: ChainId) -> Option<PaymasterConfig> {
    let raw = match chain {
        ChainId::ArbitrumSepolia => state.cfg.arbitrum_sepolia_paymaster.as_deref()?,
        ChainId::Arc => state.cfg.arc_paymaster.as_deref()?,
    };
    let addr = Address::from_str(raw).ok()?;
    Some(PaymasterConfig::for_compass(addr))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation::evaluator::{Action, ActionPlan};
    use crate::automation::policy::{ChainId, ProtocolId, VenueRef};
    use alloy::primitives::{Address, U256};

    #[derive(Default)]
    struct FakeExecutor {
        pub plans: tokio::sync::Mutex<Vec<(Address, u32, ActionPlan)>>,
    }

    #[async_trait]
    impl PlanExecutor for FakeExecutor {
        async fn execute(
            &self,
            user: Address,
            policy_version: u32,
            plan: &ActionPlan,
        ) -> Result<ExecutionOutcome> {
            self.plans.lock().await.push((user, policy_version, plan.clone()));
            Ok(ExecutionOutcome::Completed {
                tx_hashes: vec!["0xfake".into()],
            })
        }
    }

    fn aave_arb() -> VenueRef {
        VenueRef {
            chain: ChainId::ArbitrumSepolia,
            protocol: ProtocolId::AaveV3,
        }
    }
    fn idle_arb() -> VenueRef {
        VenueRef {
            chain: ChainId::ArbitrumSepolia,
            protocol: ProtocolId::Idle,
        }
    }
    fn aave_arc() -> VenueRef {
        VenueRef {
            chain: ChainId::Arc,
            protocol: ProtocolId::AaveV3,
        }
    }

    #[tokio::test]
    async fn fake_records_plan() {
        let exec = FakeExecutor::default();
        let plan = ActionPlan {
            actions: vec![Action {
                from: idle_arb(),
                to: aave_arb(),
                amount: U256::from(1_000_000u128),
            }],
            expected_profit_usd: 5.0,
            estimated_cost_usd: 1.0,
        };
        let out = exec
            .execute(Address::repeat_byte(0xAA), 1, &plan)
            .await
            .unwrap();
        assert!(matches!(out, ExecutionOutcome::Completed { .. }));
        assert_eq!(exec.plans.lock().await.len(), 1);
    }

    #[tokio::test]
    async fn cross_chain_action_rejected_by_dispatcher() {
        let action = Action {
            from: idle_arb(),
            to: aave_arc(),
            amount: U256::from(1u128),
        };
        let from = action.from.protocol;
        let to = action.to.protocol;
        let same_chain = action.from.chain == action.to.chain;
        let supported = matches!(
            (from, to, same_chain),
            (ProtocolId::Idle, ProtocolId::AaveV3, true)
                | (ProtocolId::AaveV3, ProtocolId::Idle, true)
        );
        assert!(!supported);
    }
}
