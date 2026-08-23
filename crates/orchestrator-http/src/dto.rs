//! API v1 request/response DTOs. Transport boundary only; no internal types leak.

use orchestrator_core::contract::{
    AddItemPayload, ApplyAdjustmentPayload, CancelCartPayload, CartCommand, CartId,
    CartLineProjection, CartProjection, CartStatus, CheckoutRequest, CreateCartPayload,
    CustomerHint, GetCartPayload, LocationHint, OrderAdjustment, OrderEvent, OrderRecord,
    OrderStatus, PaymentIntent, PaymentLifecycleRequest, PaymentMethodType, PaymentState,
    RemoveItemPayload, SetFulfillmentSelectionPayload, StartCheckoutPayload, TransactionResult,
    TransactionStatus, UpdateItemQtyPayload,
};
use orchestrator_core::discount::AppliedDiscount;
use orchestrator_core::fulfillment::{
    FulfillmentDestination, FulfillmentGroup, FulfillmentMethod, FulfillmentMethodType,
    FulfillmentOption, FulfillmentState, PostalAddress,
};
use orchestrator_core::{UCP_LATEST_VERSION, UCP_SUPPORTED_VERSIONS};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::str::FromStr;
use utoipa::ToSchema;
use uuid::Uuid;

// ---- Cart command request ----

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CartCommandRequest {
    pub command: CartCommandDto,
    #[serde(default)]
    pub cart_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
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
    CancelCart {
        cart_id: String,
    },
}

impl TryFrom<CartCommandDto> for CartCommand {
    type Error = String;

    fn try_from(dto: CartCommandDto) -> Result<Self, Self::Error> {
        Ok(match dto {
            CartCommandDto::CreateCart {
                merchant_id,
                currency,
            } => CartCommand::CreateCart(CreateCartPayload {
                merchant_id,
                currency,
                tenant_id: None,
            }),
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
            CartCommandDto::GetCart { cart_id } => CartCommand::GetCart(GetCartPayload {
                cart_id: parse_cart_id(&cart_id)?,
            }),
            CartCommandDto::StartCheckout {
                cart_id,
                cart_version,
            } => CartCommand::StartCheckout(StartCheckoutPayload {
                cart_id: parse_cart_id(&cart_id)?,
                cart_version,
            }),
            CartCommandDto::CancelCart { cart_id } => CartCommand::CancelCart(CancelCartPayload {
                cart_id: parse_cart_id(&cart_id)?,
            }),
        })
    }
}

fn parse_cart_id(s: &str) -> Result<CartId, String> {
    Uuid::from_str(s).map(CartId).map_err(|e| e.to_string())
}

// ---- Cart projection response ----

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
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

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CartLineProjectionDto {
    pub line_id: String,
    pub item_id: String,
    pub title: String,
    pub quantity: u32,
    pub unit_price_minor: i64,
    pub total_minor: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CartStatusDto {
    Draft,
    CheckoutReady,
    Cancelled,
}

impl From<CartProjection> for CartProjectionDto {
    fn from(p: CartProjection) -> Self {
        Self {
            cart_id: p.cart_id.0.to_string(),
            version: p.version,
            currency: p.currency,
            lines: p
                .lines
                .into_iter()
                .map(CartLineProjectionDto::from)
                .collect(),
            subtotal_minor: p.subtotal_minor,
            tax_minor: p.tax_minor,
            total_minor: p.total_minor,
            geo_ok: p.geo_ok,
            status: match p.status {
                CartStatus::Draft => CartStatusDto::Draft,
                CartStatus::CheckoutReady => CartStatusDto::CheckoutReady,
                CartStatus::Cancelled => CartStatusDto::Cancelled,
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

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
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

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CustomerHintDto {
    pub email: Option<String>,
    pub full_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct LocationHintDto {
    pub country_code: Option<String>,
    pub region: Option<String>,
    pub postal_code: Option<String>,
    #[serde(default)]
    pub intent: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PaymentIntentDto {
    pub amount_minor: i64,
    pub token_or_reference: String,
    pub ap2_consent_proof: Option<String>,
    pub payment_handler_id: Option<String>,
    #[serde(default)]
    pub payment_method_type: Option<String>,
    #[serde(default)]
    pub mpp_method: Option<String>,
    #[serde(default)]
    pub mpp_intent: Option<String>,
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
                intent: l.intent,
            }),
            payment_intent: PaymentIntent {
                amount_minor: dto.payment_intent.amount_minor,
                token_or_reference: dto.payment_intent.token_or_reference,
                ap2_consent_proof: dto.payment_intent.ap2_consent_proof,
                payment_handler_id: dto.payment_intent.payment_handler_id,
                payment_method_type: match dto.payment_intent.payment_method_type.as_deref() {
                    Some("ap2") => Some(PaymentMethodType::Ap2),
                    Some("mpp") => Some(PaymentMethodType::Mpp),
                    Some(other) => {
                        return Err(format!("unsupported payment_method_type: {}", other));
                    }
                    None => None,
                },
                mpp_method: dto.payment_intent.mpp_method,
                mpp_intent: dto.payment_intent.mpp_intent,
            },
            idempotency_key: dto.idempotency_key,
        })
    }
}

// ---- Transaction result response ----

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TransactionStatusDto {
    Completed,
    Rejected,
    AuthFailed,
    CommitFailed,
    TimedOut,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TotalsBreakdownDto {
    pub subtotal_minor: i64,
    pub tax_minor: i64,
    pub discount_minor: i64,
    pub total_minor: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
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

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
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

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PaymentOperationResultDto {
    pub success: bool,
    pub reference: String,
}

// ---- Incoming event (idempotent) ----

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct IncomingEventRequestDto {
    pub message_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct IncomingEventResponseDto {
    pub accepted: bool,
}

// ---- Outbox / dead-letter / reconciliation ----

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProcessOutboxRequestDto {
    pub max_attempts: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DeadLetterEntryDto {
    pub id: String,
    pub topic: String,
    pub correlation_id: String,
    pub attempts: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReplayDeadLetterRequestDto {
    pub message_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReplayDeadLetterResponseDto {
    pub replayed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReconciliationRequestDto {
    pub transaction_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PaymentMismatchDto {
    pub transaction_id: String,
    pub our_state: String,
    pub provider_state: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReconciliationReportDto {
    pub mismatches: Vec<PaymentMismatchDto>,
}

// ---- Identity linking (A2A envelope response) ----

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UcpMetadataDto {
    pub version: String,
    pub supported_versions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct IdentityLinkResultDto {
    pub ucp: UcpMetadataDto,
    pub link_id: String,
    pub status: String,
}

impl From<orchestrator_api::IdentityLinkResult> for IdentityLinkResultDto {
    fn from(result: orchestrator_api::IdentityLinkResult) -> Self {
        Self {
            ucp: UcpMetadataDto {
                version: if result.ucp_version.is_empty() {
                    UCP_LATEST_VERSION.to_string()
                } else {
                    result.ucp_version
                },
                supported_versions: if result.supported_versions.is_empty() {
                    UCP_SUPPORTED_VERSIONS
                        .iter()
                        .map(|v| (*v).to_string())
                        .collect()
                } else {
                    result.supported_versions
                },
            },
            link_id: result.link_id,
            status: result.status,
        }
    }
}

// ---- Order query ----

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct OrderDto {
    pub order_id: String,
    pub tenant_id: String,
    pub transaction_id: String,
    pub checkout_id: String,
    pub status: OrderStatusDto,
    pub currency: String,
    pub permalink_url: String,
    pub line_items: Vec<CartLineProjectionDto>,
    pub totals: TotalsBreakdownDto,
    pub events: Vec<OrderEventDto>,
    pub adjustments: Vec<OrderAdjustmentDto>,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum OrderStatusDto {
    Created,
    FulfillmentPending,
    Fulfilled,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct OrderEventDto {
    pub id: String,
    pub event_type: String,
    pub description: String,
    pub occurred_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct OrderAdjustmentDto {
    pub id: String,
    pub adjustment_type: String,
    pub amount_minor: i64,
    pub status: String,
}

impl From<OrderRecord> for OrderDto {
    fn from(r: OrderRecord) -> Self {
        Self {
            order_id: r.order_id,
            tenant_id: r.tenant_id,
            transaction_id: r.transaction_id,
            checkout_id: r.checkout_id.0.to_string(),
            status: match r.status {
                OrderStatus::Created => OrderStatusDto::Created,
                OrderStatus::FulfillmentPending => OrderStatusDto::FulfillmentPending,
                OrderStatus::Fulfilled => OrderStatusDto::Fulfilled,
                OrderStatus::Cancelled => OrderStatusDto::Cancelled,
                _ => OrderStatusDto::Created,
            },
            currency: r.currency,
            permalink_url: r.permalink_url,
            line_items: r
                .line_items
                .into_iter()
                .map(CartLineProjectionDto::from)
                .collect(),
            totals: TotalsBreakdownDto {
                subtotal_minor: r.totals.subtotal_minor,
                tax_minor: r.totals.tax_minor,
                discount_minor: r.totals.discount_minor,
                total_minor: r.totals.total_minor,
            },
            events: r.events.into_iter().map(OrderEventDto::from).collect(),
            adjustments: r
                .adjustments
                .into_iter()
                .map(OrderAdjustmentDto::from)
                .collect(),
            created_at: r.created_at.to_rfc3339(),
        }
    }
}

impl From<OrderEvent> for OrderEventDto {
    fn from(e: OrderEvent) -> Self {
        Self {
            id: e.id,
            event_type: e.event_type,
            description: e.description,
            occurred_at: e.occurred_at.to_rfc3339(),
        }
    }
}

impl From<OrderAdjustment> for OrderAdjustmentDto {
    fn from(a: OrderAdjustment) -> Self {
        Self {
            id: a.id,
            adjustment_type: a.adjustment_type,
            amount_minor: a.amount_minor,
            status: a.status,
        }
    }
}

// ---- Catalog lookup ----

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CatalogItemDto {
    pub id: String,
    pub title: String,
    pub price_minor: i64,
}

impl From<provider_contracts::CatalogItem> for CatalogItemDto {
    fn from(item: provider_contracts::CatalogItem) -> Self {
        Self {
            id: item.id,
            title: item.title,
            price_minor: item.price_minor,
        }
    }
}

// ---- UCP-native REST envelopes ----

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UcpCapabilityDto {
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UcpEnvelopeDto {
    pub version: String,
    pub supported_versions: Vec<String>,
    pub capabilities: BTreeMap<String, Vec<UcpCapabilityDto>>,
}

impl UcpEnvelopeDto {
    pub fn for_capabilities(capabilities: &[&str]) -> Self {
        Self {
            version: UCP_LATEST_VERSION.to_string(),
            supported_versions: UCP_SUPPORTED_VERSIONS
                .iter()
                .map(|v| (*v).to_string())
                .collect(),
            capabilities: capabilities
                .iter()
                .map(|capability| {
                    (
                        (*capability).to_string(),
                        vec![UcpCapabilityDto {
                            version: UCP_LATEST_VERSION.to_string(),
                        }],
                    )
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UcpMessageDto {
    pub code: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UcpCartItemRefDto {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UcpCartLineRequestDto {
    #[serde(default)]
    pub id: Option<String>,
    pub item: UcpCartItemRefDto,
    pub quantity: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UcpCartRequestDto {
    #[serde(default)]
    pub merchant_id: Option<String>,
    pub currency: String,
    #[serde(default)]
    pub line_items: Vec<UcpCartLineRequestDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UcpCartLineDto {
    pub id: String,
    pub item: UcpCartItemRefDto,
    pub title: String,
    pub quantity: u32,
    pub unit_price_minor: i64,
    pub total_minor: i64,
}

impl From<CartLineProjection> for UcpCartLineDto {
    fn from(line: CartLineProjection) -> Self {
        Self {
            id: line.line_id,
            item: UcpCartItemRefDto { id: line.item_id },
            title: line.title,
            quantity: line.quantity,
            unit_price_minor: line.unit_price_minor,
            total_minor: line.total_minor,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UcpCartResponseDto {
    pub ucp: UcpEnvelopeDto,
    pub id: String,
    pub version: u64,
    pub currency: String,
    pub line_items: Vec<UcpCartLineDto>,
    pub subtotal_minor: i64,
    pub tax_minor: i64,
    #[serde(default)]
    pub fulfillment_minor: i64,
    /// Total deducted by applied adjustment codes, already reflected in `total_minor`.
    #[serde(default)]
    pub discount_minor: i64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub discounts: Vec<UcpDiscountDto>,
    pub total_minor: i64,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fulfillment: Option<UcpFulfillmentDto>,
}

/// A discount granted by the pricing provider (`dev.ucp.shopping.discount`).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UcpDiscountDto {
    pub code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub amount_minor: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_item_id: Option<String>,
}

impl From<AppliedDiscount> for UcpDiscountDto {
    fn from(discount: AppliedDiscount) -> Self {
        Self {
            code: discount.code,
            description: discount.description,
            amount_minor: discount.amount_minor,
            line_item_id: discount.line_id,
        }
    }
}

impl From<CartProjection> for UcpCartResponseDto {
    fn from(cart: CartProjection) -> Self {
        let status = match cart.status {
            CartStatus::Draft => "draft",
            CartStatus::CheckoutReady => "checkout_ready",
            CartStatus::Cancelled => "canceled",
            _ => "draft",
        }
        .to_string();
        let mut capabilities = vec!["dev.ucp.shopping.cart", "dev.ucp.shopping.fulfillment"];
        if !cart.discounts.is_empty() {
            capabilities.push("dev.ucp.shopping.discount");
        }
        Self {
            ucp: UcpEnvelopeDto::for_capabilities(&capabilities),
            id: cart.cart_id.0.to_string(),
            version: cart.version,
            currency: cart.currency,
            line_items: cart.lines.into_iter().map(UcpCartLineDto::from).collect(),
            subtotal_minor: cart.subtotal_minor,
            tax_minor: cart.tax_minor,
            fulfillment_minor: cart.fulfillment_minor,
            discount_minor: cart.discount_minor,
            discounts: cart
                .discounts
                .into_iter()
                .map(UcpDiscountDto::from)
                .collect(),
            total_minor: cart.total_minor,
            status,
            fulfillment: cart.fulfillment.map(UcpFulfillmentDto::from),
        }
    }
}

// ---- UCP Fulfillment extension (dev.ucp.shopping.fulfillment) ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum UcpFulfillmentMethodTypeDto {
    Shipping,
    Pickup,
}

impl From<FulfillmentMethodType> for UcpFulfillmentMethodTypeDto {
    fn from(method_type: FulfillmentMethodType) -> Self {
        match method_type {
            FulfillmentMethodType::Shipping => Self::Shipping,
            FulfillmentMethodType::Pickup => Self::Pickup,
            _ => Self::Shipping,
        }
    }
}

impl From<UcpFulfillmentMethodTypeDto> for FulfillmentMethodType {
    fn from(dto: UcpFulfillmentMethodTypeDto) -> Self {
        match dto {
            UcpFulfillmentMethodTypeDto::Shipping => Self::Shipping,
            UcpFulfillmentMethodTypeDto::Pickup => Self::Pickup,
        }
    }
}

/// Destination request fields. For `method_type: shipping`, address fields describe the
/// shipping address. For `method_type: pickup`, `name` identifies the retail location.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct UcpFulfillmentDestinationRequestDto {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub street_address: Option<String>,
    #[serde(default)]
    pub extended_address: Option<String>,
    #[serde(default)]
    pub address_locality: Option<String>,
    #[serde(default)]
    pub address_region: Option<String>,
    #[serde(default)]
    pub address_country: Option<String>,
    #[serde(default)]
    pub postal_code: Option<String>,
    #[serde(default)]
    pub first_name: Option<String>,
    #[serde(default)]
    pub last_name: Option<String>,
    #[serde(default)]
    pub phone_number: Option<String>,
}

impl From<&UcpFulfillmentDestinationRequestDto> for PostalAddress {
    fn from(dto: &UcpFulfillmentDestinationRequestDto) -> Self {
        Self {
            extended_address: dto.extended_address.clone(),
            street_address: dto.street_address.clone(),
            address_locality: dto.address_locality.clone(),
            address_region: dto.address_region.clone(),
            address_country: dto.address_country.clone(),
            postal_code: dto.postal_code.clone(),
            first_name: dto.first_name.clone(),
            last_name: dto.last_name.clone(),
            phone_number: dto.phone_number.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UcpFulfillmentSelectionRequestDto {
    pub method_type: UcpFulfillmentMethodTypeDto,
    /// Line items this selection covers; empty/omitted means all current cart lines.
    #[serde(default)]
    pub line_item_ids: Vec<String>,
    pub destination: UcpFulfillmentDestinationRequestDto,
    /// Option id to select immediately; omit to quote options without selecting one.
    #[serde(default)]
    pub selected_option_id: Option<String>,
}

impl TryFrom<UcpFulfillmentSelectionRequestDto> for SetFulfillmentSelectionPayload {
    type Error = String;

    fn try_from(dto: UcpFulfillmentSelectionRequestDto) -> Result<Self, Self::Error> {
        let method_type: FulfillmentMethodType = dto.method_type.into();
        let destination = match method_type {
            FulfillmentMethodType::Pickup => FulfillmentDestination::Retail {
                id: dto.destination.id.clone(),
                name: dto
                    .destination
                    .name
                    .clone()
                    .ok_or_else(|| "destination.name is required for pickup".to_string())?,
                address: Some(PostalAddress::from(&dto.destination)),
            },
            FulfillmentMethodType::Shipping => FulfillmentDestination::Shipping {
                id: dto.destination.id.clone(),
                address: PostalAddress::from(&dto.destination),
            },
            _ => FulfillmentDestination::Shipping {
                id: dto.destination.id.clone(),
                address: PostalAddress::from(&dto.destination),
            },
        };
        Ok(SetFulfillmentSelectionPayload {
            method_type,
            line_item_ids: dto.line_item_ids,
            destination,
            selected_option_id: dto.selected_option_id,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UcpFulfillmentOptionDto {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub carrier: Option<String>,
    pub amount_minor: i64,
}

impl From<FulfillmentOption> for UcpFulfillmentOptionDto {
    fn from(option: FulfillmentOption) -> Self {
        Self {
            id: option.id,
            title: option.title,
            description: option.description,
            carrier: option.carrier,
            amount_minor: option.amount_minor,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UcpFulfillmentGroupDto {
    pub id: String,
    pub line_item_ids: Vec<String>,
    pub options: Vec<UcpFulfillmentOptionDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_option_id: Option<String>,
}

impl From<FulfillmentGroup> for UcpFulfillmentGroupDto {
    fn from(group: FulfillmentGroup) -> Self {
        Self {
            id: group.id,
            line_item_ids: group.line_item_ids,
            options: group
                .options
                .into_iter()
                .map(UcpFulfillmentOptionDto::from)
                .collect(),
            selected_option_id: group.selected_option_id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UcpFulfillmentMethodDto {
    pub id: String,
    #[serde(rename = "type")]
    pub method_type: UcpFulfillmentMethodTypeDto,
    pub line_item_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_destination_id: Option<String>,
    pub groups: Vec<UcpFulfillmentGroupDto>,
}

impl From<FulfillmentMethod> for UcpFulfillmentMethodDto {
    fn from(method: FulfillmentMethod) -> Self {
        Self {
            id: method.id,
            method_type: method.method_type.into(),
            line_item_ids: method.line_item_ids,
            selected_destination_id: method.selected_destination_id,
            groups: method
                .groups
                .into_iter()
                .map(UcpFulfillmentGroupDto::from)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UcpFulfillmentDto {
    pub methods: Vec<UcpFulfillmentMethodDto>,
}

impl From<FulfillmentState> for UcpFulfillmentDto {
    fn from(state: FulfillmentState) -> Self {
        Self {
            methods: state
                .methods
                .into_iter()
                .map(UcpFulfillmentMethodDto::from)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UcpCatalogSearchRequestDto {
    #[serde(default)]
    pub query: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UcpCatalogLookupRequestDto {
    pub ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UcpCatalogProductRequestDto {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UcpCatalogProductsResponseDto {
    pub ucp: UcpEnvelopeDto,
    pub products: Vec<CatalogItemDto>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<UcpMessageDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UcpCatalogProductResponseDto {
    pub ucp: UcpEnvelopeDto,
    pub product: CatalogItemDto,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<UcpMessageDto>,
}

/// Response for `POST /api/v1/ucp/checkout/:id/embedded-link` (UCP "embedded" transport /
/// link delegation): a short-lived, signed handoff URL for completing checkout on the
/// merchant's hosted embedded surface.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UcpEmbeddedCheckoutLinkResponseDto {
    pub ucp: UcpEnvelopeDto,
    pub checkout_session_id: String,
    pub embedded_url: String,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UcpSignedAmountDto {
    pub amount: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UcpTotalDto {
    #[serde(rename = "type")]
    pub total_type: String,
    pub amount: UcpSignedAmountDto,
    pub currency: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UcpOrderResponseDto {
    pub ucp: UcpEnvelopeDto,
    pub id: String,
    pub status: OrderStatusDto,
    pub currency: String,
    pub permalink_url: String,
    pub line_items: Vec<UcpCartLineDto>,
    pub totals: Vec<UcpTotalDto>,
    pub events: Vec<OrderEventDto>,
    pub created_at: String,
}

impl From<OrderRecord> for UcpOrderResponseDto {
    fn from(order: OrderRecord) -> Self {
        let status = match order.status {
            OrderStatus::Created => OrderStatusDto::Created,
            OrderStatus::FulfillmentPending => OrderStatusDto::FulfillmentPending,
            OrderStatus::Fulfilled => OrderStatusDto::Fulfilled,
            OrderStatus::Cancelled => OrderStatusDto::Cancelled,
            _ => OrderStatusDto::Created,
        };
        let totals = vec![
            UcpTotalDto {
                total_type: "subtotal".to_string(),
                amount: UcpSignedAmountDto {
                    amount: order.totals.subtotal_minor,
                },
                currency: order.currency.clone(),
            },
            UcpTotalDto {
                total_type: "tax".to_string(),
                amount: UcpSignedAmountDto {
                    amount: order.totals.tax_minor,
                },
                currency: order.currency.clone(),
            },
            UcpTotalDto {
                total_type: "discount".to_string(),
                amount: UcpSignedAmountDto {
                    amount: -order.totals.discount_minor.abs(),
                },
                currency: order.currency.clone(),
            },
            UcpTotalDto {
                total_type: "total".to_string(),
                amount: UcpSignedAmountDto {
                    amount: order.totals.total_minor,
                },
                currency: order.currency.clone(),
            },
        ];
        Self {
            ucp: UcpEnvelopeDto::for_capabilities(&["dev.ucp.shopping.order"]),
            id: order.order_id,
            status,
            currency: order.currency,
            permalink_url: order.permalink_url,
            line_items: order
                .line_items
                .into_iter()
                .map(UcpCartLineDto::from)
                .collect(),
            totals,
            events: order.events.into_iter().map(OrderEventDto::from).collect(),
            created_at: order.created_at.to_rfc3339(),
        }
    }
}

// ---- Webhook registration ----

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WebhookRegistrationRequestDto {
    pub url: String,
    pub secret: String,
    #[serde(default)]
    pub event_filter: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WebhookRegistrationDto {
    pub id: String,
    pub tenant_id: String,
    pub url: String,
    pub event_filter: Option<Vec<String>>,
    pub active: bool,
}

impl From<orchestrator_runtime::WebhookRegistration> for WebhookRegistrationDto {
    fn from(r: orchestrator_runtime::WebhookRegistration) -> Self {
        Self {
            id: r.id,
            tenant_id: r.tenant_id,
            url: r.url,
            event_filter: r.event_filter,
            active: r.active,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WebhookUnregisterRequestDto {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct WebhookUnregisterResponseDto {
    pub removed: bool,
}
