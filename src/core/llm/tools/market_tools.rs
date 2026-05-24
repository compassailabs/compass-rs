use std::str::FromStr;

use alloy::primitives::Address;
use anyhow::Result;
use serde_json::{Value, json};

use crate::aave::helpers::current_supply_apr;
use crate::account::predict_address;
use crate::core::llm::tool_context::ToolContext;

pub async fn check_balances(_args: &Value, ctx: &ToolContext) -> Result<String> {
    let cfg = &ctx.state.cfg;
    let arc_factory = Address::from_str(&cfg.arc_factory)?;
    let arb_factory = Address::from_str(&cfg.arbitrum_sepolia_factory)?;
    let arc_diamond = predict_address(&cfg.arc_rpc_url, arc_factory, ctx.user).await?;
    let arb_diamond = predict_address(&cfg.arbitrum_sepolia_rpc_url, arb_factory, ctx.user).await?;

    let arc_bal = ctx.state.arc.usdc_balance(arc_diamond).await?;
    let arb_bal = ctx.state.arbitrum_sepolia.usdc_balance(arb_diamond).await?;

    Ok(serde_json::to_string_pretty(&json!({
        "user": format!("{:?}", ctx.user),
        "arc_diamond": format!("{:?}", arc_diamond),
        "arb_diamond": format!("{:?}", arb_diamond),
        "arc_usdc_raw_6dec": arc_bal.to_string(),
        "arbitrum_sepolia_usdc_raw_6dec": arb_bal.to_string(),
        "note": "Raw 6-decimal units (divide by 1_000_000 for human USDC). Balances are the diamond's, not the Keeper's EOA."
    }))?)
}

pub async fn get_aave_apr(_args: &Value, ctx: &ToolContext) -> Result<String> {
    let apr = current_supply_apr(&ctx.state.arbitrum_sepolia, ctx.state.arbitrum_sepolia.aave_usdc).await?;
    Ok(serde_json::to_string_pretty(&json!({
        "market": "AAVE v3 USDC · Arbitrum Sepolia",
        "supply_apr": apr,
        "supply_apr_pct": format!("{:.2}%", apr * 100.0)
    }))?)
}
