use std::str::FromStr;
use std::time::Duration;

use alloy::primitives::{Address, Bytes, U256};
use alloy::providers::{Provider, ProviderBuilder};
use alloy::sol_types::SolCall;
use anyhow::Result;
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::post,
};
use hex;
use serde::Serialize;

use crate::account::predict_address;
use crate::aave::pool::IPool;
use crate::api::error::ApiError;
use crate::automation::policy::ChainId;
use crate::chain::usdc::IERC20;
use crate::contracts::{IAaveFacet, IAccount4337Facet, IGatewayFacet};
use crate::gateway::contracts::{IGatewayMinter, IGatewayWallet};
use crate::gateway::domains;
use crate::gateway::intent::{IntentArgs, build_intent, sign_burn_intent};
use crate::state::AppState;

const MINT_SETTLE_WAIT_SECS: u64 = 6;

pub fn router() -> Router<AppState> {
    Router::new().route("/withdraw/{user}", post(withdraw_to_arc))
}

#[derive(Serialize)]
pub struct WithdrawStep {
    pub label: String,
    pub chain: &'static str,
    pub tx_hash: String,
}

#[derive(Serialize)]
pub struct WithdrawResponse {
    pub user: String,
    pub arc_smart_account: String,
    pub arbitrum_smart_account: String,
    pub aave_balance_6dec: String,
    pub bridged_6dec: String,
    pub steps: Vec<WithdrawStep>,
}

async fn withdraw_to_arc(
    State(state): State<AppState>,
    Path(user): Path<Address>,
) -> Result<Json<WithdrawResponse>, ApiError> {
    let cfg = &state.cfg;
    let arc_factory = Address::from_str(&cfg.arc_factory)?;
    let arb_factory = Address::from_str(&cfg.arbitrum_sepolia_factory)?;
    let arc_rpc = cfg.arc_rpc_url.clone();
    let arb_rpc = cfg.arbitrum_sepolia_rpc_url.clone();
    let arc_diamond = predict_address(&arc_rpc, arc_factory, user).await?;
    let arb_diamond = predict_address(&arb_rpc, arb_factory, user).await?;
    let gateway_wallet = Address::from_str(&cfg.gateway_wallet)?;
    let gateway_minter = Address::from_str(&cfg.gateway_minter)?;
    let arc_usdc = Address::from_str(&cfg.arc_usdc)?;
    let arb_usdc = Address::from_str(&cfg.arbitrum_sepolia_usdc)?;
    let aave_usdc = state.arbitrum_sepolia.aave_usdc;

    let mut steps: Vec<WithdrawStep> = Vec::new();

    let aave_before = read_atoken_balance(&state, arb_diamond).await?;
    if !aave_before.is_zero() {
        let call: Bytes = IAaveFacet::withdrawAaveCall {
            amount: U256::MAX,
        }
        .abi_encode()
        .into();
        let tx = owner_send(&state, &arb_rpc, arb_diamond, call).await?;
        steps.push(WithdrawStep {
            label: "aave_withdraw".into(),
            chain: "arbitrum_sepolia",
            tx_hash: tx,
        });
    }

    let add_delegate: Bytes = IGatewayWallet::addDelegateCall {
        token: arb_usdc,
        delegate: state.agent_address,
    }
    .abi_encode()
    .into();
    let exec_call: Bytes = IAccount4337Facet::executeCall {
        target: gateway_wallet,
        value: U256::ZERO,
        data: add_delegate,
    }
    .abi_encode()
    .into();
    match owner_send(&state, &arb_rpc, arb_diamond, exec_call).await {
        Ok(tx) => steps.push(WithdrawStep {
            label: "gateway_delegate_authorized".into(),
            chain: "arbitrum_sepolia",
            tx_hash: tx,
        }),
        Err(_) => {}
    }

    let arb_diamond_usdc = read_usdc_balance(&state, arb_diamond, arb_usdc).await?;
    if arb_diamond_usdc.is_zero() {
        return Ok(Json(WithdrawResponse {
            user: format!("{user:?}"),
            arc_smart_account: format!("{arc_diamond:?}"),
            arbitrum_smart_account: format!("{arb_diamond:?}"),
            aave_balance_6dec: aave_before.to_string(),
            bridged_6dec: "0".into(),
            steps,
        }));
    }

    let deposit_call: Bytes = IGatewayFacet::depositToGatewayCall {
        amount: arb_diamond_usdc,
    }
    .abi_encode()
    .into();
    let deposit_tx = owner_send(&state, &arb_rpc, arb_diamond, deposit_call).await?;
    steps.push(WithdrawStep {
        label: "gateway_deposit".into(),
        chain: "arbitrum_sepolia",
        tx_hash: deposit_tx,
    });

    let intent = build_intent(IntentArgs {
        source_depositor: arb_diamond,
        destination_recipient: arc_diamond,
        source_token: arb_usdc,
        destination_token: arc_usdc,
        source_contract: gateway_wallet,
        destination_contract: gateway_minter,
        source_signer: state.agent_address,
        destination_caller: Address::ZERO,
        source_domain: domains::ARBITRUM_SEPOLIA,
        destination_domain: domains::ARC_TESTNET,
        amount: arb_diamond_usdc,
    });
    let signed = sign_burn_intent(state.agent_signer.as_ref(), intent).await?;
    let attestation = state.gateway.transfer(&signed).await?;
    steps.push(WithdrawStep {
        label: "burn_intent_attested".into(),
        chain: "arbitrum_sepolia",
        tx_hash: format!("0x{}", hex::encode(signed.digest.as_slice())),
    });

    let att_bytes: Bytes =
        hex::decode(attestation.attestation.trim_start_matches("0x"))?.into();
    let sig_bytes: Bytes =
        hex::decode(attestation.signature.trim_start_matches("0x"))?.into();
    let provider = ProviderBuilder::new()
        .wallet(state.agent_signer.as_ref().clone())
        .connect(&arc_rpc)
        .await?;
    let minter = IGatewayMinter::new(gateway_minter, provider);
    let pending = minter.gatewayMint(att_bytes, sig_bytes).send().await?;
    let mint_tx = format!("{:?}", pending.tx_hash());
    steps.push(WithdrawStep {
        label: "mint_on_arc".into(),
        chain: "arc",
        tx_hash: mint_tx,
    });

    tokio::time::sleep(Duration::from_secs(MINT_SETTLE_WAIT_SECS)).await;

    let _ = aave_usdc;
    let _ = ChainId::Arc;

    Ok(Json(WithdrawResponse {
        user: format!("{user:?}"),
        arc_smart_account: format!("{arc_diamond:?}"),
        arbitrum_smart_account: format!("{arb_diamond:?}"),
        aave_balance_6dec: aave_before.to_string(),
        bridged_6dec: arb_diamond_usdc.to_string(),
        steps,
    }))
}

async fn owner_send(
    state: &AppState,
    rpc_url: &str,
    diamond: Address,
    calldata: Bytes,
) -> Result<String> {
    let provider = ProviderBuilder::new()
        .wallet(state.user_signer.as_ref().clone())
        .connect(rpc_url)
        .await?;
    let tx = alloy::rpc::types::TransactionRequest::default()
        .to(diamond)
        .input(calldata.into());
    let pending = provider.send_transaction(tx).await?;
    let receipt = pending.get_receipt().await?;
    Ok(format!("{:?}", receipt.transaction_hash))
}

async fn read_atoken_balance(state: &AppState, diamond: Address) -> Result<U256, ApiError> {
    let provider = ProviderBuilder::new()
        .wallet(state.arbitrum_sepolia.signer.clone())
        .connect(&state.arbitrum_sepolia.rpc_url)
        .await?;
    let pool = IPool::new(state.arbitrum_sepolia.aave_pool, provider.clone());
    let data = pool
        .getReserveData(state.arbitrum_sepolia.aave_usdc)
        .call()
        .await?;
    let token = IERC20::new(data.aTokenAddress, provider);
    Ok(token.balanceOf(diamond).call().await?)
}

async fn read_usdc_balance(
    state: &AppState,
    holder: Address,
    usdc: Address,
) -> Result<U256, ApiError> {
    let provider = ProviderBuilder::new()
        .wallet(state.arbitrum_sepolia.signer.clone())
        .connect(&state.arbitrum_sepolia.rpc_url)
        .await?;
    let token = IERC20::new(usdc, provider);
    Ok(token.balanceOf(holder).call().await?)
}
