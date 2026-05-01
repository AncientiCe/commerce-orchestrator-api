//! Mapping layer: orchestrator-native types to UCP-like request/response DTOs.
//! No hard dependency on UCP wire format; enables future REST/A2A adapters.

use orchestrator_core::{
    CapabilityManifest, CartId, UcpCapabilityBinding, UcpServiceBinding, UCP_LATEST_VERSION,
    UCP_SUPPORTED_VERSIONS,
};
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
    let base = base_url.trim_end_matches('/');
    if selected_version != UCP_LATEST_VERSION {
        return legacy_well_known_manifest(base, selected_version);
    }

    WellKnownUcp {
        ucp: UcpSection {
            version: selected_version.to_string(),
            supported_versions: serde_json::json!({
                "2026-01-23": format!("{}/.well-known/ucp?ucp_version=2026-01-23", base),
                "2026-01-11": format!("{}/.well-known/ucp?ucp_version=2026-01-11", base),
            }),
            services: Some(BTreeMap::from([(
                "dev.ucp.shopping".to_string(),
                vec![
                    UcpServiceBinding {
                        version: selected_version.to_string(),
                        spec: spec_url(selected_version, "overview"),
                        transport: "rest".to_string(),
                        schema: Some(format!(
                            "https://ucp.dev/{}/services/shopping/rest.openapi.json",
                            selected_version
                        )),
                        endpoint: Some(format!("{}/api/v1/ucp", base)),
                    },
                    UcpServiceBinding {
                        version: selected_version.to_string(),
                        spec: spec_url(selected_version, "overview"),
                        transport: "mcp".to_string(),
                        schema: Some(format!(
                            "https://ucp.dev/{}/services/shopping/mcp.openrpc.json",
                            selected_version
                        )),
                        endpoint: Some(format!("{}/api/v1/mcp/message", base)),
                    },
                ],
            )])),
            capabilities: Some(core_capabilities(selected_version)),
            manifest: None,
            rest_endpoint: None,
            mcp_endpoint: None,
            capability_flags: None,
        },
    }
}

fn legacy_well_known_manifest(base: &str, selected_version: &str) -> WellKnownUcp {
    WellKnownUcp {
        ucp: UcpSection {
            version: selected_version.to_string(),
            supported_versions: serde_json::Value::Array(
                UCP_SUPPORTED_VERSIONS
                    .iter()
                    .filter(|version| **version != UCP_LATEST_VERSION)
                    .map(|version| serde_json::Value::String((*version).to_string()))
                    .collect(),
            ),
            services: None,
            capabilities: None,
            manifest: Some(CapabilityManifest::for_version(selected_version)),
            rest_endpoint: Some(format!("{}/", base)),
            mcp_endpoint: Some(format!("{}/api/v1/mcp/message", base)),
            capability_flags: Some(BTreeMap::from([
                ("dev.ucp.shopping.cart.multi_item".to_string(), false),
                ("dev.ucp.shopping.catalog.lookup".to_string(), true),
                ("dev.ucp.identity.linking".to_string(), true),
                ("dev.ucp.payments.mpp".to_string(), false),
            ])),
        },
    }
}

fn core_capabilities(version: &str) -> BTreeMap<String, Vec<UcpCapabilityBinding>> {
    BTreeMap::from([
        (
            "dev.ucp.shopping.checkout".to_string(),
            vec![capability(
                version,
                "checkout",
                "shopping/checkout.json",
                None,
            )],
        ),
        (
            "dev.ucp.shopping.cart".to_string(),
            vec![capability(version, "cart", "shopping/cart.json", None)],
        ),
        (
            "dev.ucp.shopping.catalog.lookup".to_string(),
            vec![capability(
                version,
                "catalog/lookup",
                "shopping/catalog_lookup.json",
                None,
            )],
        ),
        (
            "dev.ucp.shopping.catalog.search".to_string(),
            vec![capability(
                version,
                "catalog/search",
                "shopping/catalog_search.json",
                None,
            )],
        ),
        (
            "dev.ucp.shopping.order".to_string(),
            vec![capability(version, "order", "shopping/order.json", None)],
        ),
        (
            "dev.ucp.shopping.discount".to_string(),
            vec![capability(
                version,
                "checkout/discounts",
                "shopping/discount.json",
                Some(vec![
                    "dev.ucp.shopping.checkout".to_string(),
                    "dev.ucp.shopping.cart".to_string(),
                ]),
            )],
        ),
        (
            "dev.ucp.common.identity_linking".to_string(),
            vec![capability(
                version,
                "identity-linking",
                "common/identity_linking.json",
                None,
            )],
        ),
    ])
}

fn capability(
    version: &str,
    spec_slug: &str,
    schema_path: &str,
    extends: Option<Vec<String>>,
) -> UcpCapabilityBinding {
    UcpCapabilityBinding {
        version: version.to_string(),
        spec: spec_url(version, spec_slug),
        schema: format!("https://ucp.dev/{}/schemas/{}", version, schema_path),
        extends,
    }
}

fn spec_url(version: &str, slug: &str) -> String {
    format!("https://ucp.dev/{}/specification/{}", version, slug)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WellKnownUcp {
    pub ucp: UcpSection,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UcpSection {
    pub version: String,
    pub supported_versions: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub services: Option<BTreeMap<String, Vec<UcpServiceBinding>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<BTreeMap<String, Vec<UcpCapabilityBinding>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manifest: Option<CapabilityManifest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rest_endpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp_endpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capability_flags: Option<BTreeMap<String, bool>>,
}

/// Cart/checkout session identifier for UCP-style APIs.
pub fn cart_id_to_session_id(cart_id: CartId) -> String {
    format!("chk_{}", cart_id.0)
}
