pub(crate) mod exports;
pub(crate) mod handlers;
pub(crate) mod models;
pub(crate) mod quality;

use axum::{routing::post, Router};

pub(crate) fn router() -> Router<super::AppState> {
    Router::new()
        .route("/exports/run", post(handlers::run_export))
        .route("/quality-checks/run", post(handlers::run_quality_check))
}
