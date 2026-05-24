use std::str::FromStr;

use alloy::primitives::{Address, Bytes, U256};
use alloy::providers::{Provider, ProviderBuilder};
use alloy::sol_types::SolCall;
use anyhow::Result;
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::post,
};
use chrono::Utc;
use serde::Serialize;

use crate::account::predict_address;
use crate::api::error::ApiError;
use crate::automation::funding::{FundingKind, NewFundingEvent};
use crate::automation::policy::ChainId;
use crate::chain::usdc::IERC20;
use crate::contracts::IAccount4337Facet;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/send-to-wallet/{user}", post(send_to_wallet))
}

#[derive(Serialize)]
pub struct SendToWalletResponse {
    pub user: String,
    pub smart_account: String,
    pub balance_before_6dec: String,
    pub sent_6dec: String,
    pub tx_hash: Option<String>,
}

async fn send_to_wallet(
    State(state): State<AppState>,
    Path(user): Path<Address>,
) -> Result<Json<SendToWalletResponse>, ApiError> {
    let cfg = &state.cfg;
    let arc_factory = Address::from_str(&cfg.arc_factory)?;
    let arc_rpc = cfg.arc_rpc_url.clone();
    let arc_diamond = predict_address(&arc_rpc, arc_factory, user).await?;
    let usdc = Address::from_str(&cfg.arc_usdc)?;

    let balance_before = read_arc_diamond_usdc(&state, arc_diamond, usdc).await?;
    if balance_before.is_zero() {
        return Ok(Json(SendToWalletResponse {
            user: format!("{user:?}"),
            smart_account: format!("{arc_diamond:?}"),
            balance_before_6dec: "0".into(),
            sent_6dec: "0".into(),
            tx_hash: None,
        }));
    }

    let transfer_call: Bytes = IERC20::transferCall {
        to: user,
        amount: balance_before,
    }
    .abi_encode()
    .into();
    let outer: Bytes = IAccount4337Facet::executeCall {
        target: usdc,
        value: U256::ZERO,
        data: transfer_call,
    }
    .abi_encode()
    .into();

    let provider = ProviderBuilder::new()
        .wallet(state.user_signer.as_ref().clone())
        .connect(&arc_rpc)
        .await?;
    let tx = alloy::rpc::types::TransactionRequest::default()
        .to(arc_diamond)
        .input(outer.into());
    let pending = provider.send_transaction(tx).await?;
    let receipt = pending.get_receipt().await?;
    let tx_hash = format!("{:?}", receipt.transaction_hash);

    let _ = state
        .funding
        .append(NewFundingEvent::new(
            user,
            ChainId::Arc,
            FundingKind::WithdrawToEoa,
            balance_before,
            tx_hash.clone(),
            Utc::now(),
        ))
        .await;

    Ok(Json(SendToWalletResponse {
        user: format!("{user:?}"),
        smart_account: format!("{arc_diamond:?}"),
        balance_before_6dec: balance_before.to_string(),
        sent_6dec: balance_before.to_string(),
        tx_hash: Some(tx_hash),
    }))
}

async fn read_arc_diamond_usdc(
    state: &AppState,
    diamond: Address,
    usdc: Address,
) -> Result<U256, ApiError> {
    let provider = ProviderBuilder::new()
        .wallet(state.arc.signer.clone())
        .connect(&state.arc.rpc_url)
        .await?;
    let token = IERC20::new(usdc, provider);
    Ok(token.balanceOf(diamond).call().await?)
}
