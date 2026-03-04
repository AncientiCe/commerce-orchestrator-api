//! Mapping layer: orchestrator-native types to UCP-like request/response DTOs.
//! No hard dependency on UCP wire format; enables future REST/A2A adapters.

use orchestrator_core::{CapabilityManifest, CartId};

/// Build discovery manifest for agents (/.well-known/ucp equivalent).
pub fn build_well_known_manifest(base_url: &str) -> WellKnownUcp {
    WellKnownUcp {
        ucp: UcpSection {
            version: "2026-01-11".to_string(),
            manifest: CapabilityManifest::default(),
            rest_endpoint: Some(format!("{}/", base_url.trim_end_matches('/'))),
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
    pub manifest: CapabilityManifest,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rest_endpoint: Option<String>,
}

/// Cart/checkout session identifier for UCP-style APIs.
pub fn cart_id_to_session_id(cart_id: CartId) -> String {
    format!("chk_{}", cart_id.0)
}
