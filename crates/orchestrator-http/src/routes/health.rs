//! Health check endpoints for liveness and readiness.

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::Serialize;

use crate::state::AppState;

#[derive(Serialize)]
pub struct LiveResponse {
    pub status: &'static str,
}

#[derive(Serialize)]
pub struct ReadyResponse {
    pub status: &'static str,
}

#[derive(Serialize)]
pub struct MetricsResponse {
    pub http_requests_total: u64,
}

/// GET /metrics - simple metrics for RED (request count; extend with error/latency in future).
pub async fn metrics() -> impl IntoResponse {
    use crate::observability::get_request_count;
    (
        axum::http::StatusCode::OK,
        axum::Json(MetricsResponse {
            http_requests_total: get_request_count(),
        }),
    )
}

/// GET /health/live - liveness probe (process is running).
pub async fn live() -> impl IntoResponse {
    (StatusCode::OK, Json(LiveResponse { status: "ok" }))
}

/// GET /health/ready - readiness probe (ready to accept traffic). Returns 503 when shutting down.
pub async fn ready(State(state): State<AppState>) -> impl IntoResponse {
    if state.is_shutting_down() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ReadyResponse {
                status: "shutting_down",
            }),
        );
    }
    (StatusCode::OK, Json(ReadyResponse { status: "ok" }))
}

pub fn routes() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/health/live", axum::routing::get(live))
        .route("/health/ready", axum::routing::get(ready))
        .route("/metrics", axum::routing::get(metrics))
}
