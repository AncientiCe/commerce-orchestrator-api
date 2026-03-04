//! Cart and transaction request/response contracts.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Cart command kinds for the full AI journey.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum CartCommand {
    CreateCart(CreateCartPayload),
    AddItem(AddItemPayload),
    UpdateItemQty(UpdateItemQtyPayload),
    RemoveItem(RemoveItemPayload),
    ApplyAdjustment(ApplyAdjustmentPayload),
    GetCart(GetCartPayload),
    StartCheckout(StartCheckoutPayload),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateCartPayload {
    pub merchant_id: String,
    pub currency: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddItemPayload {
    pub item_id: String,
    pub quantity: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateItemQtyPayload {
    pub line_id: String,
    pub quantity: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoveItemPayload {
    pub line_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyAdjustmentPayload {
    pub code: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GetCartPayload {
    pub cart_id: CartId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartCheckoutPayload {
    pub cart_id: CartId,
    pub cart_version: u64,
}

/// Cart/checkout identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CartId(pub Uuid);

impl CartId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for CartId {
    fn default() -> Self {
        Self::new()
    }
}

/// Checkout request: merchant context, cart id/version, customer/location, payment, idempotency.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckoutRequest {
    pub tenant_id: String,
    pub merchant_id: String,
    pub cart_id: CartId,
    pub cart_version: u64,
    pub currency: String,
    pub customer: Option<CustomerHint>,
    pub location: Option<LocationHint>,
    pub payment_intent: PaymentIntent,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomerHint {
    pub email: Option<String>,
    pub full_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocationHint {
    pub country_code: Option<String>,
    pub region: Option<String>,
    pub postal_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentIntent {
    pub amount_minor: i64,
    pub token_or_reference: String,
    pub ap2_consent_proof: Option<String>,
    pub payment_handler_id: Option<String>,
}

/// Cart projection returned from GetCart / after mutations: lines, totals, tax/geo flags.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CartProjection {
    pub cart_id: CartId,
    pub version: u64,
    pub currency: String,
    pub lines: Vec<CartLineProjection>,
    pub subtotal_minor: i64,
    pub tax_minor: i64,
    pub total_minor: i64,
    pub geo_ok: bool,
    pub status: CartStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CartLineProjection {
    pub line_id: String,
    pub item_id: String,
    pub title: String,
    pub quantity: u32,
    pub unit_price_minor: i64,
    pub total_minor: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum CartStatus {
    Draft,
    CheckoutReady,
}

/// Transaction terminal output after checkout execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionResult {
    pub transaction_id: String,
    pub status: TransactionStatus,
    pub totals_breakdown: TotalsBreakdown,
    pub payment_reference: Option<String>,
    pub receipt_payload: Option<String>,
    pub correlation_id: Uuid,
    pub audit_trail_id: Option<String>,
    pub payment_state: PaymentState,
    pub order_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum TransactionStatus {
    Completed,
    Rejected,
    AuthFailed,
    CommitFailed,
    TimedOut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum PaymentState {
    Authorized,
    Captured,
    Voided,
    RefundPending,
    Refunded,
    Reconciled,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentLifecycleRequest {
    pub tenant_id: String,
    pub merchant_id: String,
    pub transaction_id: String,
    pub amount_minor: i64,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderRecord {
    pub order_id: String,
    pub transaction_id: String,
    pub checkout_id: CartId,
    pub status: OrderStatus,
    pub events: Vec<OrderEvent>,
    pub adjustments: Vec<OrderAdjustment>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum OrderStatus {
    Created,
    FulfillmentPending,
    Fulfilled,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderEvent {
    pub id: String,
    pub event_type: String,
    pub description: String,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderAdjustment {
    pub id: String,
    pub adjustment_type: String,
    pub amount_minor: i64,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TotalsBreakdown {
    pub subtotal_minor: i64,
    pub tax_minor: i64,
    pub discount_minor: i64,
    pub total_minor: i64,
}
