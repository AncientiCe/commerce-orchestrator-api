//! Mapping layer: orchestrator-native types to UCP-like request/response DTOs.
//! No hard dependency on UCP wire format; enables future REST/A2A adapters.

use orchestrator_core::{
    CapabilityManifest, CartId, UcpCapabilityBinding, UcpServiceBinding, UCP_LATEST_VERSION,
    UCP_SUPPORTED_VERSIONS,
};
use serde::{Deserialize, Serialize};
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
            services: Some(BTreeMap::from([
                (
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
                        UcpServiceBinding {
                            version: selected_version.to_string(),
                            spec: "https://agenticcommerce.dev/specs/checkout".to_string(),
                            transport: "acp".to_string(),
                            schema: Some(
                                "https://raw.githubusercontent.com/agentic-commerce-protocol/agentic-commerce-protocol/main/spec/2026-04-17/openapi/openapi.agentic_checkout.yaml".to_string(),
                            ),
                            endpoint: Some(format!("{}/api/v1/acp", base)),
                        },
                        UcpServiceBinding {
                            version: selected_version.to_string(),
                            spec: spec_url(selected_version, "embedded-link-delegation"),
                            transport: "embedded".to_string(),
                            schema: Some(format!(
                                "https://ucp.dev/{}/services/shopping/embedded.openapi.json",
                                selected_version
                            )),
                            endpoint: Some(format!("{}/api/v1/ucp/checkout", base)),
                        },
                    ],
                ),
            ])),
            capabilities: Some(core_capabilities(selected_version)),
            manifest: None,
            rest_endpoint: None,
            mcp_endpoint: Some(format!("{}/api/v1/mcp/message", base)),
            capability_flags: Some(BTreeMap::from([
                ("dev.ucp.payments.mpp".to_string(), true),
                ("dev.ucp.shopping.payment_handlers".to_string(), true),
                ("dev.ucp.security.signatures".to_string(), true),
                ("dev.ucp.shopping.checkout.embedded".to_string(), true),
            ])),
            signing_keys: Some(default_signing_keys()),
            payment_handlers: Some(default_payment_handlers()),
            mcp_supported_versions: Some(vec![
                "2026-07-28".to_string(),
                "2025-11-25".to_string(),
                "2024-11-05".to_string(),
            ]),
            a2a_profile_version: Some("1.0".to_string()),
            ap2_protocol_version: Some("0.2".to_string()),
            acp_api_version: Some("2026-04-17".to_string()),
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
                ("dev.ucp.payments.mpp".to_string(), true),
            ])),
            signing_keys: None,
            payment_handlers: None,
            mcp_supported_versions: None,
            a2a_profile_version: None,
            ap2_protocol_version: None,
            acp_api_version: None,
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
        (
            "dev.ucp.shopping.payment_handlers".to_string(),
            vec![capability(
                version,
                "payment-handlers",
                "shopping/payment_handlers.json",
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

fn default_signing_keys() -> Vec<SigningKeyDescriptor> {
    vec![SigningKeyDescriptor {
        kid: "orch-default-1".to_string(),
        kty: "OKP".to_string(),
        crv: Some("Ed25519".to_string()),
        alg: "EdDSA".to_string(),
        // Placeholder public key material for discovery advertisement; replace via ops config.
        x: Some("11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo".to_string()),
        use_: Some("sig".to_string()),
    }]
}

pub fn default_payment_handlers() -> Vec<PaymentHandlerDescriptor> {
    vec![
        PaymentHandlerDescriptor {
            id: "card".to_string(),
            name: "Card".to_string(),
            handler_type: "card".to_string(),
            instruments: vec!["card".to_string()],
        },
        PaymentHandlerDescriptor {
            id: "mpp".to_string(),
            name: "Machine Payments".to_string(),
            handler_type: "mpp".to_string(),
            instruments: vec!["mpp".to_string()],
        },
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WellKnownUcp {
    pub ucp: UcpSection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signing_keys: Option<Vec<SigningKeyDescriptor>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payment_handlers: Option<Vec<PaymentHandlerDescriptor>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp_supported_versions: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub a2a_profile_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ap2_protocol_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acp_api_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SigningKeyDescriptor {
    pub kid: String,
    pub kty: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crv: Option<String>,
    pub alg: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<String>,
    #[serde(rename = "use", skip_serializing_if = "Option::is_none")]
    pub use_: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentHandlerDescriptor {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub handler_type: String,
    pub instruments: Vec<String>,
}

/// Cart/checkout session identifier for UCP-style APIs.
pub fn cart_id_to_session_id(cart_id: CartId) -> String {
    format!("chk_{}", cart_id.0)
}

/// Verify a simple HMAC-style request signature header against discovery key kid.
/// Accepts `UCP-Signature: kid=<kid>;sig=<base64url>` when `UCP_SIGNING_SECRET` is set.
pub fn verify_ucp_request_signature(
    signature_header: Option<&str>,
    body: &[u8],
) -> Result<(), String> {
    let secret = match std::env::var("UCP_SIGNING_SECRET") {
        Ok(s) if !s.is_empty() => s,
        _ => return Ok(()), // signing optional unless secret configured
    };
    let header = signature_header.ok_or_else(|| "missing UCP-Signature header".to_string())?;
    let mut kid = None;
    let mut sig = None;
    for part in header.split(';') {
        let part = part.trim();
        if let Some(v) = part.strip_prefix("kid=") {
            kid = Some(v);
        } else if let Some(v) = part.strip_prefix("sig=") {
            sig = Some(v);
        }
    }
    let kid = kid.ok_or_else(|| "UCP-Signature missing kid".to_string())?;
    let sig = sig.ok_or_else(|| "UCP-Signature missing sig".to_string())?;
    if kid != "orch-default-1" {
        return Err(format!("unknown signing key kid '{kid}'"));
    }
    let expected = simple_hmac_hex(&secret, body);
    if expected != sig {
        return Err("UCP-Signature verification failed".to_string());
    }
    Ok(())
}

fn simple_hmac_hex(secret: &str, body: &[u8]) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    secret.hash(&mut hasher);
    body.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

/// Time-to-live for embedded checkout handoff links, in seconds (UCP 2026-04-08 "embedded"
/// transport / link delegation extension).
pub const EMBEDDED_CHECKOUT_LINK_TTL_SECONDS: i64 = 900;

/// A short-lived, signed handoff link a client can open to complete checkout on the merchant's
/// hosted embedded surface without leaving the embedding agent/client experience.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbeddedCheckoutLink {
    pub checkout_session_id: String,
    pub embedded_url: String,
    pub expires_at: i64,
}

/// Build an embedded checkout handoff link for the UCP "embedded" transport.
/// Signed with `UCP_SIGNING_SECRET` when configured, matching `verify_ucp_request_signature`.
pub fn build_embedded_checkout_link(base_url: &str, cart_id: CartId) -> EmbeddedCheckoutLink {
    let base = base_url.trim_end_matches('/');
    let session_id = cart_id_to_session_id(cart_id);
    let expires_at = now_unix_timestamp() + EMBEDDED_CHECKOUT_LINK_TTL_SECONDS;
    let token = embedded_link_token(&session_id, expires_at);
    EmbeddedCheckoutLink {
        embedded_url: format!(
            "{base}/api/v1/ucp/checkout/{session_id}/embedded?token={token}&expires={expires_at}"
        ),
        checkout_session_id: session_id,
        expires_at,
    }
}

fn embedded_link_token(session_id: &str, expires_at: i64) -> String {
    let secret = std::env::var("UCP_SIGNING_SECRET").unwrap_or_default();
    signed_token(session_id, expires_at, &secret)
}

fn signed_token(session_id: &str, expires_at: i64, secret: &str) -> String {
    let payload = format!("{session_id}:{expires_at}");
    simple_hmac_hex(secret, payload.as_bytes())
}

fn now_unix_timestamp() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_embedded_checkout_link_returns_url_with_session_id_and_future_expiry() {
        let before = now_unix_timestamp();
        let link = build_embedded_checkout_link(
            "https://orchestrator.example.com/",
            CartId(uuid::Uuid::nil()),
        );

        assert_eq!(
            link.checkout_session_id,
            cart_id_to_session_id(CartId(uuid::Uuid::nil()))
        );
        assert!(link
            .embedded_url
            .starts_with("https://orchestrator.example.com/api/v1/ucp/checkout/"));
        assert!(link.embedded_url.contains(&link.checkout_session_id));
        assert!(link.embedded_url.contains("token="));
        assert!(link
            .embedded_url
            .contains(&format!("expires={}", link.expires_at)));
        assert!(link.expires_at > before);
        assert_eq!(link.expires_at - before, EMBEDDED_CHECKOUT_LINK_TTL_SECONDS,);
    }

    #[test]
    fn signed_token_changes_with_secret_but_is_deterministic() {
        let unsigned_a = signed_token("chk_session", 1_000, "");
        let unsigned_b = signed_token("chk_session", 1_000, "");
        let signed = signed_token("chk_session", 1_000, "test-secret");

        assert_eq!(
            unsigned_a, unsigned_b,
            "same inputs must produce same token"
        );
        assert_ne!(unsigned_a, signed, "different secret must change the token");
    }
}
