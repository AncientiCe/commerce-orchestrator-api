//! ACP (Agentic Commerce Protocol) mapping: OpenAI/Stripe checkout session DTOs ↔ domain.

use orchestrator_core::contract::{
    AddItemPayload, CancelCartPayload, CartCommand, CartId, CartProjection, CheckoutRequest,
    CreateCartPayload, CustomerHint, GetCartPayload, PaymentIntent, PaymentMethodType,
    StartCheckoutPayload,
};
use provider_contracts::DelegatedPayment;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use uuid::Uuid;

pub const ACP_API_VERSION: &str = "2026-04-17";
pub const ACP_SUPPORTED_VERSIONS: &[&str] = &[ACP_API_VERSION];

pub fn negotiate_acp_version(requested: Option<&str>) -> Result<&'static str, AcpVersionError> {
    match requested.map(str::trim).filter(|v| !v.is_empty()) {
        None => Err(AcpVersionError::Missing),
        Some(v) if ACP_SUPPORTED_VERSIONS.contains(&v) => Ok(ACP_API_VERSION),
        Some(v) => Err(AcpVersionError::Unsupported(v.to_string())),
    }
}

#[derive(Debug, Clone)]
pub enum AcpVersionError {
    Missing,
    Unsupported(String),
}

impl AcpVersionError {
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            Self::Missing => serde_json::json!({
                "type": "invalid_request",
                "code": "missing_api_version",
                "message": "API-Version header is required",
                "supported_versions": ACP_SUPPORTED_VERSIONS,
            }),
            Self::Unsupported(v) => serde_json::json!({
                "type": "invalid_request",
                "code": "unsupported_api_version",
                "message": format!("API version '{v}' is not supported"),
                "supported_versions": ACP_SUPPORTED_VERSIONS,
            }),
        }
    }

    pub fn message(&self) -> String {
        self.to_json()
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("ACP version error")
            .to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpLineItem {
    pub id: Option<String>,
    pub item: AcpItemRef,
    pub quantity: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpItemRef {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpCheckoutSessionCreateRequest {
    pub merchant_id: String,
    pub currency: String,
    #[serde(default)]
    pub line_items: Vec<AcpLineItem>,
    #[serde(default)]
    pub buyer: Option<AcpBuyer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpBuyer {
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub full_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpCheckoutSessionUpdateRequest {
    #[serde(default)]
    pub line_items: Vec<AcpLineItem>,
    #[serde(default)]
    pub buyer: Option<AcpBuyer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpCompleteSessionRequest {
    pub tenant_id: String,
    pub merchant_id: String,
    pub idempotency_key: String,
    pub payment_data: AcpPaymentData,
    #[serde(default)]
    pub buyer: Option<AcpBuyer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpPaymentData {
    pub token: String,
    pub amount_minor: i64,
    #[serde(default)]
    pub payment_handler_id: Option<String>,
    #[serde(default)]
    pub ap2_consent_proof: Option<String>,
    #[serde(default)]
    pub payment_method_type: Option<String>,
    #[serde(default)]
    pub mpp_method: Option<String>,
    #[serde(default)]
    pub mpp_intent: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpDelegatePaymentRequest {
    pub tenant_id: String,
    pub payment_method: AcpDelegatedPaymentMethod,
    #[serde(default)]
    pub risk_signals: Vec<serde_json::Value>,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpDelegatedPaymentMethod {
    #[serde(default, rename = "type")]
    pub method_type: Option<String>,
    pub token: String,
    #[serde(default)]
    pub amount_minor: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpCheckoutSessionResponse {
    pub id: String,
    pub status: String,
    pub currency: String,
    pub line_items: Vec<AcpSessionLine>,
    pub totals: AcpTotals,
    pub api_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpSessionLine {
    pub id: String,
    pub item_id: String,
    pub quantity: u32,
    pub unit_amount: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpTotals {
    pub subtotal_minor: i64,
    pub tax_minor: i64,
    /// Total deducted by applied adjustment codes, already reflected in `total_minor`.
    #[serde(default)]
    pub discount_minor: i64,
    pub total_minor: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpDelegatePaymentResponse {
    pub id: String,
    pub status: String,
    pub token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    pub api_version: String,
}

pub fn cart_id_to_acp_session_id(cart_id: CartId) -> String {
    format!("cs_{}", cart_id.0)
}

pub fn parse_acp_session_id(id: &str) -> Result<CartId, String> {
    let raw = id.strip_prefix("cs_").unwrap_or(id);
    Uuid::from_str(raw)
        .map(CartId)
        .map_err(|e| format!("invalid checkout session id: {e}"))
}

/// ACP Cart Capability (added `2026-04-17`): pre-checkout basket state decoupled from the
/// checkout session, with estimated pricing. Reuses the checkout-session cart plumbing since
/// both are backed by the same orchestrator-native `CartProjection`.
pub const ACP_CART_ID_PREFIX: &str = "cart_";

pub fn cart_id_to_acp_cart_id(cart_id: CartId) -> String {
    format!("{ACP_CART_ID_PREFIX}{}", cart_id.0)
}

pub fn parse_acp_cart_id(id: &str) -> Result<CartId, String> {
    let raw = id.strip_prefix(ACP_CART_ID_PREFIX).unwrap_or(id);
    Uuid::from_str(raw)
        .map(CartId)
        .map_err(|e| format!("invalid cart id: {e}"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpCartCreateRequest {
    pub merchant_id: String,
    pub currency: String,
    #[serde(default)]
    pub line_items: Vec<AcpLineItem>,
    #[serde(default)]
    pub buyer: Option<AcpBuyer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpCartUpdateRequest {
    #[serde(default)]
    pub line_items: Vec<AcpLineItem>,
    #[serde(default)]
    pub buyer: Option<AcpBuyer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpCartResponse {
    pub id: String,
    pub status: String,
    pub currency: String,
    pub line_items: Vec<AcpSessionLine>,
    pub totals: AcpTotals,
    pub api_version: String,
}

pub fn create_cart_command_for_acp_cart(req: &AcpCartCreateRequest) -> CartCommand {
    CartCommand::CreateCart(CreateCartPayload {
        merchant_id: req.merchant_id.clone(),
        currency: req.currency.clone(),
        tenant_id: None,
    })
}

pub fn add_item_commands_for_acp_cart(req: &AcpCartCreateRequest) -> Vec<CartCommand> {
    req.line_items
        .iter()
        .map(|line| {
            CartCommand::AddItem(AddItemPayload {
                item_id: line.item.id.clone(),
                quantity: line.quantity,
            })
        })
        .collect()
}

pub fn projection_to_acp_cart(projection: &CartProjection) -> AcpCartResponse {
    let status = match projection.status {
        orchestrator_core::contract::CartStatus::Cancelled => "canceled",
        _ => "active",
    };
    AcpCartResponse {
        id: cart_id_to_acp_cart_id(projection.cart_id),
        status: status.to_string(),
        currency: projection.currency.clone(),
        line_items: projection
            .lines
            .iter()
            .map(|line| AcpSessionLine {
                id: line.line_id.clone(),
                item_id: line.item_id.clone(),
                quantity: line.quantity,
                unit_amount: line.unit_price_minor,
            })
            .collect(),
        totals: AcpTotals {
            subtotal_minor: projection.subtotal_minor,
            tax_minor: projection.tax_minor,
            discount_minor: projection.discount_minor,
            total_minor: projection.total_minor,
        },
        api_version: ACP_API_VERSION.to_string(),
    }
}

pub fn projection_to_acp_session(projection: &CartProjection) -> AcpCheckoutSessionResponse {
    let status = match projection.status {
        orchestrator_core::contract::CartStatus::CheckoutReady => "ready_for_payment",
        orchestrator_core::contract::CartStatus::Cancelled => "canceled",
        _ => "open",
    };
    AcpCheckoutSessionResponse {
        id: cart_id_to_acp_session_id(projection.cart_id),
        status: status.to_string(),
        currency: projection.currency.clone(),
        line_items: projection
            .lines
            .iter()
            .map(|line| AcpSessionLine {
                id: line.line_id.clone(),
                item_id: line.item_id.clone(),
                quantity: line.quantity,
                unit_amount: line.unit_price_minor,
            })
            .collect(),
        totals: AcpTotals {
            subtotal_minor: projection.subtotal_minor,
            tax_minor: projection.tax_minor,
            discount_minor: projection.discount_minor,
            total_minor: projection.total_minor,
        },
        api_version: ACP_API_VERSION.to_string(),
    }
}

pub fn create_cart_command(req: &AcpCheckoutSessionCreateRequest) -> CartCommand {
    CartCommand::CreateCart(CreateCartPayload {
        merchant_id: req.merchant_id.clone(),
        currency: req.currency.clone(),
        tenant_id: None,
    })
}

pub fn add_item_commands(req: &AcpCheckoutSessionCreateRequest) -> Vec<CartCommand> {
    req.line_items
        .iter()
        .map(|line| {
            CartCommand::AddItem(AddItemPayload {
                item_id: line.item.id.clone(),
                quantity: line.quantity,
            })
        })
        .collect()
}

pub fn get_cart_command(cart_id: CartId) -> CartCommand {
    CartCommand::GetCart(GetCartPayload { cart_id })
}

pub fn cancel_cart_command(cart_id: CartId) -> CartCommand {
    CartCommand::CancelCart(CancelCartPayload { cart_id })
}

pub fn start_checkout_command(cart_id: CartId, cart_version: u64) -> CartCommand {
    CartCommand::StartCheckout(StartCheckoutPayload {
        cart_id,
        cart_version,
    })
}

pub fn complete_to_checkout_request(
    cart_id: CartId,
    cart_version: u64,
    currency: &str,
    req: &AcpCompleteSessionRequest,
) -> CheckoutRequest {
    let method = req
        .payment_data
        .payment_method_type
        .as_deref()
        .and_then(|m| match m.to_ascii_lowercase().as_str() {
            "ap2" => Some(PaymentMethodType::Ap2),
            "mpp" => Some(PaymentMethodType::Mpp),
            _ => None,
        });
    CheckoutRequest {
        tenant_id: req.tenant_id.clone(),
        merchant_id: req.merchant_id.clone(),
        cart_id,
        cart_version,
        currency: currency.to_string(),
        customer: req.buyer.as_ref().map(|b| CustomerHint {
            email: b.email.clone(),
            full_name: b.full_name.clone(),
        }),
        location: None,
        payment_intent: PaymentIntent {
            amount_minor: req.payment_data.amount_minor,
            token_or_reference: req.payment_data.token.clone(),
            ap2_consent_proof: req.payment_data.ap2_consent_proof.clone(),
            payment_handler_id: req.payment_data.payment_handler_id.clone(),
            payment_method_type: method,
            mpp_method: req.payment_data.mpp_method.clone(),
            mpp_intent: req.payment_data.mpp_intent.clone(),
        },
        idempotency_key: req.idempotency_key.clone(),
    }
}

/// ACP discovery document (`GET /.well-known/acp.json`, added `2026-04-17`): advertises the
/// seller's protocol version, REST base URL, transports, and supported services/extensions.
/// See `rfcs/rfc.discovery.md` in the ACP spec repo.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpDiscoveryProtocol {
    pub name: String,
    pub version: String,
    pub supported_versions: Vec<String>,
    pub documentation_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpDiscoveryExtension {
    pub name: String,
    pub spec: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpDiscoveryCapabilities {
    pub services: Vec<String>,
    pub extensions: Vec<AcpDiscoveryExtension>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpDiscoveryDocument {
    pub protocol: AcpDiscoveryProtocol,
    pub api_base_url: String,
    pub transports: Vec<String>,
    pub capabilities: AcpDiscoveryCapabilities,
}

/// `payment_delegation` reflects whether a PSP delegation adapter is wired in;
/// the service is only advertised when the endpoint can actually do something.
pub fn build_acp_discovery_document(
    base_url: &str,
    payment_delegation: bool,
) -> AcpDiscoveryDocument {
    let base = base_url.trim_end_matches('/');
    let mut services = vec!["checkout".to_string(), "carts".to_string()];
    if payment_delegation {
        services.push("delegate_payment".to_string());
    }
    AcpDiscoveryDocument {
        protocol: AcpDiscoveryProtocol {
            name: "acp".to_string(),
            version: ACP_API_VERSION.to_string(),
            supported_versions: ACP_SUPPORTED_VERSIONS
                .iter()
                .map(|v| v.to_string())
                .collect(),
            documentation_url:
                "https://github.com/agentic-commerce-protocol/agentic-commerce-protocol".to_string(),
        },
        api_base_url: format!("{base}/api/v1/acp"),
        transports: vec!["rest".to_string()],
        capabilities: AcpDiscoveryCapabilities {
            services,
            extensions: vec![],
        },
    }
}

/// Map the PSP's delegated credential onto the ACP response shape. The token is
/// the PSP's, never the caller's own — echoing that back would delegate nothing.
pub fn delegate_payment_response(delegated: &DelegatedPayment) -> AcpDelegatePaymentResponse {
    AcpDelegatePaymentResponse {
        id: delegated.id.clone(),
        status: delegated.status.clone(),
        token: delegated.token.clone(),
        expires_at: delegated.expires_at.clone(),
        api_version: ACP_API_VERSION.to_string(),
    }
}
