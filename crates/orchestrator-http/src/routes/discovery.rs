//! UCP-style discovery endpoint: GET /.well-known/ucp returns capability manifest and REST base URL.

use axum::{
    extract::{Query, State},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;

use crate::state::AppState;
use orchestrator_api::build_well_known_manifest_with_version;

/// GET /.well-known/ucp — returns JSON manifest with version, services, capabilities, and rest_endpoint.
/// No auth required; used by agents and clients for capability discovery.
pub async fn well_known_ucp(
    State(state): State<AppState>,
    Query(params): Query<DiscoveryQuery>,
) -> impl IntoResponse {
    let manifest = build_well_known_manifest_with_version(
        &state.discovery_base_url,
        params.ucp_version.as_deref(),
    );
    let selected = manifest.ucp.version.replace('-', "_");
    orchestrator_observability::incr(&format!(
        "ucp_discovery_version_selected_{}_total",
        selected
    ));
    if params
        .ucp_version
        .as_deref()
        .is_some_and(|v| v != manifest.ucp.version)
    {
        orchestrator_observability::incr("ucp_discovery_version_fallback_total");
    }
    Json(manifest)
}

pub fn routes() -> axum::Router<AppState> {
    axum::Router::new().route("/.well-known/ucp", axum::routing::get(well_known_ucp))
}

#[derive(Debug, Deserialize)]
pub struct DiscoveryQuery {
    pub ucp_version: Option<String>,
}
