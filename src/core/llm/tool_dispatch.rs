use anyhow::Result;
use serde_json::Value;

use super::tool_context::ToolContext;
use super::tools::{aave_tools, account_tools, gateway_tools, market_tools, skill_tools};

pub async fn execute_tool(name: &str, args: &Value, ctx: &ToolContext) -> Result<String> {
    match name {
        "load_skill" => skill_tools::load_skill(args, ctx).await,
        "account_status" => account_tools::account_status(args, ctx).await,
        "ensure_account" => account_tools::ensure_account(args, ctx).await,
        "check_balances" => market_tools::check_balances(args, ctx).await,
        "get_aave_apr" => market_tools::get_aave_apr(args, ctx).await,
        "deposit_to_gateway" => gateway_tools::deposit_to_gateway(args, ctx).await,
        "burn_intent_to_attestation" => gateway_tools::burn_intent_to_attestation(args, ctx).await,
        "mint_on_destination" => gateway_tools::mint_on_destination(args, ctx).await,
        "supply_aave" => aave_tools::supply_aave(args, ctx).await,
        "withdraw_aave" => aave_tools::withdraw_aave(args, ctx).await,
        other => Ok(format!("Unknown tool: {other}")),
    }
}
