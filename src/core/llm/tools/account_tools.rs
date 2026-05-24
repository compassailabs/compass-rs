use std::str::FromStr;

use alloy::primitives::Address;
use anyhow::Result;
use serde_json::{Value, json};

use crate::account::{
    ACCOUNT_SALT, arc_agent_selectors, arbitrum_agent_selectors, deploy, ensure_session,
    is_deployed, predict_address, validate_init,
};
use crate::contracts::InitArgs;
use crate::core::llm::tool_context::ToolContext;

pub async fn ensure_account(_args: &Value, ctx: &ToolContext) -> Result<String> {
    let cfg = &ctx.state.cfg;

    let arc_factory = Address::from_str(&cfg.arc_factory)?;
    let arb_factory = Address::from_str(&cfg.arbitrum_sepolia_factory)?;
    let arc_diamond = predict_address(&cfg.arc_rpc_url, arc_factory, ctx.user).await?;
    let arb_diamond = predict_address(&cfg.arbitrum_sepolia_rpc_url, arb_factory, ctx.user).await?;

    let upgrade_authority = Address::from_str(&cfg.compass_upgrade_authority)?;

    let arc_paymaster = cfg
        .arc_paymaster
        .as_deref()
        .map(Address::from_str)
        .transpose()?
        .unwrap_or(Address::ZERO);
    let arb_paymaster = cfg
        .arbitrum_sepolia_paymaster
        .as_deref()
        .map(Address::from_str)
        .transpose()?
        .unwrap_or(Address::ZERO);

    let arc_init = InitArgs {
        entryPoint: Address::from_str(&cfg.arc_entry_point)?,
        usdc: Address::from_str(&cfg.arc_usdc)?,
        gatewayWallet: Address::from_str(&cfg.gateway_wallet)?,
        gatewayMinter: Address::from_str(&cfg.gateway_minter)?,
        aavePool: Address::ZERO, // no AAVE on Arc in MVP
        upgradeAuthority: upgrade_authority,
        paymaster: arc_paymaster,
    };
    validate_init(&arc_init)?;

    let arb_init = InitArgs {
        entryPoint: Address::from_str(&cfg.arbitrum_sepolia_entry_point)?,
        usdc: Address::from_str(&cfg.arbitrum_sepolia_aave_usdc)?,
        gatewayWallet: Address::from_str(&cfg.gateway_wallet)?,
        gatewayMinter: Address::from_str(&cfg.gateway_minter)?,
        aavePool: Address::from_str(&cfg.arbitrum_sepolia_aave_pool)?,
        upgradeAuthority: upgrade_authority,
        paymaster: arb_paymaster,
    };
    validate_init(&arb_init)?;

    let (_, arc_deploy_tx) = deploy(
        &cfg.arc_rpc_url,
        ctx.state.agent_signer.as_ref().clone(),
        arc_factory,
        ctx.user,
        arc_init,
    )
    .await?;
    let (_, arb_deploy_tx) = deploy(
        &cfg.arbitrum_sepolia_rpc_url,
        ctx.state.agent_signer.as_ref().clone(),
        arb_factory,
        ctx.user,
        arb_init,
    )
    .await?;

    let expires_at = (chrono::Utc::now().timestamp() as u64) + 86400;

    let arc_session_tx = ensure_session(
        &cfg.arc_rpc_url,
        ctx.state.user_signer.as_ref().clone(),
        arc_diamond,
        ctx.state.agent_address,
        expires_at,
        &arc_agent_selectors(),
    )
    .await?;
    let arb_session_tx = ensure_session(
        &cfg.arbitrum_sepolia_rpc_url,
        ctx.state.user_signer.as_ref().clone(),
        arb_diamond,
        ctx.state.agent_address,
        expires_at,
        &arbitrum_agent_selectors(),
    )
    .await?;

    Ok(serde_json::to_string_pretty(&json!({
        "user": format!("{:?}", ctx.user),
        "agent": format!("{:?}", ctx.state.agent_address),
        "arc_diamond": format!("{:?}", arc_diamond),
        "arb_diamond": format!("{:?}", arb_diamond),
        "salt": ACCOUNT_SALT,
        "deploy": {
            "arc_tx": arc_deploy_tx,
            "arb_tx": arb_deploy_tx,
            "note": "null tx = already deployed"
        },
        "session": {
            "arc_tx": arc_session_tx,
            "arb_tx": arb_session_tx,
            "expires_at": expires_at,
            "note": "null tx = session already valid"
        }
    }))?)
}

pub async fn account_status(_args: &Value, ctx: &ToolContext) -> Result<String> {
    let cfg = &ctx.state.cfg;
    let arc_factory = Address::from_str(&cfg.arc_factory)?;
    let arb_factory = Address::from_str(&cfg.arbitrum_sepolia_factory)?;
    let arc_diamond = predict_address(&cfg.arc_rpc_url, arc_factory, ctx.user).await?;
    let arb_diamond = predict_address(&cfg.arbitrum_sepolia_rpc_url, arb_factory, ctx.user).await?;

    let arc_live = is_deployed(&cfg.arc_rpc_url, arc_diamond).await?;
    let arb_live = is_deployed(&cfg.arbitrum_sepolia_rpc_url, arb_diamond).await?;

    Ok(serde_json::to_string_pretty(&json!({
        "user": format!("{:?}", ctx.user),
        "agent": format!("{:?}", ctx.state.agent_address),
        "arc_diamond": { "address": format!("{:?}", arc_diamond), "deployed": arc_live },
        "arb_diamond": { "address": format!("{:?}", arb_diamond), "deployed": arb_live }
    }))?)
}
