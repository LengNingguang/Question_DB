//! HTTP API composition for the question bank service.

mod ops;
mod papers;
mod questions;
mod shared;
mod system;
mod tests;

use axum::{extract::DefaultBodyLimit, Router};
use sqlx::PgPool;

pub use self::papers::models::{PaperDetail, PaperQuestionSummary, PaperSummary};
pub use self::questions::models::{
    QuestionAssetRef, QuestionDetail, QuestionPaperRef, QuestionSummary,
};

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
}

/// Build the complete Axum router for the service.
pub fn router(state: AppState) -> Router {
    Router::new()
        .merge(system::router())
        .merge(papers::router())
        .merge(questions::router())
        .merge(ops::router())
        .layer(DefaultBodyLimit::max(
            questions::MAX_UPLOAD_BYTES.max(papers::MAX_UPLOAD_BYTES),
        ))
        .with_state(state)
}
