use alloy::primitives::Address;
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};
use serde::Serialize;

use crate::api::error::ApiError;
use crate::automation::evaluator::{Position, Snapshot};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/debug/snapshot", post(put_snapshot).get(get_snapshot))
        .route("/debug/position/{user}", post(put_position))
}

#[derive(Serialize)]
struct OkResponse {
    ok: bool,
}

async fn put_snapshot(
    State(state): State<AppState>,
    Json(snapshot): Json<Snapshot>,
) -> Result<Json<OkResponse>, ApiError> {
    state.snapshots.put(snapshot).await?;
    Ok(Json(OkResponse { ok: true }))
}

async fn get_snapshot(State(state): State<AppState>) -> Result<Response, ApiError> {
    match state.snapshots.latest().await? {
        Some(s) => Ok(Json(s).into_response()),
        None => Ok((StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "no snapshot" }))).into_response()),
    }
}

async fn put_position(
    State(state): State<AppState>,
    Path(user): Path<Address>,
    Json(position): Json<Position>,
) -> Result<Json<OkResponse>, ApiError> {
    state.positions.put(user, position).await?;
    Ok(Json(OkResponse { ok: true }))
}
