use std::str::FromStr;

use alloy::primitives::{Address, Bytes, U256};
use alloy::providers::ProviderBuilder;
use alloy::sol_types::SolCall;
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::post,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::account::predict_address;
use crate::api::error::ApiError;
use crate::automation::audit::{EventType, NewAuditEvent};
use crate::automation::policy::ChainId;
use crate::contracts::IAaveFacet;
use crate::gateway::contracts::IGatewayMinter;
use crate::gateway::domains;
use crate::gateway::intent::{IntentArgs, build_intent, sign_burn_intent};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route(
        "/debug/recover-gateway/{user}",
        post(recover_gateway_escrow),
    )
}

#[derive(Deserialize)]
pub struct RecoverRequest {
    pub amount_6dec: String,
    #[serde(default)]
    pub then_supply_aave: bool,
}

#[derive(Serialize)]
pub struct RecoverResponse {
    pub intent_digest: String,
    pub mint_tx: String,
    pub supply_tx: Option<String>,
}

async fn recover_gateway_escrow(
    State(state): State<AppState>,
    Path(user): Path<Address>,
    Json(req): Json<RecoverRequest>,
) -> Result<Json<RecoverResponse>, ApiError> {
    let amount = U256::from_str_radix(req.amount_6dec.trim_start_matches("0x"), 10)
        .or_else(|_| U256::from_str_radix(req.amount_6dec.trim_start_matches("0x"), 16))
        .map_err(|e| ApiError(anyhow::anyhow!("bad amount: {e}")))?;
    if amount == U256::ZERO {
        return Err(ApiError(anyhow::anyhow!("amount must be > 0")));
    }

    let cfg = &state.cfg;
    let arc_factory = Address::from_str(&cfg.arc_factory)?;
    let arb_factory = Address::from_str(&cfg.arbitrum_sepolia_factory)?;
    let arc_diamond = predict_address(&cfg.arc_rpc_url, arc_factory, user).await?;
    let arb_diamond = predict_address(&cfg.arbitrum_sepolia_rpc_url, arb_factory, user).await?;

    let start_id = state
        .audit
        .append(NewAuditEvent::new(
            user,
            EventType::ExecutorActionStart,
            json!({
                "recovery": true,
                "amount_6dec": amount.to_string(),
                "arc_diamond": format!("{arc_diamond:?}"),
                "arb_diamond": format!("{arb_diamond:?}"),
            }),
            Utc::now(),
        ))
        .await?;

    let gateway_wallet = Address::from_str(&cfg.gateway_wallet)?;
    let gateway_minter_addr = Address::from_str(&cfg.gateway_minter)?;
    let intent = build_intent(IntentArgs {
        source_depositor: arc_diamond,
        destination_recipient: arb_diamond,
        source_token: state.arc.usdc,
        destination_token: Address::from_str(&cfg.arbitrum_sepolia_usdc)?,
        source_contract: gateway_wallet,
        destination_contract: gateway_minter_addr,
        source_signer: state.agent_address,
        destination_caller: Address::ZERO,
        source_domain: domains::ARC_TESTNET,
        destination_domain: domains::ARBITRUM_SEPOLIA,
        amount,
    });
    let signed = sign_burn_intent(state.agent_signer.as_ref(), intent).await?;
    let digest_hex = format!("0x{}", hex::encode(signed.digest.as_slice()));
    let attestation = state.gateway.transfer(&signed).await?;
    let _ = state
        .audit
        .append(
            NewAuditEvent::new(
                user,
                EventType::ExecutorSubstep,
                json!({
                    "label": "recovery_attestation",
                    "start_event_id": start_id,
                    "intent_digest": digest_hex,
                }),
                Utc::now(),
            )
            .with_chain(ChainId::Arc)
            .with_tx_hash(digest_hex.clone()),
        )
        .await;

    let minter_addr = Address::from_str(&cfg.gateway_minter)?;
    let att_bytes: Bytes =
        hex::decode(attestation.attestation.trim_start_matches("0x"))?.into();
    let sig_bytes: Bytes =
        hex::decode(attestation.signature.trim_start_matches("0x"))?.into();
    let provider = ProviderBuilder::new()
        .wallet(state.agent_signer.as_ref().clone())
        .connect(&cfg.arbitrum_sepolia_rpc_url)
        .await?;
    let minter = IGatewayMinter::new(minter_addr, provider);
    let pending = minter.gatewayMint(att_bytes, sig_bytes).send().await?;
    let mint_tx = format!("{:?}", pending.tx_hash());
    let _ = state
        .audit
        .append(
            NewAuditEvent::new(
                user,
                EventType::ExecutorSubstep,
                json!({
                    "label": "recovery_mint",
                    "start_event_id": start_id,
                }),
                Utc::now(),
            )
            .with_chain(ChainId::ArbitrumSepolia)
            .with_tx_hash(mint_tx.clone()),
        )
        .await;

    let supply_tx = if req.then_supply_aave {
        // Same 8s sleep as the regular cross-chain flow — keeps the
        // supply from racing the mint inclusion.
        tokio::time::sleep(std::time::Duration::from_secs(8)).await;
        let supply_call: Bytes = IAaveFacet::supplyAaveCall { amount }
            .abi_encode()
            .into();
        let tx = submit_userop_arb(&state, user, supply_call).await?;
        let _ = state
            .audit
            .append(
                NewAuditEvent::new(
                    user,
                    EventType::ExecutorSubstep,
                    json!({
                        "label": "recovery_aave_supply",
                        "start_event_id": start_id,
                    }),
                    Utc::now(),
                )
                .with_chain(ChainId::ArbitrumSepolia)
                .with_tx_hash(tx.clone()),
            )
            .await;
        Some(tx)
    } else {
        None
    };

    let _ = state
        .audit
        .append(
            NewAuditEvent::new(
                user,
                EventType::ExecutorActionDone,
                json!({
                    "recovery": true,
                    "intent_digest": digest_hex,
                    "mint_tx": mint_tx,
                    "supply_tx": supply_tx,
                    "start_event_id": start_id,
                }),
                Utc::now(),
            )
            .with_chain(ChainId::ArbitrumSepolia),
        )
        .await;

    Ok(Json(RecoverResponse {
        intent_digest: digest_hex,
        mint_tx,
        supply_tx,
    }))
}

async fn submit_userop_arb(
    state: &AppState,
    user: Address,
    call_data: Bytes,
) -> anyhow::Result<String> {
    use crate::userop::{PaymasterConfig, build_userop, sign_and_submit};
    let cfg = &state.cfg;
    let factory = Address::from_str(&cfg.arbitrum_sepolia_factory)?;
    let entry_point = Address::from_str(&cfg.arbitrum_sepolia_entry_point)?;
    let diamond = predict_address(&cfg.arbitrum_sepolia_rpc_url, factory, user).await?;
    let paymaster = cfg
        .arbitrum_sepolia_paymaster
        .as_deref()
        .and_then(|s| Address::from_str(s).ok())
        .map(PaymasterConfig::for_compass);
    let userop = build_userop(
        &cfg.arbitrum_sepolia_rpc_url,
        state.agent_signer.as_ref().clone(),
        entry_point,
        diamond,
        call_data,
        paymaster,
    )
    .await?;
    sign_and_submit(
        &cfg.arbitrum_sepolia_rpc_url,
        state.agent_signer.as_ref().clone(),
        state.agent_signer.as_ref(),
        entry_point,
        userop,
        state.agent_address,
    )
    .await
}
