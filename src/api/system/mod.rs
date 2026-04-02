mod handlers;

use axum::{routing::get, Router};

pub(crate) fn router() -> Router<super::AppState> {
    Router::new().route("/health", get(handlers::health))
}
