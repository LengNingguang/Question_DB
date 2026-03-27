pub(crate) mod handlers;
pub(crate) mod imports;
pub(crate) mod models;
pub(crate) mod queries;

use axum::{routing::get, Router};

pub(crate) use imports::MAX_UPLOAD_BYTES;

pub(crate) fn router() -> Router<super::AppState> {
    Router::new()
        .route(
            "/papers",
            get(handlers::list_papers).post(handlers::create_paper),
        )
        .route(
            "/papers/:paper_id",
            get(handlers::get_paper_detail)
                .patch(handlers::update_paper)
                .delete(handlers::delete_paper),
        )
        .route(
            "/papers/:paper_id/file",
            axum::routing::put(handlers::replace_paper_file),
        )
}
