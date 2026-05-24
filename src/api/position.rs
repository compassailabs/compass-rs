use alloy::primitives::Address;
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};

use crate::api::error::ApiError;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/position/{user}", get(get_position))
}

async fn get_position(
    State(state): State<AppState>,
    Path(user): Path<Address>,
) -> Result<Response, ApiError> {
    if let Some(p) = state.positions.get(user).await? {
        return Ok(Json(p).into_response());
    }
    match state.position_fetcher.fetch(user).await {
        Ok(p) => {
            let _ = state.positions.put(user, p.clone()).await;
            Ok(Json(p).into_response())
        }
        Err(e) => Ok((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "no position for user",
                "fetch_error": e.to_string(),
            })),
        )
            .into_response()),
    }
}
