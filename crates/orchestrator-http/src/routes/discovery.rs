//! UCP-style discovery endpoint: GET /.well-known/ucp returns capability manifest and REST base URL.

use axum::{
    extract::{Query, State},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;

use crate::state::AppState;
use orchestrator_api::{build_acp_discovery_document, build_well_known_manifest_with_version};

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

/// GET /.well-known/acp.json — ACP discovery document (protocol version, REST base URL,
/// transports, and supported services/extensions). No auth required.
pub async fn well_known_acp(State(state): State<AppState>) -> impl IntoResponse {
    let document = build_acp_discovery_document(&state.discovery_base_url);
    orchestrator_observability::incr("acp_discovery_document_total");
    Json(document)
}

pub fn routes() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/.well-known/ucp", axum::routing::get(well_known_ucp))
        .route("/.well-known/acp.json", axum::routing::get(well_known_acp))
}

#[derive(Debug, Deserialize)]
pub struct DiscoveryQuery {
    pub ucp_version: Option<String>,
}
