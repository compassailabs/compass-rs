use std::str::FromStr;

use alloy::primitives::Address;
use axum::{
    Json, Router,
    extract::State,
    routing::post,
};
use serde::{Deserialize, Serialize};

use crate::api::error::ApiError;
use crate::core::llm::chat_engine::{AgentResult, run_agent};
use crate::core::llm::skills::RiskProfile;
use crate::core::llm::tool_context::ToolContext;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/strategy/propose", post(propose))
        .route("/strategy/execute", post(execute))
        .route("/strategy/rebalance", post(rebalance))
}

#[derive(Deserialize)]
pub struct StrategyRequest {
    pub user: String,
    #[serde(default)]
    pub risk: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Serialize)]
pub struct StrategyResponse {
    pub risk: &'static str,
    pub agent: AgentResult,
}

async fn propose(
    State(state): State<AppState>,
    Json(req): Json<StrategyRequest>,
) -> Result<Json<StrategyResponse>, ApiError> {
    let (risk, ctx) = ctx_from(&state, &req)?;
    let user_msg = req.message.unwrap_or_else(|| {
        "I want to put my USDC to work. Read my current state and propose the best plan under my active risk profile. \
         Do NOT execute any write actions — give me a concrete plan with the exact tool calls you'd make and the projected APR.".into()
    });
    let result = run_agent(state.llm.as_ref(), &ctx, risk, &user_msg, None).await?;
    Ok(Json(StrategyResponse { risk: risk.label(), agent: result }))
}

async fn execute(
    State(state): State<AppState>,
    Json(req): Json<StrategyRequest>,
) -> Result<Json<StrategyResponse>, ApiError> {
    let (risk, ctx) = ctx_from(&state, &req)?;
    let user_msg = req.message.unwrap_or_else(|| {
        "Execute the plan you'd otherwise propose: read state, decide, and run the full sequence of \
         move + supply tool calls. Stop and report the final tx hashes when done.".into()
    });
    let result = run_agent(state.llm.as_ref(), &ctx, risk, &user_msg, None).await?;
    Ok(Json(StrategyResponse { risk: risk.label(), agent: result }))
}

async fn rebalance(
    State(state): State<AppState>,
    Json(req): Json<StrategyRequest>,
) -> Result<Json<StrategyResponse>, ApiError> {
    let (risk, ctx) = ctx_from(&state, &req)?;
    let user_msg = req.message.unwrap_or_else(|| {
        "Check my current AAVE position and the live APR. Compared to my active risk profile's rules, \
         is a rebalance worth it right now? If yes, run it. If no, explain why and stop.".into()
    });
    let result = run_agent(state.llm.as_ref(), &ctx, risk, &user_msg, None).await?;
    Ok(Json(StrategyResponse { risk: risk.label(), agent: result }))
}

fn ctx_from(state: &AppState, req: &StrategyRequest) -> Result<(RiskProfile, ToolContext), ApiError> {
    let user = Address::from_str(&req.user).map_err(|e| ApiError(anyhow::anyhow!("bad user address: {e}")))?;
    let risk = req.risk.as_deref().map(RiskProfile::parse).unwrap_or(RiskProfile::Balanced);
    Ok((risk, ToolContext::new(state.clone(), user, risk)))
}
