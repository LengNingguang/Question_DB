pub(crate) mod handlers;
pub(crate) mod imports;
pub(crate) mod models;
pub(crate) mod queries;

use axum::{routing::get, Router};

pub(crate) use imports::MAX_UPLOAD_BYTES;

pub(crate) fn router() -> Router<super::AppState> {
    Router::new()
        .route(
            "/questions",
            get(handlers::list_questions).post(handlers::create_question),
        )
        .route(
            "/questions/:question_id",
            get(handlers::get_question_detail)
                .patch(handlers::update_question_metadata)
                .delete(handlers::delete_question),
        )
        .route(
            "/questions/:question_id/file",
            axum::routing::put(handlers::replace_question_file),
        )
}
