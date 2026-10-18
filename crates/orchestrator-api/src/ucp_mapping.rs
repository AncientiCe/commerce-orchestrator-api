//! Mapping layer: orchestrator-native types to UCP-like request/response DTOs.
//! No hard dependency on UCP wire format; enables future REST/A2A adapters.

use orchestrator_core::{
    CapabilityManifest, CartId, UcpCapabilityBinding, UcpServiceBinding, UCP_LATEST_VERSION,
    UCP_SUPPORTED_VERSIONS,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::acp_mapping::ACP_API_VERSION;
use crate::signing::SigningKeyring;

/// UCP version whose profile shape this orchestrator still serves to platforms
/// that negotiated it before `2026-08-25`.
pub const UCP_PREVIOUS_VERSION: &str = "2026-04-08";

/// MCP protocol versions the orchestrator's own MCP server negotiates.
pub const MCP_SUPPORTED_VERSIONS: &[&str] = &["2026-07-28", "2025-11-25", "2024-11-05"];

/// What this particular deployment can actually do, as opposed to what the
/// protocol allows. Discovery is built from this so an agent is never told about
/// a capability that is not wired up.
#[derive(Debug, Clone, Default)]
pub struct AdvertisedCapabilities {
    /// Public JWKs from the configured keyring. Empty means this deployment does
    /// not sign, and the manifest says so rather than advertising a key that
    /// cannot verify anything.
    pub signing_keys: Vec<SigningKeyDescriptor>,
    /// True when an identity provider is configured behind identity linking.
    pub identity_linking: bool,
    /// True when a merchant-hosted embedded checkout page and signing material are
    /// both configured, so an embedded handoff link is reachable and verifiable.
    pub embedded_checkout: bool,
    /// UCP payment handler registry, keyed by reverse-DNS handler name. Handlers
    /// belong to the operator's payment providers, so they are configured, not
    /// invented; an empty registry is published as `{}`.
    pub payment_handlers: serde_json::Map<String, serde_json::Value>,
}

impl AdvertisedCapabilities {
    /// Convenience for callers that only have signing material to declare.
    pub fn with_signing_keys(signing_keys: Vec<SigningKeyDescriptor>) -> Self {
        Self {
            signing_keys,
            ..Self::default()
        }
    }
}

/// Build discovery manifest for agents (/.well-known/ucp equivalent).
pub fn build_well_known_manifest(
    base_url: &str,
    advertised: &AdvertisedCapabilities,
) -> WellKnownUcp {
    build_well_known_manifest_with_version(base_url, None, advertised)
}

pub fn build_well_known_manifest_with_version(
    base_url: &str,
    requested_version: Option<&str>,
    advertised: &AdvertisedCapabilities,
) -> WellKnownUcp {
    let selected_version = requested_version
        .filter(|v| UCP_SUPPORTED_VERSIONS.contains(v))
        .unwrap_or(UCP_LATEST_VERSION);
    let base = base_url.trim_end_matches('/');
    match selected_version {
        UCP_LATEST_VERSION => latest_profile(base, advertised),
        UCP_PREVIOUS_VERSION => profile_2026_04_08(base, advertised),
        legacy => legacy_well_known_manifest(base, legacy),
    }
}

/// The current (`2026-08-25`) business profile. Everything inside `ucp` is UCP
/// core vocabulary; the orchestrator's own discovery hints live in the
/// top-level `orchestrator` member, because only UCP core may define `ucp`
/// members.
fn latest_profile(base: &str, advertised: &AdvertisedCapabilities) -> WellKnownUcp {
    let version = UCP_LATEST_VERSION;
    let signing_keys = &advertised.signing_keys;
    let supported_versions: serde_json::Map<String, serde_json::Value> = UCP_SUPPORTED_VERSIONS
        .iter()
        .filter(|v| **v != version)
        .map(|v| {
            (
                (*v).to_string(),
                serde_json::Value::String(format!("{base}/.well-known/ucp?ucp_version={v}")),
            )
        })
        .collect();
    let mut shopping = vec![UcpServiceBinding {
        version: version.to_string(),
        spec: spec_url(version, "overview"),
        transport: "rest".to_string(),
        schema: Some(format!(
            "https://ucp.dev/{version}/services/shopping/rest.openapi.json"
        )),
        endpoint: Some(format!("{base}/api/v1/ucp")),
    }];
    if advertised.embedded_checkout {
        shopping.push(UcpServiceBinding {
            version: version.to_string(),
            spec: spec_url(version, "shopping/checkout/embedded"),
            transport: "embedded".to_string(),
            schema: Some(format!(
                "https://ucp.dev/{version}/services/shopping/embedded.openrpc.json"
            )),
            endpoint: Some(format!("{base}/api/v1/ucp/checkout-sessions")),
        });
    }
    WellKnownUcp {
        ucp: UcpSection {
            version: version.to_string(),
            supported_versions: Some(serde_json::Value::Object(supported_versions)),
            services: Some(BTreeMap::from([("dev.ucp.shopping".to_string(), shopping)])),
            capabilities: Some(core_capabilities(version, advertised, &LATEST_SLUGS)),
            payment_handlers: Some(serde_json::Value::Object(
                advertised.payment_handlers.clone(),
            )),
            ..UcpSection::new(version)
        },
        keys: (!signing_keys.is_empty()).then(|| signing_keys.clone()),
        orchestrator: Some(OrchestratorExtensions {
            mcp_endpoint: format!("{base}/api/v1/mcp/message"),
            mcp_supported_versions: MCP_SUPPORTED_VERSIONS
                .iter()
                .map(|v| (*v).to_string())
                .collect(),
            a2a_profile_version: "1.0".to_string(),
            ap2_protocol_version: "0.2".to_string(),
            acp_api_version: ACP_API_VERSION.to_string(),
            acp_discovery: format!("{base}/.well-known/acp.json"),
            capability_flags: capability_flags(advertised),
        }),
    }
}

fn capability_flags(advertised: &AdvertisedCapabilities) -> BTreeMap<String, bool> {
    BTreeMap::from([
        ("dev.ucp.payments.mpp".to_string(), true),
        (
            "dev.ucp.security.signatures".to_string(),
            !advertised.signing_keys.is_empty(),
        ),
        (
            "dev.ucp.shopping.checkout.embedded".to_string(),
            advertised.embedded_checkout,
        ),
        ("dev.ucp.shopping.fulfillment".to_string(), true),
        (
            "dev.ucp.common.identity_linking".to_string(),
            advertised.identity_linking,
        ),
    ])
}

/// The `2026-04-08` profile, unchanged in shape for platforms still on it,
/// except that a linked profile carries no `supported_versions` of its own.
fn profile_2026_04_08(base: &str, advertised: &AdvertisedCapabilities) -> WellKnownUcp {
    let version = UCP_PREVIOUS_VERSION;
    let signing_keys = advertised.signing_keys.as_slice();
    let mut shopping = vec![
        UcpServiceBinding {
            version: version.to_string(),
            spec: spec_url(version, "overview"),
            transport: "rest".to_string(),
            schema: Some(format!(
                "https://ucp.dev/{version}/services/shopping/rest.openapi.json"
            )),
            endpoint: Some(format!("{base}/api/v1/ucp")),
        },
        UcpServiceBinding {
            version: version.to_string(),
            spec: spec_url(version, "overview"),
            transport: "mcp".to_string(),
            schema: Some(format!(
                "https://ucp.dev/{version}/services/shopping/mcp.openrpc.json"
            )),
            endpoint: Some(format!("{base}/api/v1/mcp/message")),
        },
        UcpServiceBinding {
            version: version.to_string(),
            spec: "https://agenticcommerce.dev/specs/checkout".to_string(),
            transport: "acp".to_string(),
            schema: Some(format!(
                "https://raw.githubusercontent.com/agentic-commerce-protocol/agentic-commerce-protocol/main/spec/{ACP_API_VERSION}/openapi/openapi.agentic_checkout.yaml"
            )),
            endpoint: Some(format!("{base}/api/v1/acp")),
        },
    ];
    if advertised.embedded_checkout {
        shopping.push(UcpServiceBinding {
            version: version.to_string(),
            spec: spec_url(version, "embedded-link-delegation"),
            transport: "embedded".to_string(),
            schema: Some(format!(
                "https://ucp.dev/{version}/services/shopping/embedded.openapi.json"
            )),
            endpoint: Some(format!("{base}/api/v1/ucp/checkout")),
        });
    }
    let mut capabilities = core_capabilities(version, advertised, &PREVIOUS_SLUGS);
    capabilities.insert(
        "dev.ucp.shopping.payment_handlers".to_string(),
        vec![capability(
            version,
            "payment-handlers",
            "shopping/payment_handlers.json",
            None,
        )],
    );
    let mut flags = capability_flags(advertised);
    flags.insert("dev.ucp.shopping.payment_handlers".to_string(), true);
    WellKnownUcp {
        ucp: UcpSection {
            services: Some(BTreeMap::from([("dev.ucp.shopping".to_string(), shopping)])),
            capabilities: Some(capabilities),
            mcp_endpoint: Some(format!("{base}/api/v1/mcp/message")),
            capability_flags: Some(flags),
            signing_keys: (!signing_keys.is_empty()).then(|| signing_keys.to_vec()),
            payment_handlers: Some(
                serde_json::to_value(default_payment_handlers()).unwrap_or_default(),
            ),
            mcp_supported_versions: Some(
                MCP_SUPPORTED_VERSIONS
                    .iter()
                    .map(|v| (*v).to_string())
                    .collect(),
            ),
            a2a_profile_version: Some("1.0".to_string()),
            ap2_protocol_version: Some("0.2".to_string()),
            acp_api_version: Some(ACP_API_VERSION.to_string()),
            ..UcpSection::new(version)
        },
        // 2026-04-08 named signing_keys canonical and keys[] its JWK Set mirror.
        keys: (!signing_keys.is_empty()).then(|| signing_keys.to_vec()),
        orchestrator: None,
    }
}

fn legacy_well_known_manifest(base: &str, selected_version: &str) -> WellKnownUcp {
    WellKnownUcp {
        ucp: UcpSection {
            manifest: Some(CapabilityManifest::for_version(selected_version)),
            rest_endpoint: Some(format!("{}/", base)),
            mcp_endpoint: Some(format!("{}/api/v1/mcp/message", base)),
            capability_flags: Some(BTreeMap::from([
                ("dev.ucp.shopping.cart.multi_item".to_string(), false),
                ("dev.ucp.shopping.catalog.lookup".to_string(), true),
                ("dev.ucp.identity.linking".to_string(), true),
                ("dev.ucp.payments.mpp".to_string(), true),
            ])),
            ..UcpSection::new(selected_version)
        },
        keys: None,
        orchestrator: None,
    }
}

/// Specification page slugs per capability; the spec site was reorganised into
/// `shopping/`, `payment/` and `common/` sections in `2026-08-25`.
struct SpecSlugs {
    checkout: &'static str,
    cart: &'static str,
    catalog_lookup: &'static str,
    catalog_search: &'static str,
    order: &'static str,
    discount: &'static str,
    fulfillment: &'static str,
    identity_linking: &'static str,
}

const LATEST_SLUGS: SpecSlugs = SpecSlugs {
    checkout: "shopping/checkout",
    cart: "shopping/cart",
    catalog_lookup: "shopping/catalog/lookup",
    catalog_search: "shopping/catalog/search",
    order: "shopping/order",
    discount: "shopping/extensions/discount",
    fulfillment: "shopping/extensions/fulfillment",
    identity_linking: "common/identity-linking",
};

const PREVIOUS_SLUGS: SpecSlugs = SpecSlugs {
    checkout: "checkout",
    cart: "cart",
    catalog_lookup: "catalog/lookup",
    catalog_search: "catalog/search",
    order: "order",
    discount: "checkout/discounts",
    fulfillment: "fulfillment",
    identity_linking: "identity-linking",
};

fn core_capabilities(
    version: &str,
    advertised: &AdvertisedCapabilities,
    slugs: &SpecSlugs,
) -> BTreeMap<String, Vec<UcpCapabilityBinding>> {
    let checkout_and_cart = || {
        Some(vec![
            "dev.ucp.shopping.checkout".to_string(),
            "dev.ucp.shopping.cart".to_string(),
        ])
    };
    let mut capabilities = BTreeMap::from([
        (
            "dev.ucp.shopping.checkout".to_string(),
            vec![capability(
                version,
                slugs.checkout,
                "shopping/checkout.json",
                None,
            )],
        ),
        (
            "dev.ucp.shopping.cart".to_string(),
            vec![capability(version, slugs.cart, "shopping/cart.json", None)],
        ),
        (
            "dev.ucp.shopping.catalog.lookup".to_string(),
            vec![capability(
                version,
                slugs.catalog_lookup,
                "shopping/catalog_lookup.json",
                None,
            )],
        ),
        (
            "dev.ucp.shopping.catalog.search".to_string(),
            vec![capability(
                version,
                slugs.catalog_search,
                "shopping/catalog_search.json",
                None,
            )],
        ),
        (
            "dev.ucp.shopping.order".to_string(),
            vec![capability(
                version,
                slugs.order,
                "shopping/order.json",
                None,
            )],
        ),
        (
            "dev.ucp.shopping.discount".to_string(),
            vec![capability(
                version,
                slugs.discount,
                "shopping/discount.json",
                checkout_and_cart(),
            )],
        ),
        (
            "dev.ucp.shopping.fulfillment".to_string(),
            vec![capability(
                version,
                slugs.fulfillment,
                "shopping/fulfillment.json",
                checkout_and_cart(),
            )],
        ),
    ]);
    if advertised.identity_linking {
        capabilities.insert(
            "dev.ucp.common.identity_linking".to_string(),
            vec![capability(
                version,
                slugs.identity_linking,
                "common/identity_linking.json",
                None,
            )],
        );
    }
    capabilities
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

/// A UCP profile document: the `ucp` envelope, the JWK Set beside it, and the
/// orchestrator's own discovery hints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WellKnownUcp {
    pub ucp: UcpSection,
    /// Signing keys as an RFC 7517 JWK Set, the canonical location since `2026-08-25`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keys: Option<Vec<SigningKeyDescriptor>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orchestrator: Option<OrchestratorExtensions>,
}

/// Discovery hints outside UCP core: where the orchestrator's MCP server and ACP
/// discovery live, which versions of the neighbouring protocols it speaks, and
/// feature flags.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorExtensions {
    pub mcp_endpoint: String,
    pub mcp_supported_versions: Vec<String>,
    pub a2a_profile_version: String,
    pub ap2_protocol_version: String,
    pub acp_api_version: String,
    pub acp_discovery: String,
    pub capability_flags: BTreeMap<String, bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UcpSection {
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supported_versions: Option<serde_json::Value>,
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
    /// A reverse-DNS keyed registry in `2026-08-25`; a handler list before it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payment_handlers: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp_supported_versions: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub a2a_profile_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ap2_protocol_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acp_api_version: Option<String>,
}

impl UcpSection {
    fn new(version: &str) -> Self {
        Self {
            version: version.to_string(),
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SigningKeyDescriptor {
    pub kid: String,
    pub kty: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crv: Option<String>,
    /// JOSE algorithm. Optional on JWKs we read; always set on the ones we publish.
    #[serde(default)]
    pub alg: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<String>,
    /// Second coordinate of an EC (P-256) public key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<String>,
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

/// Time-to-live for embedded checkout handoff links, in seconds (UCP "embedded"
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

/// The exact bytes the handoff token signs. The hosted checkout page rebuilds this
/// from the `session` and `expires` query parameters and verifies `token` with the
/// JWK named by `kid` in `/.well-known/ucp`.
pub fn embedded_link_signing_base(session_id: &str, expires_at: i64) -> String {
    format!("ucp-embedded-checkout:{session_id}:{expires_at}")
}

/// Build an embedded checkout handoff link pointing at the merchant's hosted page.
///
/// The token is an Ed25519 signature by the active discovery key, so the hosted
/// page needs no shared secret: anyone holding the public JWK can verify it, and
/// nobody without the private key can mint one.
pub fn build_embedded_checkout_link(
    hosted_checkout_url: &str,
    cart_id: CartId,
    keyring: &SigningKeyring,
) -> Result<EmbeddedCheckoutLink, url::ParseError> {
    let session_id = cart_id_to_session_id(cart_id);
    let expires_at = now_unix_timestamp() + EMBEDDED_CHECKOUT_LINK_TTL_SECONDS;
    let signature = keyring.sign(embedded_link_signing_base(&session_id, expires_at).as_bytes());
    let mut url = url::Url::parse(hosted_checkout_url)?;
    url.query_pairs_mut()
        .append_pair("session", &session_id)
        .append_pair("expires", &expires_at.to_string())
        .append_pair("kid", &signature.kid)
        .append_pair("token", &signature.signature);
    Ok(EmbeddedCheckoutLink {
        embedded_url: url.into(),
        checkout_session_id: session_id,
        expires_at,
    })
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

    const SEED: &str = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8";

    #[test]
    fn embedded_checkout_link_targets_hosted_page_and_expires_after_ttl() {
        let keyring = SigningKeyring::from_seeds("orch-a", SEED, &[]).unwrap();
        let before = now_unix_timestamp();
        let link = build_embedded_checkout_link(
            "https://shop.example.com/checkout/embedded",
            CartId(uuid::Uuid::nil()),
            &keyring,
        )
        .unwrap();

        assert_eq!(
            link.checkout_session_id,
            cart_id_to_session_id(CartId(uuid::Uuid::nil()))
        );
        assert!(link
            .embedded_url
            .starts_with("https://shop.example.com/checkout/embedded?session=chk_"));
        assert!(link.embedded_url.contains("kid=orch-a"));
        assert_eq!(link.expires_at - before, EMBEDDED_CHECKOUT_LINK_TTL_SECONDS);
    }

    #[test]
    fn embedded_checkout_token_cannot_be_moved_to_another_session() {
        let keyring = SigningKeyring::from_seeds("orch-a", SEED, &[]).unwrap();
        let link = build_embedded_checkout_link(
            "https://shop.example.com/checkout",
            CartId(uuid::Uuid::nil()),
            &keyring,
        )
        .unwrap();
        let token = url::Url::parse(&link.embedded_url)
            .unwrap()
            .query_pairs()
            .find(|(k, _)| k == "token")
            .map(|(_, v)| v.into_owned())
            .unwrap();

        keyring
            .verify(
                "orch-a",
                embedded_link_signing_base(&link.checkout_session_id, link.expires_at).as_bytes(),
                &token,
            )
            .expect("token verifies for its own session");
        assert!(keyring
            .verify(
                "orch-a",
                embedded_link_signing_base("chk_other", link.expires_at).as_bytes(),
                &token,
            )
            .is_err());
    }

    #[test]
    fn embedded_checkout_link_rejects_an_unparseable_hosted_url() {
        let keyring = SigningKeyring::from_seeds("orch-a", SEED, &[]).unwrap();
        assert!(
            build_embedded_checkout_link("not a url", CartId(uuid::Uuid::nil()), &keyring).is_err()
        );
    }
}
