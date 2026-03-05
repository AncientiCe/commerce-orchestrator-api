//! Route modules for the API.

mod health;
mod v1;

use axum::middleware;
use axum::Router;
use tower_http::trace::TraceLayer;

use crate::observability::request_id_middleware;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(health::routes())
        .nest("/api/v1", v1::routes())
        .layer(middleware::from_fn(request_id_middleware))
        .layer(TraceLayer::new_for_http())
}
