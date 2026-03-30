//! Mapping layer: orchestrator-native types to UCP-like request/response DTOs.
//! No hard dependency on UCP wire format; enables future REST/A2A adapters.

use orchestrator_core::{CapabilityManifest, CartId, UCP_LATEST_VERSION, UCP_SUPPORTED_VERSIONS};
use std::collections::BTreeMap;

/// Build discovery manifest for agents (/.well-known/ucp equivalent).
pub fn build_well_known_manifest(base_url: &str) -> WellKnownUcp {
    build_well_known_manifest_with_version(base_url, None)
}

pub fn build_well_known_manifest_with_version(
    base_url: &str,
    requested_version: Option<&str>,
) -> WellKnownUcp {
    let selected_version = requested_version
        .filter(|v| UCP_SUPPORTED_VERSIONS.contains(v))
        .unwrap_or(UCP_LATEST_VERSION);
    WellKnownUcp {
        ucp: UcpSection {
            version: selected_version.to_string(),
            supported_versions: UCP_SUPPORTED_VERSIONS
                .iter()
                .map(|v| (*v).to_string())
                .collect(),
            manifest: CapabilityManifest::for_version(selected_version),
            rest_endpoint: Some(format!("{}/", base_url.trim_end_matches('/'))),
            capability_flags: BTreeMap::from([
                ("dev.ucp.shopping.cart.multi_item".to_string(), false),
                ("dev.ucp.shopping.catalog.lookup".to_string(), false),
                ("dev.ucp.identity.linking".to_string(), true),
            ]),
        },
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WellKnownUcp {
    pub ucp: UcpSection,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UcpSection {
    pub version: String,
    pub supported_versions: Vec<String>,
    pub manifest: CapabilityManifest,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rest_endpoint: Option<String>,
    pub capability_flags: BTreeMap<String, bool>,
}

/// Cart/checkout session identifier for UCP-style APIs.
pub fn cart_id_to_session_id(cart_id: CartId) -> String {
    format!("chk_{}", cart_id.0)
}
