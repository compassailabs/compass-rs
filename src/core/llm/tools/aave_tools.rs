use std::str::FromStr;

use alloy::primitives::{Address, Bytes, U256};
use alloy::sol_types::SolCall;
use anyhow::Result;
use serde_json::{Value, json};

use crate::account::predict_address;
use crate::contracts::IAaveFacet;
use crate::core::llm::tool_context::ToolContext;
use crate::core::llm::tools::parse_u256_usdc;
use crate::userop::{PaymasterConfig, build_userop, sign_and_submit};

pub async fn supply_aave(args: &Value, ctx: &ToolContext) -> Result<String> {
    let amount = parse_u256_usdc(args.get("amount_usdc"))?;
    let call: Bytes = IAaveFacet::supplyAaveCall { amount }.abi_encode().into();
    submit(ctx, call, amount).await
}

pub async fn withdraw_aave(args: &Value, ctx: &ToolContext) -> Result<String> {
    let raw = args
        .get("amount_usdc")
        .and_then(|v| v.as_str())
        .unwrap_or("max");
    let amount = if raw == "max" {
        U256::MAX
    } else {
        parse_u256_usdc(args.get("amount_usdc"))?
    };
    let call: Bytes = IAaveFacet::withdrawAaveCall { amount }.abi_encode().into();
    submit(ctx, call, amount).await
}

async fn submit(ctx: &ToolContext, call_data: Bytes, amount: U256) -> Result<String> {
    let cfg = &ctx.state.cfg;
    let factory = Address::from_str(&cfg.arbitrum_sepolia_factory)?;
    let diamond = predict_address(&cfg.arbitrum_sepolia_rpc_url, factory, ctx.user).await?;
    let entry_point = Address::from_str(&cfg.arbitrum_sepolia_entry_point)?;

    let paymaster = cfg
        .arbitrum_sepolia_paymaster
        .as_deref()
        .and_then(|s| Address::from_str(s).ok())
        .map(PaymasterConfig::for_compass);
    let userop = build_userop(
        &cfg.arbitrum_sepolia_rpc_url,
        ctx.state.agent_signer.as_ref().clone(),
        entry_point,
        diamond,
        call_data,
        paymaster,
    )
    .await?;

    let tx = sign_and_submit(
        &cfg.arbitrum_sepolia_rpc_url,
        ctx.state.agent_signer.as_ref().clone(),
        ctx.state.agent_signer.as_ref(),
        entry_point,
        userop,
        ctx.state.agent_address,
    )
    .await?;

    Ok(serde_json::to_string_pretty(&json!({
        "diamond": format!("{:?}", diamond),
        "amount_usdc_raw_6dec": amount.to_string(),
        "user_op_tx": tx
    }))?)
}
