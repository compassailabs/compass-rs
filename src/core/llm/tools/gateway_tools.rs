use std::str::FromStr;

use alloy::primitives::{Address, Bytes};
use alloy::providers::ProviderBuilder;
use alloy::sol_types::SolCall;
use anyhow::{Result, anyhow};
use serde_json::{Value, json};

use crate::account::predict_address;
use crate::contracts::IGatewayFacet;
use crate::core::llm::tool_context::ToolContext;
use crate::core::llm::tools::parse_u256_usdc;
use crate::gateway::contracts::IGatewayMinter;
use crate::gateway::domains;
use crate::gateway::intent::{build_intent, sign_burn_intent};
use crate::userop::{build_userop, sign_and_submit};

pub async fn deposit_to_gateway(args: &Value, ctx: &ToolContext) -> Result<String> {
    let amount = parse_u256_usdc(args.get("amount_usdc"))?;
    let cfg = &ctx.state.cfg;

    let factory = Address::from_str(&cfg.arc_factory)?;
    let diamond = predict_address(&cfg.arc_rpc_url, factory, ctx.user).await?;
    let entry_point = Address::from_str(&cfg.arc_entry_point)?;

    let call_data: Bytes = IGatewayFacet::depositToGatewayCall { amount }
        .abi_encode()
        .into();

    let userop = build_userop(
        &cfg.arc_rpc_url,
        ctx.state.agent_signer.as_ref().clone(),
        entry_point,
        diamond,
        call_data,
        None,
    )
    .await?;

    let tx = sign_and_submit(
        &cfg.arc_rpc_url,
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
        "user_op_tx": tx,
        "next_step": "Call burn_intent_to_attestation, then mint_on_destination."
    }))?)
}

pub async fn burn_intent_to_attestation(args: &Value, ctx: &ToolContext) -> Result<String> {
    let amount = parse_u256_usdc(args.get("amount_usdc"))?;
    let dest = args
        .get("destination_domain")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| anyhow!("missing destination_domain"))? as u32;

    let cfg = &ctx.state.cfg;
    let arc_factory = Address::from_str(&cfg.arc_factory)?;
    let arb_factory = Address::from_str(&cfg.arbitrum_sepolia_factory)?;
    let arc_diamond = predict_address(&cfg.arc_rpc_url, arc_factory, ctx.user).await?;
    let arb_diamond = predict_address(&cfg.arbitrum_sepolia_rpc_url, arb_factory, ctx.user).await?;

    let gateway_wallet = Address::from_str(&cfg.gateway_wallet)?;
    let gateway_minter = Address::from_str(&cfg.gateway_minter)?;
    let intent = build_intent(crate::gateway::intent::IntentArgs {
        source_depositor: arc_diamond,
        destination_recipient: arb_diamond,
        source_token: ctx.state.arc.usdc,
        destination_token: Address::from_str(&cfg.arbitrum_sepolia_usdc)?,
        source_contract: gateway_wallet,
        destination_contract: gateway_minter,
        source_signer: ctx.state.agent_address,
        destination_caller: Address::ZERO,
        source_domain: domains::ARC_TESTNET,
        destination_domain: dest,
        amount,
    });
    let signed = sign_burn_intent(ctx.state.agent_signer.as_ref(), intent).await?;
    let att = ctx.state.gateway.transfer(&signed).await?;

    Ok(serde_json::to_string_pretty(&json!({
        "from_diamond": format!("{:?}", arc_diamond),
        "to_diamond": format!("{:?}", arb_diamond),
        "destination_domain": dest,
        "attestation": att.attestation,
        "signature": att.signature
    }))?)
}

pub async fn mint_on_destination(args: &Value, ctx: &ToolContext) -> Result<String> {
    let dest = args
        .get("destination_domain")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| anyhow!("missing destination_domain"))? as u32;
    let attestation_hex = args
        .get("attestation")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("missing attestation"))?;
    let signature_hex = args
        .get("signature")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("missing signature"))?;

    let cfg = &ctx.state.cfg;
    let minter_addr = Address::from_str(&cfg.gateway_minter)?;
    let att: Bytes = hex::decode(attestation_hex.trim_start_matches("0x"))?.into();
    let sig: Bytes = hex::decode(signature_hex.trim_start_matches("0x"))?.into();

    let (rpc, label) = match dest {
        domains::ARBITRUM_SEPOLIA => (cfg.arbitrum_sepolia_rpc_url.clone(), "Arbitrum Sepolia"),
        other => return Err(anyhow!("destination domain {other} not wired in MVP")),
    };

    let provider = ProviderBuilder::new()
        .wallet(ctx.state.agent_signer.as_ref().clone())
        .connect(&rpc)
        .await?;
    let m = IGatewayMinter::new(minter_addr, provider);
    let pending = m.gatewayMint(att, sig).send().await?;
    Ok(serde_json::to_string_pretty(&json!({
        "destination": label,
        "mint_tx": format!("{:?}", pending.tx_hash()),
        "note": "USDC now lives in the destination diamond. Next: supply_aave."
    }))?)
}
