use alloy::primitives::Address;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::api::error::ApiError;
use crate::automation::audit::AuditEvent;
use crate::automation::policy::{Policy, PolicyStatus};
use crate::automation::scheduler::tick_user_now;
use crate::automation::tick::{TickOutcome, tick_once};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/policy/{user}", post(put_policy).get(get_policy))
        .route("/policy/{user}/pause", post(pause))
        .route("/policy/{user}/resume", post(resume))
        .route("/policy/{user}/run", post(run_once))
        .route("/policy/{user}/audit", get(list_audit))
}

#[derive(Serialize)]
struct PutResponse {
    version: u32,
}

#[derive(Serialize)]
struct StatusResponse {
    status: PolicyStatus,
}

async fn put_policy(
    State(state): State<AppState>,
    Path(user): Path<Address>,
    Json(mut policy): Json<Policy>,
) -> Result<Json<PutResponse>, ApiError> {
    policy.user = user;
    let version = state.policies.put(policy).await?;
    let state_clone = state.clone();
    tokio::spawn(async move {
        tick_user_now(&state_clone, user).await;
    });

    Ok(Json(PutResponse { version }))
}

async fn get_policy(
    State(state): State<AppState>,
    Path(user): Path<Address>,
) -> Result<Response, ApiError> {
    match state.policies.get(user).await? {
        Some(p) => Ok(Json(p).into_response()),
        None => Ok((StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "no policy for user" }))).into_response()),
    }
}

async fn pause(
    State(state): State<AppState>,
    Path(user): Path<Address>,
) -> Result<Json<StatusResponse>, ApiError> {
    state.policies.set_status(user, PolicyStatus::Paused).await?;
    Ok(Json(StatusResponse {
        status: PolicyStatus::Paused,
    }))
}

async fn resume(
    State(state): State<AppState>,
    Path(user): Path<Address>,
) -> Result<Json<StatusResponse>, ApiError> {
    state.policies.set_status(user, PolicyStatus::Active).await?;
    Ok(Json(StatusResponse {
        status: PolicyStatus::Active,
    }))
}

async fn run_once(
    State(state): State<AppState>,
    Path(user): Path<Address>,
) -> Result<Json<TickOutcome>, ApiError> {
    match state.position_fetcher.fetch(user).await {
        Ok(pos) => {
            if let Err(e) = state.positions.put(user, pos).await {
                tracing::warn!(user = %user, error = %e, "position put failed");
            }
        }
        Err(e) => {
            tracing::warn!(user = %user, error = %e, "manual run: position fetch failed; using stored")
        }
    }
    let outcome = tick_once(
        &*state.policies,
        &*state.snapshots,
        &*state.positions,
        &*state.audit,
        user,
        "manual",
        Utc::now(),
    )
    .await?;
    Ok(Json(outcome))
}

#[derive(Deserialize)]
struct AuditQuery {
    since: Option<i64>,
    limit: Option<usize>,
}

async fn list_audit(
    State(state): State<AppState>,
    Path(user): Path<Address>,
    Query(q): Query<AuditQuery>,
) -> Result<Json<Vec<AuditEvent>>, ApiError> {
    let since: Option<DateTime<Utc>> = q.since.and_then(|s| DateTime::from_timestamp(s, 0));
    let limit = q.limit.unwrap_or(50);
    let events = state.audit.list_for_user(user, since, limit).await?;
    Ok(Json(events))
}
