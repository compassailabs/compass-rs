use std::str::FromStr;

use alloy::primitives::{Address, U256};
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use serde::Serialize;

use crate::account::predict_address;
use crate::api::error::ApiError;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/balance/{user}", get(get_balance))
}

#[derive(Serialize)]
pub struct BalanceResponse {
    pub user: String,
    pub smart_account: String,
    pub arc_usdc_6dec: String,
    pub arbitrum_sepolia_usdc_6dec: String,
    pub arc_usdc: String,
    pub arbitrum_sepolia_usdc: String,
    pub has_funds: bool,
}

async fn get_balance(
    State(state): State<AppState>,
    Path(user): Path<Address>,
) -> Result<Json<BalanceResponse>, ApiError> {
    let cfg = &state.cfg;
    let arc_factory = Address::from_str(&cfg.arc_factory)?;
    let arb_factory = Address::from_str(&cfg.arbitrum_sepolia_factory)?;

    let arc_state = state.arc.clone();
    let arbitrum_state = state.arbitrum_sepolia.clone();
    let arc_rpc = cfg.arc_rpc_url.clone();
    let arbitrum_rpc = cfg.arbitrum_sepolia_rpc_url.clone();
    let (arc_result, arb_result) = tokio::join!(
        async move {
            let diamond = predict_address(&arc_rpc, arc_factory, user).await?;
            let bal = arc_state
                .usdc_balance(diamond)
                .await
                .unwrap_or(U256::ZERO);
            Ok::<_, anyhow::Error>((diamond, bal))
        },
        async move {
            let diamond = predict_address(&arbitrum_rpc, arb_factory, user).await?;
            let bal = arbitrum_state
                .usdc_balance(diamond)
                .await
                .unwrap_or(U256::ZERO);
            Ok::<_, anyhow::Error>((diamond, bal))
        },
    );
    let (arc_diamond, arc_bal) = arc_result?;
    let (_arb_diamond, arb_bal) = arb_result?;

    let has_funds = arc_bal > U256::ZERO || arb_bal > U256::ZERO;

    Ok(Json(BalanceResponse {
        user: format!("{user:?}"),
        smart_account: format!("{arc_diamond:?}"),
        arc_usdc_6dec: arc_bal.to_string(),
        arbitrum_sepolia_usdc_6dec: arb_bal.to_string(),
        arc_usdc: format_usdc(arc_bal),
        arbitrum_sepolia_usdc: format_usdc(arb_bal),
        has_funds,
    }))
}

fn format_usdc(raw: U256) -> String {
    let whole = raw / U256::from(1_000_000u64);
    let frac = raw % U256::from(1_000_000u64);
    format!("{whole}.{frac:0>6}")
}
