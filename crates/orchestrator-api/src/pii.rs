//! PII and sensitive field redaction for logs and audit payloads.

use orchestrator_core::contract::{CheckoutRequest, CustomerHint, PaymentIntent};

/// Redacted placeholder for sensitive fields in logs.
pub const REDACTED: &str = "[REDACTED]";

/// Payment intent with token and consent proof redacted for safe logging.
#[derive(Debug, Clone)]
pub struct RedactedPaymentIntent {
    pub amount_minor: i64,
    pub token_or_reference: String,
    pub ap2_consent_proof: Option<String>,
    pub payment_handler_id: Option<String>,
}

impl RedactedPaymentIntent {
    pub fn from(intent: &PaymentIntent) -> Self {
        Self {
            amount_minor: intent.amount_minor,
            token_or_reference: REDACTED.to_string(),
            ap2_consent_proof: intent
                .ap2_consent_proof
                .as_ref()
                .map(|_| REDACTED.to_string()),
            payment_handler_id: intent.payment_handler_id.clone(),
        }
    }
}

/// Customer hint with email and full name redacted.
#[derive(Debug, Clone)]
pub struct RedactedCustomerHint {
    pub email: Option<String>,
    pub full_name: Option<String>,
}

impl RedactedCustomerHint {
    pub fn from(c: &CustomerHint) -> Self {
        Self {
            email: c.email.as_ref().map(|_| REDACTED.to_string()),
            full_name: c.full_name.as_ref().map(|_| REDACTED.to_string()),
        }
    }
}

/// Checkout request with PII redacted for logging and audit sinks.
#[derive(Debug, Clone)]
pub struct RedactedCheckoutRequest {
    pub tenant_id: String,
    pub merchant_id: String,
    pub cart_id: orchestrator_core::contract::CartId,
    pub cart_version: u64,
    pub currency: String,
    pub customer: Option<RedactedCustomerHint>,
    pub location: Option<orchestrator_core::contract::LocationHint>,
    pub payment_intent: RedactedPaymentIntent,
    pub idempotency_key: String,
}

/// Redact a checkout request for safe logging. Never log the raw request.
pub fn redact_checkout_request(req: &CheckoutRequest) -> RedactedCheckoutRequest {
    RedactedCheckoutRequest {
        tenant_id: req.tenant_id.clone(),
        merchant_id: req.merchant_id.clone(),
        cart_id: req.cart_id,
        cart_version: req.cart_version,
        currency: req.currency.clone(),
        customer: req.customer.as_ref().map(RedactedCustomerHint::from),
        location: req.location.clone(),
        payment_intent: RedactedPaymentIntent::from(&req.payment_intent),
        idempotency_key: req.idempotency_key.clone(),
    }
}
