//! Route modules for the API.

mod discovery;
mod health;
mod v1;

use axum::middleware;
use axum::response::IntoResponse;
use axum::Router;
use tower_http::trace::TraceLayer;

use crate::observability::request_id_middleware;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(discovery::routes())
        .merge(health::routes())
        .route("/api/v1/openapi.json", axum::routing::get(openapi))
        .nest("/api/v1", v1::routes())
        .layer(middleware::from_fn(request_id_middleware))
        .layer(TraceLayer::new_for_http())
}

async fn openapi() -> impl IntoResponse {
    match crate::openapi::openapi_json() {
        Ok(body) => (
            axum::http::StatusCode::OK,
            [(
                axum::http::header::CONTENT_TYPE,
                "application/json; charset=utf-8",
            )],
            body,
        )
            .into_response(),
        Err(error) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            [(
                axum::http::header::CONTENT_TYPE,
                "application/json; charset=utf-8",
            )],
            format!("{{\"error\":\"{}\"}}", error.replace('"', "'")),
        )
            .into_response(),
    }
}
