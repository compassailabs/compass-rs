use axum::{Json, Router, extract::State, routing::get};
use serde::Serialize;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/health", get(health))
}

#[derive(Serialize)]
struct Health {
    status: &'static str,
    user: String,
    agent: String,
    arc_rpc: String,
    arc_factory: String,
    arc_entry_point: String,
    base_rpc: String,
    arb_factory: String,
    arb_entry_point: String,
    gateway_api: String,
    model: String,
}

async fn health(State(state): State<AppState>) -> Json<Health> {
    Json(Health {
        status: "ok",
        user: format!("{:?}", state.user_address),
        agent: format!("{:?}", state.agent_address),
        arc_rpc: state.cfg.arc_rpc_url.clone(),
        arc_factory: state.cfg.arc_factory.clone(),
        arc_entry_point: state.cfg.arc_entry_point.clone(),
        base_rpc: state.cfg.arbitrum_sepolia_rpc_url.clone(),
        arb_factory: state.cfg.arbitrum_sepolia_factory.clone(),
        arb_entry_point: state.cfg.arbitrum_sepolia_entry_point.clone(),
        gateway_api: state.cfg.gateway_api_url.clone(),
        model: state.cfg.anthropic_model.clone(),
    })
}
