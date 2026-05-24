use axum::{Json, Router, extract::State, routing::get};
use serde::Serialize;

use crate::aave::helpers::current_supply_apr;
use crate::api::error::ApiError;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/markets", get(list_markets))
}

#[derive(Serialize)]
pub struct MarketEntry {
    pub chain: &'static str,
    pub protocol: &'static str,
    pub label: String,
    pub apr: f64,
    pub is_yield_venue: bool,
    pub status: &'static str,
}

#[derive(Serialize)]
pub struct MarketsResponse {
    pub markets: Vec<MarketEntry>,
}

async fn list_markets(
    State(state): State<AppState>,
) -> Result<Json<MarketsResponse>, ApiError> {
    let aave_apr =
        current_supply_apr(&state.arbitrum_sepolia, state.arbitrum_sepolia.aave_usdc)
            .await
            .unwrap_or(0.0);

    let markets = vec![
        MarketEntry {
            chain: "arc",
            protocol: "idle",
            label: "Wallet on Arc".into(),
            apr: 0.0,
            is_yield_venue: false,
            status: "live",
        },
        MarketEntry {
            chain: "arbitrum_sepolia",
            protocol: "idle",
            label: "Wallet on Arbitrum Sepolia".into(),
            apr: 0.0,
            is_yield_venue: false,
            status: "live",
        },
        MarketEntry {
            chain: "arbitrum_sepolia",
            protocol: "aave_v3",
            label: "AAVE v3 on Arbitrum Sepolia".into(),
            apr: aave_apr,
            is_yield_venue: true,
            status: "live",
        },
    ];

    Ok(Json(MarketsResponse { markets }))
}
