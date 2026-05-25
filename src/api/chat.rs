use std::str::FromStr;

use std::convert::Infallible;
use std::time::Duration;

use alloy::primitives::{Address, U256};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, post},
};
use futures::StreamExt;
use serde::Deserialize;

use crate::account::predict_address_with_provider;
use crate::api::error::ApiError;
use crate::automation::chat_history::{
    ChatRole as DbChatRole, ChatTurnRow, NewChatTurn,
};
use crate::core::llm::chat_agent::{ChatRole, ChatTurn, stream_chat_agent};
use crate::core::llm::skills::RiskProfile;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/chat/{user}", post(chat_stream))
        .route("/chat/{user}/history", get(history).delete(clear_history))
}

#[derive(Deserialize)]
struct ChatRequest {
    message: String,
}

#[derive(Deserialize)]
struct HistoryQuery {
    limit: Option<usize>,
}

const DEFAULT_HISTORY_LIMIT: usize = 50;

async fn chat_stream(
    State(state): State<AppState>,
    Path(user): Path<Address>,
    Json(req): Json<ChatRequest>,
) -> Result<Response, ApiError> {
    if let Err(e) = state
        .chat_history
        .append(NewChatTurn {
            user,
            role: DbChatRole::User,
            text: req.message.clone(),
            trace: None,
        })
        .await
    {
        tracing::warn!(user = %user, error = %e, "failed to persist user turn");
    }

    let rows = state
        .chat_history
        .list_for_user(user, DEFAULT_HISTORY_LIMIT)
        .await?;
    let mut history: Vec<ChatTurn> = rows
        .into_iter()
        .map(|r| ChatTurn {
            role: match r.role {
                DbChatRole::User => ChatRole::User,
                DbChatRole::Assistant => ChatRole::Assistant,
            },
            text: r.text,
        })
        .collect();
    if matches!(history.last(), Some(t) if t.role == ChatRole::User && t.text == req.message) {
        history.pop();
    }

    let live_state = build_live_state(&state, user).await;
    let llm = state.llm.clone();
    let event_stream =
        stream_chat_agent(llm, state, user, RiskProfile::Balanced, history, req.message, live_state)
            .map(|ev: Event| Ok::<Event, Infallible>(ev));

    let sse = Sse::new(event_stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    );

    let mut resp = sse.into_response();
    resp.headers_mut()
        .insert("X-Accel-Buffering", "no".parse().unwrap());
    Ok(resp)
}

async fn build_live_state(state: &AppState, user: Address) -> Option<String> {
    let cfg = &state.cfg;
    let arc_factory = Address::from_str(&cfg.arc_factory).ok()?;
    let arb_factory = Address::from_str(&cfg.arbitrum_sepolia_factory).ok()?;
    let arc_diamond =
        predict_address_with_provider(state.arc.read_provider(), arc_factory, user)
            .await
            .ok()?;
    let arb_diamond = predict_address_with_provider(
        state.arbitrum_sepolia.read_provider(),
        arb_factory,
        user,
    )
    .await
    .ok()?;
    let arc_bal = state
        .arc
        .usdc_balance(arc_diamond)
        .await
        .unwrap_or(U256::ZERO);
    let arb_bal = state
        .arbitrum_sepolia
        .usdc_balance(arb_diamond)
        .await
        .unwrap_or(U256::ZERO);

    let total = arc_bal + arb_bal;
    let fund_hint = if total == U256::ZERO {
        "\n\n**The user's smart account has 0 USDC on both chains.** \
         If they want to start earning, tell them to fund their smart \
         account first — do NOT call `commit_policy` until they have \
         balance. Their wallet button in the UI has a Fund action."
    } else {
        ""
    };

    Some(format!(
        "- **Wallet (EOA)**: `{user:?}`\n\
         - **Compass smart account** (same address on both chains): `{arc_diamond:?}`\n\
         - **Arc USDC balance**: {arc} (raw 6-dec: {arc_raw})\n\
         - **Arbitrum Sepolia USDC balance**: {arb} (raw 6-dec: {arb_raw})\n\
         {fund_hint}",
        arc = format_usdc(arc_bal),
        arc_raw = arc_bal,
        arb = format_usdc(arb_bal),
        arb_raw = arb_bal,
    ))
}

fn format_usdc(raw: U256) -> String {
    let whole = raw / U256::from(1_000_000u64);
    let frac = raw % U256::from(1_000_000u64);
    format!("{whole}.{frac:0>6}")
}

async fn history(
    State(state): State<AppState>,
    Path(user): Path<Address>,
    Query(q): Query<HistoryQuery>,
) -> Result<Json<Vec<ChatTurnRow>>, ApiError> {
    let limit = q.limit.unwrap_or(DEFAULT_HISTORY_LIMIT).min(500);
    let rows = state.chat_history.list_for_user(user, limit).await?;
    Ok(Json(rows))
}

async fn clear_history(
    State(state): State<AppState>,
    Path(user): Path<Address>,
) -> Result<StatusCode, ApiError> {
    state.chat_history.clear_for_user(user).await?;
    Ok(StatusCode::NO_CONTENT)
}
