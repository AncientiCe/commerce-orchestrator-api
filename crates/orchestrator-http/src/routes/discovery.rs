//! UCP-style discovery endpoint: GET /.well-known/ucp returns capability manifest and REST base URL.

use axum::{extract::State, response::IntoResponse, Json};

use crate::state::AppState;
use orchestrator_api::build_well_known_manifest;

/// GET /.well-known/ucp — returns JSON manifest with version, services, capabilities, and rest_endpoint.
/// No auth required; used by agents and clients for capability discovery.
pub async fn well_known_ucp(State(state): State<AppState>) -> impl IntoResponse {
    let manifest = build_well_known_manifest(&state.discovery_base_url);
    Json(manifest)
}

pub fn routes() -> axum::Router<AppState> {
    axum::Router::new().route("/.well-known/ucp", axum::routing::get(well_known_ucp))
}
