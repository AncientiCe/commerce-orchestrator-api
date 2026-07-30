//! ACP (Agentic Commerce Protocol) mapping: OpenAI/Stripe checkout session DTOs ↔ domain.

use orchestrator_core::contract::{
    AddItemPayload, CancelCartPayload, CartCommand, CartId, CartProjection, CheckoutRequest,
    CreateCartPayload, CustomerHint, GetCartPayload, PaymentIntent, PaymentMethodType,
    StartCheckoutPayload,
};
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
    pub total_minor: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpDelegatePaymentResponse {
    pub id: String,
    pub status: String,
    pub token: String,
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
            total_minor: projection.total_minor,
        },
        api_version: ACP_API_VERSION.to_string(),
    }
}

pub fn create_cart_command(req: &AcpCheckoutSessionCreateRequest) -> CartCommand {
    CartCommand::CreateCart(CreateCartPayload {
        merchant_id: req.merchant_id.clone(),
        currency: req.currency.clone(),
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

pub fn delegate_payment_response(req: &AcpDelegatePaymentRequest) -> AcpDelegatePaymentResponse {
    AcpDelegatePaymentResponse {
        id: format!("dpay_{}", Uuid::new_v4()),
        status: "delegated".to_string(),
        token: req.payment_method.token.clone(),
        api_version: ACP_API_VERSION.to_string(),
    }
}
