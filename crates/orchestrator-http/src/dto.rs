//! API v1 request/response DTOs. Transport boundary only; no internal types leak.

use orchestrator_core::contract::{
    AddItemPayload, ApplyAdjustmentPayload, CartCommand, CartId, CartLineProjection, CartProjection,
    CartStatus, CheckoutRequest, CreateCartPayload, CustomerHint, GetCartPayload, LocationHint,
    PaymentIntent, PaymentLifecycleRequest, RemoveItemPayload, StartCheckoutPayload,
    TransactionResult, TransactionStatus, PaymentState,
    UpdateItemQtyPayload,
};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use uuid::Uuid;

// ---- Cart command request ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CartCommandRequest {
    pub command: CartCommandDto,
    #[serde(default)]
    pub cart_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CartCommandDto {
    CreateCart {
        merchant_id: String,
        currency: String,
    },
    AddItem {
        item_id: String,
        quantity: u32,
    },
    UpdateItemQty {
        line_id: String,
        quantity: u32,
    },
    RemoveItem {
        line_id: String,
    },
    ApplyAdjustment {
        code: String,
    },
    GetCart {
        cart_id: String,
    },
    StartCheckout {
        cart_id: String,
        cart_version: u64,
    },
}

impl TryFrom<CartCommandDto> for CartCommand {
    type Error = String;

    fn try_from(dto: CartCommandDto) -> Result<Self, Self::Error> {
        Ok(match dto {
            CartCommandDto::CreateCart { merchant_id, currency } => {
                CartCommand::CreateCart(CreateCartPayload { merchant_id, currency })
            }
            CartCommandDto::AddItem { item_id, quantity } => {
                CartCommand::AddItem(AddItemPayload { item_id, quantity })
            }
            CartCommandDto::UpdateItemQty { line_id, quantity } => {
                CartCommand::UpdateItemQty(UpdateItemQtyPayload { line_id, quantity })
            }
            CartCommandDto::RemoveItem { line_id } => {
                CartCommand::RemoveItem(RemoveItemPayload { line_id })
            }
            CartCommandDto::ApplyAdjustment { code } => {
                CartCommand::ApplyAdjustment(ApplyAdjustmentPayload { code })
            }
            CartCommandDto::GetCart { cart_id } => {
                CartCommand::GetCart(GetCartPayload { cart_id: parse_cart_id(&cart_id)? })
            }
            CartCommandDto::StartCheckout { cart_id, cart_version } => {
                CartCommand::StartCheckout(StartCheckoutPayload {
                    cart_id: parse_cart_id(&cart_id)?,
                    cart_version,
                })
            }
        })
    }
}

fn parse_cart_id(s: &str) -> Result<CartId, String> {
    Uuid::from_str(s).map(CartId).map_err(|e| e.to_string())
}

// ---- Cart projection response ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CartProjectionDto {
    pub cart_id: String,
    pub version: u64,
    pub currency: String,
    pub lines: Vec<CartLineProjectionDto>,
    pub subtotal_minor: i64,
    pub tax_minor: i64,
    pub total_minor: i64,
    pub geo_ok: bool,
    pub status: CartStatusDto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CartLineProjectionDto {
    pub line_id: String,
    pub item_id: String,
    pub title: String,
    pub quantity: u32,
    pub unit_price_minor: i64,
    pub total_minor: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CartStatusDto {
    Draft,
    CheckoutReady,
}

impl From<CartProjection> for CartProjectionDto {
    fn from(p: CartProjection) -> Self {
        Self {
            cart_id: p.cart_id.0.to_string(),
            version: p.version,
            currency: p.currency,
            lines: p.lines.into_iter().map(CartLineProjectionDto::from).collect(),
            subtotal_minor: p.subtotal_minor,
            tax_minor: p.tax_minor,
            total_minor: p.total_minor,
            geo_ok: p.geo_ok,
            status: match p.status {
                CartStatus::Draft => CartStatusDto::Draft,
                CartStatus::CheckoutReady => CartStatusDto::CheckoutReady,
                _ => CartStatusDto::Draft,
            },
        }
    }
}

impl From<CartLineProjection> for CartLineProjectionDto {
    fn from(l: CartLineProjection) -> Self {
        Self {
            line_id: l.line_id,
            item_id: l.item_id,
            title: l.title,
            quantity: l.quantity,
            unit_price_minor: l.unit_price_minor,
            total_minor: l.total_minor,
        }
    }
}

// ---- Checkout request ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckoutRequestDto {
    pub tenant_id: String,
    pub merchant_id: String,
    pub cart_id: String,
    pub cart_version: u64,
    pub currency: String,
    pub customer: Option<CustomerHintDto>,
    pub location: Option<LocationHintDto>,
    pub payment_intent: PaymentIntentDto,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomerHintDto {
    pub email: Option<String>,
    pub full_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocationHintDto {
    pub country_code: Option<String>,
    pub region: Option<String>,
    pub postal_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentIntentDto {
    pub amount_minor: i64,
    pub token_or_reference: String,
    pub ap2_consent_proof: Option<String>,
    pub payment_handler_id: Option<String>,
}

impl TryFrom<CheckoutRequestDto> for CheckoutRequest {
    type Error = String;

    fn try_from(dto: CheckoutRequestDto) -> Result<Self, Self::Error> {
        Ok(CheckoutRequest {
            tenant_id: dto.tenant_id,
            merchant_id: dto.merchant_id,
            cart_id: parse_cart_id(&dto.cart_id)?,
            cart_version: dto.cart_version,
            currency: dto.currency,
            customer: dto.customer.map(|c| CustomerHint {
                email: c.email,
                full_name: c.full_name,
            }),
            location: dto.location.map(|l| LocationHint {
                country_code: l.country_code,
                region: l.region,
                postal_code: l.postal_code,
            }),
            payment_intent: PaymentIntent {
                amount_minor: dto.payment_intent.amount_minor,
                token_or_reference: dto.payment_intent.token_or_reference,
                ap2_consent_proof: dto.payment_intent.ap2_consent_proof,
                payment_handler_id: dto.payment_intent.payment_handler_id,
            },
            idempotency_key: dto.idempotency_key,
        })
    }
}

// ---- Transaction result response ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionResultDto {
    pub transaction_id: String,
    pub status: TransactionStatusDto,
    pub totals_breakdown: TotalsBreakdownDto,
    pub payment_reference: Option<String>,
    pub receipt_payload: Option<String>,
    pub correlation_id: String,
    pub audit_trail_id: Option<String>,
    pub payment_state: PaymentStateDto,
    pub order_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransactionStatusDto {
    Completed,
    Rejected,
    AuthFailed,
    CommitFailed,
    TimedOut,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TotalsBreakdownDto {
    pub subtotal_minor: i64,
    pub tax_minor: i64,
    pub discount_minor: i64,
    pub total_minor: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaymentStateDto {
    Authorized,
    Captured,
    Voided,
    RefundPending,
    Refunded,
    Reconciled,
    Failed,
}

impl From<TransactionResult> for TransactionResultDto {
    fn from(r: TransactionResult) -> Self {
        Self {
            transaction_id: r.transaction_id,
            status: match r.status {
                TransactionStatus::Completed => TransactionStatusDto::Completed,
                TransactionStatus::Rejected => TransactionStatusDto::Rejected,
                TransactionStatus::AuthFailed => TransactionStatusDto::AuthFailed,
                TransactionStatus::CommitFailed => TransactionStatusDto::CommitFailed,
                TransactionStatus::TimedOut => TransactionStatusDto::TimedOut,
                _ => TransactionStatusDto::Rejected,
            },
            totals_breakdown: TotalsBreakdownDto {
                subtotal_minor: r.totals_breakdown.subtotal_minor,
                tax_minor: r.totals_breakdown.tax_minor,
                discount_minor: r.totals_breakdown.discount_minor,
                total_minor: r.totals_breakdown.total_minor,
            },
            payment_reference: r.payment_reference,
            receipt_payload: r.receipt_payload,
            correlation_id: r.correlation_id.to_string(),
            audit_trail_id: r.audit_trail_id,
            payment_state: match r.payment_state {
                PaymentState::Authorized => PaymentStateDto::Authorized,
                PaymentState::Captured => PaymentStateDto::Captured,
                PaymentState::Voided => PaymentStateDto::Voided,
                PaymentState::RefundPending => PaymentStateDto::RefundPending,
                PaymentState::Refunded => PaymentStateDto::Refunded,
                PaymentState::Reconciled => PaymentStateDto::Reconciled,
                PaymentState::Failed => PaymentStateDto::Failed,
                _ => PaymentStateDto::Failed,
            },
            order_id: r.order_id,
        }
    }
}

// ---- Payment lifecycle request ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentLifecycleRequestDto {
    pub tenant_id: String,
    pub merchant_id: String,
    pub transaction_id: String,
    pub amount_minor: i64,
    pub idempotency_key: String,
}

impl From<PaymentLifecycleRequestDto> for PaymentLifecycleRequest {
    fn from(dto: PaymentLifecycleRequestDto) -> Self {
        PaymentLifecycleRequest {
            tenant_id: dto.tenant_id,
            merchant_id: dto.merchant_id,
            transaction_id: dto.transaction_id,
            amount_minor: dto.amount_minor,
            idempotency_key: dto.idempotency_key,
        }
    }
}

// ---- Payment operation result (from provider) ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentOperationResultDto {
    pub success: bool,
    pub reference: String,
}

// ---- Incoming event (idempotent) ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncomingEventRequestDto {
    pub message_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncomingEventResponseDto {
    pub accepted: bool,
}

// ---- Outbox / dead-letter / reconciliation ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessOutboxRequestDto {
    pub max_attempts: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadLetterEntryDto {
    pub id: String,
    pub topic: String,
    pub correlation_id: String,
    pub attempts: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayDeadLetterRequestDto {
    pub message_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayDeadLetterResponseDto {
    pub replayed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconciliationRequestDto {
    pub transaction_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentMismatchDto {
    pub transaction_id: String,
    pub our_state: String,
    pub provider_state: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconciliationReportDto {
    pub mismatches: Vec<PaymentMismatchDto>,
}
