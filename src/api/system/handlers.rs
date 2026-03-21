use axum::{extract::State, http::StatusCode, Json};
use sqlx::query;

use crate::api::{shared::error::HealthResponse, AppState};

pub(crate) async fn health(
    State(state): State<AppState>,
) -> Result<Json<HealthResponse>, StatusCode> {
    if let Err(_err) = query("SELECT 1").execute(&state.pool).await {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }

    Ok(Json(HealthResponse {
        status: "ok",
        service: "qb_api_rust",
    }))
}
