//! HTTP API composition for the question bank service.
//!
//! This module keeps the public API surface small:
//! - `AppState` carries shared dependencies.
//! - `router` assembles all routes.
//! - submodules own handlers, data models, and domain helpers.

mod error;
mod exports;
mod handlers;
mod imports;
mod models;
mod quality;
mod queries;
mod tests;
mod utils;

use axum::{
    routing::{get, post, put},
    Router,
};
use sqlx::PgPool;

pub use self::models::{
    PaperDetail, PaperQuestionSummary, PaperSummary, QuestionAsset, QuestionDetail,
    QuestionPaperRef, QuestionSummary,
};

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
}

/// Build the complete Axum router for the service.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(handlers::health))
        .route("/papers", get(handlers::list_papers).post(handlers::create_paper))
        .route("/papers/:paper_id", get(handlers::get_paper_detail))
        .route("/papers/:paper_id/questions", put(handlers::replace_paper_questions))
        .route("/questions", get(handlers::list_questions))
        .route(
            "/questions/:question_id",
            get(handlers::get_question_detail),
        )
        .route("/search", get(handlers::search_questions))
        .route(
            "/questions/imports/validate",
            post(handlers::validate_question_import),
        )
        .route(
            "/questions/imports/commit",
            post(handlers::commit_question_import),
        )
        .route("/exports/run", post(handlers::run_export))
        .route("/quality-checks/run", post(handlers::run_quality_check))
        .with_state(state)
}
