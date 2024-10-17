use axum::{http::StatusCode, response::IntoResponse, routing, Router};
use tracing::debug;

pub(crate) fn init_router() -> Router {
    Router::new().route("/health", routing::get(get_health))
}

async fn get_health() -> impl IntoResponse {
    debug!("Entering GET health endpoint.");
    StatusCode::OK
}
