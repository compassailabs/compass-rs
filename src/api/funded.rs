use std::str::FromStr;

use alloy::primitives::{Address, U256};
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::post,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::api::error::ApiError;
use crate::automation::funding::{FundingKind, NewFundingEvent};
use crate::automation::policy::ChainId;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/funded/{user}", post(record_funded))
}

#[derive(Deserialize)]
pub struct FundedRequest {
    pub chain: String,
    pub kind: String,
    pub amount_6dec: String,
    pub tx_hash: String,
}

#[derive(Serialize)]
pub struct FundedResponse {
    pub id: u64,
    pub user: String,
    pub chain: String,
    pub kind: String,
    pub amount_6dec: String,
    pub tx_hash: String,
}

async fn record_funded(
    State(state): State<AppState>,
    Path(user): Path<Address>,
    Json(body): Json<FundedRequest>,
) -> Result<Json<FundedResponse>, ApiError> {
    let chain = parse_chain(&body.chain)?;
    let kind = parse_kind(&body.kind)?;
    let amount = U256::from_str(&body.amount_6dec)
        .map_err(|e| ApiError::from(anyhow::anyhow!("bad amount_6dec: {e}")))?;
    let tx_hash = body.tx_hash.trim().to_string();
    if tx_hash.is_empty() {
        return Err(ApiError::from(anyhow::anyhow!("tx_hash required")));
    }
    let id = state
        .funding
        .append(NewFundingEvent::new(
            user,
            chain,
            kind,
            amount,
            tx_hash.clone(),
            Utc::now(),
        ))
        .await?;
    Ok(Json(FundedResponse {
        id,
        user: format!("{user:?}"),
        chain: body.chain,
        kind: body.kind,
        amount_6dec: amount.to_string(),
        tx_hash,
    }))
}

fn parse_chain(s: &str) -> Result<ChainId, ApiError> {
    match s {
        "arc" => Ok(ChainId::Arc),
        "arbitrum_sepolia" => Ok(ChainId::ArbitrumSepolia),
        other => Err(ApiError::from(anyhow::anyhow!("unknown chain: {other}"))),
    }
}

fn parse_kind(s: &str) -> Result<FundingKind, ApiError> {
    match s {
        "deposit" => Ok(FundingKind::Deposit),
        "withdraw_to_eoa" => Ok(FundingKind::WithdrawToEoa),
        other => Err(ApiError::from(anyhow::anyhow!("unknown kind: {other}"))),
    }
}
