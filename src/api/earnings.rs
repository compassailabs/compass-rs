use std::str::FromStr;

use alloy::primitives::{Address, U256};
use alloy::providers::ProviderBuilder;
use anyhow::Result;
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use serde::Serialize;

use crate::account::predict_address;
use crate::aave::pool::IPool;
use crate::api::error::ApiError;
use crate::automation::funding::net_deposited;
use crate::chain::usdc::IERC20;
use crate::state::AppState;

const PERFORMANCE_FEE_PCT: u32 = 0;

pub fn router() -> Router<AppState> {
    Router::new().route("/earnings/{user}", get(get_earnings))
}

#[derive(Serialize)]
pub struct EarningsResponse {
    pub user: String,
    pub smart_account: String,
    pub net_deposited_6dec: String,
    pub current_value_6dec: String,
    pub gross_earned_6dec: String,
    pub performance_fee_pct: u32,
    pub fee_6dec: String,
    pub net_earned_6dec: String,
}

async fn get_earnings(
    State(state): State<AppState>,
    Path(user): Path<Address>,
) -> Result<Json<EarningsResponse>, ApiError> {
    let cfg = &state.cfg;
    let arc_factory = Address::from_str(&cfg.arc_factory)?;
    let arb_factory = Address::from_str(&cfg.arbitrum_sepolia_factory)?;
    let arc_diamond = predict_address(&cfg.arc_rpc_url, arc_factory, user).await?;
    let arb_diamond =
        predict_address(&cfg.arbitrum_sepolia_rpc_url, arb_factory, user).await?;
    let arc_usdc = Address::from_str(&cfg.arc_usdc)?;
    let arb_usdc = Address::from_str(&cfg.arbitrum_sepolia_usdc)?;

    let (
        net_deposited_r,
        balance_arc_r,
        balance_arb_r,
        aave_balance_r,
    ) = tokio::join!(
        net_deposited(state.funding.as_ref(), user),
        read_erc20_balance(&cfg.arc_rpc_url, arc_usdc, arc_diamond),
        read_erc20_balance(&cfg.arbitrum_sepolia_rpc_url, arb_usdc, arb_diamond),
        read_atoken_balance(&state, arb_diamond),
    );

    let net_deposited = net_deposited_r?;
    let arc_balance = balance_arc_r?;
    let arb_balance = balance_arb_r?;
    let aave_balance = aave_balance_r?;

    let current_value = arc_balance + arb_balance + aave_balance;

    let (gross_str, gross_positive) = if current_value >= net_deposited {
        (
            (current_value - net_deposited).to_string(),
            true,
        )
    } else {
        (
            format!("-{}", net_deposited - current_value),
            false,
        )
    };

    let (fee, net_earned_str) = if gross_positive {
        let gross = current_value - net_deposited;
        let fee = gross * U256::from(PERFORMANCE_FEE_PCT) / U256::from(100u32);
        let net = gross - fee;
        (fee, net.to_string())
    } else {
        (U256::ZERO, gross_str.clone())
    };

    let _ = arb_diamond;
    Ok(Json(EarningsResponse {
        user: format!("{user:?}"),
        smart_account: format!("{arc_diamond:?}"),
        net_deposited_6dec: net_deposited.to_string(),
        current_value_6dec: current_value.to_string(),
        gross_earned_6dec: gross_str,
        performance_fee_pct: PERFORMANCE_FEE_PCT,
        fee_6dec: fee.to_string(),
        net_earned_6dec: net_earned_str,
    }))
}

async fn read_erc20_balance(rpc_url: &str, token: Address, holder: Address) -> Result<U256> {
    let provider = ProviderBuilder::new().connect(rpc_url).await?;
    let erc20 = IERC20::new(token, provider);
    Ok(erc20.balanceOf(holder).call().await?)
}

async fn read_atoken_balance(state: &AppState, diamond: Address) -> Result<U256> {
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
