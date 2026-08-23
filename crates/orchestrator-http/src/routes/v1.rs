//! API v1 routes: cart, checkout, payments, events, operations.

use crate::auth::AuthContextExtractor;
use crate::dto::{
    CartCommandRequest, CartProjectionDto, CatalogItemDto, CheckoutRequestDto, DeadLetterEntryDto,
    IdentityLinkResultDto, IncomingEventRequestDto, IncomingEventResponseDto, OrderDto,
    PaymentLifecycleRequestDto, PaymentMismatchDto, PaymentOperationResultDto,
    ProcessOutboxRequestDto, ReconciliationReportDto, ReconciliationRequestDto,
    ReplayDeadLetterRequestDto, ReplayDeadLetterResponseDto, TransactionResultDto,
    UcpCartRequestDto, UcpCartResponseDto, UcpCatalogLookupRequestDto, UcpCatalogProductRequestDto,
    UcpCatalogProductResponseDto, UcpCatalogProductsResponseDto, UcpCatalogSearchRequestDto,
    UcpEmbeddedCheckoutLinkResponseDto, UcpEnvelopeDto, UcpFulfillmentSelectionRequestDto,
    UcpMessageDto, UcpOrderResponseDto, WebhookRegistrationDto, WebhookRegistrationRequestDto,
    WebhookUnregisterResponseDto,
};
use crate::error::ApiError;
use crate::state::AppState;
use axum::{
    extract::{Path, State},
    http::HeaderMap,
    routing::{get, post, put},
    Json, Router,
};
use orchestrator_api::{
    add_item_commands, add_item_commands_for_acp_cart, build_embedded_checkout_link,
    cancel_cart_command, complete_to_checkout_request, create_cart_command,
    create_cart_command_for_acp_cart, default_payment_handlers, delegate_payment_response,
    get_cart_command, negotiate_a2a_version, negotiate_acp_version, normalize_a2a_cart_envelope,
    normalize_a2a_checkout_envelope, normalize_a2a_identity_link_envelope, parse_acp_cart_id,
    parse_acp_session_id, projection_to_acp_cart, projection_to_acp_session,
    redact_checkout_request, start_checkout_command, AcpCartCreateRequest, AcpCartUpdateRequest,
    AcpCheckoutSessionCreateRequest, AcpCheckoutSessionUpdateRequest, AcpCompleteSessionRequest,
    AcpDelegatePaymentRequest, PaymentHandlerDescriptor,
};
use orchestrator_core::contract::{
    AddItemPayload, CancelCartPayload, CartCommand, CartId, CheckoutRequest, CreateCartPayload,
    GetCartPayload, RemoveItemPayload, SetFulfillmentSelectionPayload, StartCheckoutPayload,
    UpdateItemQtyPayload,
};
use provider_contracts::PaymentDelegationRequest;
use std::collections::HashSet;
use std::str::FromStr;
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/cart/commands", post(dispatch_cart_command))
        .route("/ucp/cart", post(ucp_create_cart))
        .route("/ucp/cart/:id", get(ucp_get_cart))
        .route("/ucp/cart/:id", put(ucp_update_cart))
        .route("/ucp/cart/:id/cancel", post(ucp_cancel_cart))
        .route("/ucp/cart/:id/fulfillment", post(ucp_set_cart_fulfillment))
        .route("/ucp/checkout", post(ucp_create_checkout))
        .route("/ucp/checkout/:id", get(ucp_get_checkout))
        .route("/ucp/checkout/:id", put(ucp_update_checkout))
        .route("/ucp/checkout/:id/complete", post(ucp_complete_checkout))
        .route("/ucp/checkout/:id/cancel", post(ucp_cancel_checkout))
        .route(
            "/ucp/checkout/:id/fulfillment",
            post(ucp_set_checkout_fulfillment),
        )
        .route(
            "/ucp/checkout/:id/embedded-link",
            post(ucp_create_embedded_checkout_link),
        )
        .route("/ucp/payment-handlers", get(ucp_list_payment_handlers))
        .route("/ucp/payment-handlers/:id", get(ucp_get_payment_handler))
        .route("/ucp/catalog/search", post(ucp_catalog_search))
        .route("/ucp/catalog/lookup", post(ucp_catalog_lookup))
        .route("/ucp/catalog/product", post(ucp_catalog_product))
        .route("/ucp/orders/:id", get(ucp_get_order))
        .route("/acp/checkout_sessions", post(acp_create_session))
        .route("/acp/checkout_sessions/:id", get(acp_get_session))
        .route("/acp/checkout_sessions/:id", put(acp_update_session))
        .route(
            "/acp/checkout_sessions/:id/complete",
            post(acp_complete_session),
        )
        .route(
            "/acp/checkout_sessions/:id/cancel",
            post(acp_cancel_session),
        )
        .route("/acp/delegate_payment", post(acp_delegate_payment))
        .route("/acp/carts", post(acp_create_cart))
        .route("/acp/carts/:id", get(acp_get_cart))
        .route("/acp/carts/:id", put(acp_update_cart_endpoint))
        .route("/acp/carts/:id/cancel", post(acp_cancel_cart_endpoint))
        .route("/checkout/execute", post(execute_checkout))
        .route("/a2a/checkout", post(a2a_execute_checkout))
        .route("/a2a/cart", post(a2a_dispatch_cart_command))
        .route("/a2a/identity/link", post(a2a_link_identity))
        .route("/a2a/orders", post(a2a_get_order))
        .route("/orders", get(list_orders))
        .route("/orders/:id", get(get_order))
        .route("/catalog/items/:id", get(lookup_catalog_item))
        .route("/webhooks", post(register_webhook))
        .route("/webhooks", get(list_webhooks))
        .route("/webhooks/:id", axum::routing::delete(unregister_webhook))
        .route("/payments/capture", post(capture_payment))
        .route("/payments/void", post(void_payment))
        .route("/payments/refund", post(refund_payment))
        .route("/mcp/message", post(mcp_message))
        .route("/events/incoming", post(accept_incoming_event))
        .route("/ops/outbox/process", post(process_outbox))
        .route("/ops/dead-letter", get(list_dead_letter))
        .route("/ops/dead-letter/replay", post(replay_dead_letter))
        .route("/ops/reconciliation", post(run_reconciliation))
}

async fn dispatch_cart_command(
    AuthContextExtractor(auth): AuthContextExtractor,
    State(state): State<AppState>,
    Json(req): Json<CartCommandRequest>,
) -> Result<Json<CartProjectionDto>, ApiError> {
    let cmd = CartCommand::try_from(req.command).map_err(ApiError::BadRequest)?;
    let cart_id = req
        .cart_id
        .as_deref()
        .map(|s| Uuid::from_str(s).map(CartId))
        .transpose()
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let projection = state
        .facade
        .dispatch_cart_command_for_tenant(&auth.tenant_id, cmd, cart_id)
        .await?;
    Ok(Json(projection.into()))
}

async fn ucp_create_cart(
    AuthContextExtractor(auth): AuthContextExtractor,
    State(state): State<AppState>,
    Json(req): Json<UcpCartRequestDto>,
) -> Result<Json<UcpCartResponseDto>, ApiError> {
    let merchant_id = req
        .merchant_id
        .ok_or_else(|| ApiError::BadRequest("merchant_id is required".to_string()))?;
    let mut projection = state
        .facade
        .dispatch_cart_command_for_tenant(
            &auth.tenant_id,
            CartCommand::CreateCart(CreateCartPayload {
                merchant_id,
                currency: req.currency,
                tenant_id: Some(auth.tenant_id.clone()),
            }),
            None,
        )
        .await?;
    let cart_id = projection.cart_id;
    for line in req.line_items {
        projection = state
            .facade
            .dispatch_cart_command_for_tenant(
                &auth.tenant_id,
                CartCommand::AddItem(AddItemPayload {
                    item_id: line.item.id,
                    quantity: line.quantity,
                }),
                Some(cart_id),
            )
            .await?;
    }
    orchestrator_observability::incr("ucp_cart_create_total");
    Ok(Json(projection.into()))
}

async fn ucp_get_cart(
    AuthContextExtractor(auth): AuthContextExtractor,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<UcpCartResponseDto>, ApiError> {
    let cart_id = parse_ucp_cart_id(&id)?;
    let projection = state
        .facade
        .dispatch_cart_command_for_tenant(
            &auth.tenant_id,
            CartCommand::GetCart(GetCartPayload { cart_id }),
            None,
        )
        .await?;
    orchestrator_observability::incr("ucp_cart_get_total");
    Ok(Json(projection.into()))
}

async fn ucp_update_cart(
    AuthContextExtractor(auth): AuthContextExtractor,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<UcpCartRequestDto>,
) -> Result<Json<UcpCartResponseDto>, ApiError> {
    let cart_id = parse_ucp_cart_id(&id)?;
    let mut projection = state
        .facade
        .dispatch_cart_command_for_tenant(
            &auth.tenant_id,
            CartCommand::GetCart(GetCartPayload { cart_id }),
            None,
        )
        .await?;
    let desired_existing: HashSet<String> = req
        .line_items
        .iter()
        .filter_map(|line| line.id.clone())
        .collect();
    let current_ids: Vec<String> = projection
        .lines
        .iter()
        .map(|line| line.line_id.clone())
        .collect();

    for line_id in current_ids {
        if !desired_existing.contains(&line_id) {
            projection = state
                .facade
                .dispatch_cart_command_for_tenant(
                    &auth.tenant_id,
                    CartCommand::RemoveItem(RemoveItemPayload { line_id }),
                    Some(cart_id),
                )
                .await?;
        }
    }

    for line in req.line_items {
        projection = if let Some(line_id) = line.id {
            state
                .facade
                .dispatch_cart_command_for_tenant(
                    &auth.tenant_id,
                    CartCommand::UpdateItemQty(UpdateItemQtyPayload {
                        line_id,
                        quantity: line.quantity,
                    }),
                    Some(cart_id),
                )
                .await?
        } else {
            state
                .facade
                .dispatch_cart_command_for_tenant(
                    &auth.tenant_id,
                    CartCommand::AddItem(AddItemPayload {
                        item_id: line.item.id,
                        quantity: line.quantity,
                    }),
                    Some(cart_id),
                )
                .await?
        };
    }
    orchestrator_observability::incr("ucp_cart_update_total");
    Ok(Json(projection.into()))
}

async fn ucp_cancel_cart(
    AuthContextExtractor(auth): AuthContextExtractor,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<UcpCartResponseDto>, ApiError> {
    let cart_id = parse_ucp_cart_id(&id)?;
    let projection = state
        .facade
        .dispatch_cart_command_for_tenant(
            &auth.tenant_id,
            CartCommand::CancelCart(CancelCartPayload { cart_id }),
            None,
        )
        .await?;
    orchestrator_observability::incr("ucp_cart_cancel_total");
    Ok(Json(projection.into()))
}

/// POST /api/v1/ucp/cart/:id/fulfillment — UCP fulfillment extension (`dev.ucp.shopping.fulfillment`):
/// quote or select a shipping/pickup destination and option for a cart's line items.
async fn ucp_set_cart_fulfillment(
    AuthContextExtractor(auth): AuthContextExtractor,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<UcpFulfillmentSelectionRequestDto>,
) -> Result<Json<UcpCartResponseDto>, ApiError> {
    let cart_id = parse_ucp_cart_id(&id)?;
    let payload = SetFulfillmentSelectionPayload::try_from(req).map_err(ApiError::BadRequest)?;
    let projection = state
        .facade
        .dispatch_cart_command_for_tenant(
            &auth.tenant_id,
            CartCommand::SetFulfillmentSelection(Box::new(payload)),
            Some(cart_id),
        )
        .await?;
    orchestrator_observability::incr("ucp_cart_fulfillment_select_total");
    Ok(Json(projection.into()))
}

/// POST /api/v1/ucp/checkout/:id/fulfillment — same as the cart fulfillment endpoint; the UCP
/// fulfillment extension extends both `dev.ucp.shopping.cart` and `dev.ucp.shopping.checkout`.
async fn ucp_set_checkout_fulfillment(
    auth: AuthContextExtractor,
    state: State<AppState>,
    path: Path<String>,
    req: Json<UcpFulfillmentSelectionRequestDto>,
) -> Result<Json<UcpCartResponseDto>, ApiError> {
    let response = ucp_set_cart_fulfillment(auth, state, path, req).await?;
    orchestrator_observability::incr("ucp_checkout_fulfillment_select_total");
    Ok(response)
}

async fn ucp_catalog_search(
    AuthContextExtractor(_auth): AuthContextExtractor,
    State(state): State<AppState>,
    Json(req): Json<UcpCatalogSearchRequestDto>,
) -> Result<Json<UcpCatalogProductsResponseDto>, ApiError> {
    let products = state
        .facade
        .search_catalog_items(req.query.as_deref())
        .await?
        .into_iter()
        .map(CatalogItemDto::from)
        .collect();
    orchestrator_observability::incr("ucp_catalog_search_total");
    Ok(Json(UcpCatalogProductsResponseDto {
        ucp: UcpEnvelopeDto::for_capabilities(&["dev.ucp.shopping.catalog.search"]),
        products,
        messages: Vec::new(),
    }))
}

async fn ucp_catalog_lookup(
    AuthContextExtractor(_auth): AuthContextExtractor,
    State(state): State<AppState>,
    Json(req): Json<UcpCatalogLookupRequestDto>,
) -> Result<Json<UcpCatalogProductsResponseDto>, ApiError> {
    let found = state.facade.lookup_catalog_items(&req.ids).await?;
    let found_ids: HashSet<String> = found.iter().map(|item| item.id.clone()).collect();
    let messages = req
        .ids
        .iter()
        .filter(|id| !found_ids.contains(*id))
        .map(|id| UcpMessageDto {
            code: "not_found".to_string(),
            content: id.clone(),
        })
        .collect();
    orchestrator_observability::incr("ucp_catalog_lookup_total");
    Ok(Json(UcpCatalogProductsResponseDto {
        ucp: UcpEnvelopeDto::for_capabilities(&["dev.ucp.shopping.catalog.lookup"]),
        products: found.into_iter().map(CatalogItemDto::from).collect(),
        messages,
    }))
}

async fn ucp_catalog_product(
    AuthContextExtractor(_auth): AuthContextExtractor,
    State(state): State<AppState>,
    Json(req): Json<UcpCatalogProductRequestDto>,
) -> Result<Json<UcpCatalogProductResponseDto>, ApiError> {
    let product = state.facade.lookup_catalog_item(&req.id).await?;
    orchestrator_observability::incr("ucp_catalog_product_total");
    Ok(Json(UcpCatalogProductResponseDto {
        ucp: UcpEnvelopeDto::for_capabilities(&["dev.ucp.shopping.catalog.lookup"]),
        product: product.into(),
        messages: Vec::new(),
    }))
}

async fn ucp_get_order(
    AuthContextExtractor(auth_ctx): AuthContextExtractor,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<UcpOrderResponseDto>, ApiError> {
    let order = state
        .facade
        .get_order(&id)
        .await?
        .ok_or_else(|| ApiError::NotFound("order not found".to_string()))?;
    if auth_ctx.tenant_id != order.tenant_id {
        return Err(ApiError::Forbidden("tenant mismatch".to_string()));
    }
    orchestrator_observability::incr("ucp_order_get_total");
    Ok(Json(order.into()))
}

fn parse_ucp_cart_id(id: &str) -> Result<CartId, ApiError> {
    Uuid::from_str(id)
        .map(CartId)
        .map_err(|e| ApiError::BadRequest(e.to_string()))
}

fn a2a_version_from_headers(headers: &HeaderMap) -> Result<String, ApiError> {
    let raw = headers
        .get("A2A-Version")
        .or_else(|| headers.get("a2a-version"))
        .and_then(|v| v.to_str().ok());
    negotiate_a2a_version(raw).map_err(ApiError::BadRequest)
}

fn acp_version_from_headers(headers: &HeaderMap) -> Result<&'static str, ApiError> {
    let raw = headers
        .get("API-Version")
        .or_else(|| headers.get("api-version"))
        .and_then(|v| v.to_str().ok());
    negotiate_acp_version(raw).map_err(|e| ApiError::BadRequest(e.message()))
}

/// ACP `2026-04-17` requires a non-empty `Idempotency-Key` header on every mutating POST request;
/// missing or blank values return `400` with `code: idempotency_key_required`.
fn require_acp_idempotency_key(headers: &HeaderMap) -> Result<String, ApiError> {
    headers
        .get("Idempotency-Key")
        .or_else(|| headers.get("idempotency-key"))
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .ok_or_else(|| {
            ApiError::BadRequestWithCode(
                "Idempotency-Key header is required".to_string(),
                "idempotency_key_required".to_string(),
            )
        })
}

/// Build the PSP request. `tenant_id` comes from the caller's auth context: the
/// body's own `tenant_id` is a claim by the caller and is only accepted when it
/// agrees with it (see [`acp_delegate_payment`]).
fn delegation_request(
    req: &AcpDelegatePaymentRequest,
    tenant_id: &str,
    idempotency_key: String,
) -> PaymentDelegationRequest {
    let metadata = req
        .metadata
        .as_ref()
        .and_then(|value| value.as_object())
        .map(|map| {
            map.iter()
                .map(|(key, value)| {
                    let value = match value {
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    (key.clone(), value)
                })
                .collect()
        })
        .unwrap_or_default();
    PaymentDelegationRequest {
        tenant_id: tenant_id.to_string(),
        token: req.payment_method.token.clone(),
        method_type: req.payment_method.method_type.clone(),
        max_amount_minor: req.payment_method.amount_minor,
        idempotency_key,
        metadata,
    }
}

async fn ucp_create_checkout(
    AuthContextExtractor(auth): AuthContextExtractor,
    State(state): State<AppState>,
    Json(req): Json<UcpCartRequestDto>,
) -> Result<Json<UcpCartResponseDto>, ApiError> {
    let merchant_id = req
        .merchant_id
        .ok_or_else(|| ApiError::BadRequest("merchant_id is required".to_string()))?;
    let mut projection = state
        .facade
        .dispatch_cart_command_for_tenant(
            &auth.tenant_id,
            CartCommand::CreateCart(CreateCartPayload {
                merchant_id,
                currency: req.currency,
                tenant_id: Some(auth.tenant_id.clone()),
            }),
            None,
        )
        .await?;
    let cart_id = projection.cart_id;
    for line in req.line_items {
        projection = state
            .facade
            .dispatch_cart_command_for_tenant(
                &auth.tenant_id,
                CartCommand::AddItem(AddItemPayload {
                    item_id: line.item.id,
                    quantity: line.quantity,
                }),
                Some(cart_id),
            )
            .await?;
    }
    projection = state
        .facade
        .dispatch_cart_command_for_tenant(
            &auth.tenant_id,
            CartCommand::StartCheckout(StartCheckoutPayload {
                cart_id,
                cart_version: projection.version,
            }),
            Some(cart_id),
        )
        .await?;
    orchestrator_observability::incr("ucp_checkout_create_total");
    Ok(Json(projection.into()))
}

async fn ucp_get_checkout(
    AuthContextExtractor(auth): AuthContextExtractor,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<UcpCartResponseDto>, ApiError> {
    let cart_id = parse_ucp_cart_id(&id)?;
    let projection = state
        .facade
        .dispatch_cart_command_for_tenant(
            &auth.tenant_id,
            CartCommand::GetCart(GetCartPayload { cart_id }),
            None,
        )
        .await?;
    orchestrator_observability::incr("ucp_checkout_get_total");
    Ok(Json(projection.into()))
}

async fn ucp_update_checkout(
    auth: AuthContextExtractor,
    state: State<AppState>,
    path: Path<String>,
    req: Json<UcpCartRequestDto>,
) -> Result<Json<UcpCartResponseDto>, ApiError> {
    let response = ucp_update_cart(auth, state, path, req).await?;
    orchestrator_observability::incr("ucp_checkout_update_total");
    Ok(response)
}

async fn ucp_complete_checkout(
    AuthContextExtractor(auth_ctx): AuthContextExtractor,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<CheckoutRequestDto>,
) -> Result<Json<TransactionResultDto>, ApiError> {
    let cart_id = parse_ucp_cart_id(&id)?;
    let mut request = CheckoutRequest::try_from(req).map_err(ApiError::BadRequest)?;
    request.cart_id = cart_id;
    let result = state
        .facade
        .execute_checkout_authorized(&auth_ctx, request)
        .await?;
    orchestrator_observability::incr("ucp_checkout_complete_total");
    Ok(Json(result.into()))
}

async fn ucp_cancel_checkout(
    AuthContextExtractor(auth): AuthContextExtractor,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<UcpCartResponseDto>, ApiError> {
    let cart_id = parse_ucp_cart_id(&id)?;
    let projection = state
        .facade
        .dispatch_cart_command_for_tenant(
            &auth.tenant_id,
            CartCommand::CancelCart(CancelCartPayload { cart_id }),
            None,
        )
        .await?;
    orchestrator_observability::incr("ucp_checkout_cancel_total");
    Ok(Json(projection.into()))
}

/// POST /api/v1/ucp/checkout/:id/embedded-link — UCP "embedded" transport / link delegation.
/// Returns a short-lived, signed handoff URL for completing checkout on the merchant's hosted
/// embedded surface without leaving the embedding agent/client experience.
async fn ucp_create_embedded_checkout_link(
    AuthContextExtractor(auth): AuthContextExtractor,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<UcpEmbeddedCheckoutLinkResponseDto>, ApiError> {
    let cart_id = parse_ucp_cart_id(&id)?;
    // Verify the checkout/cart exists before minting a handoff link for it.
    state
        .facade
        .dispatch_cart_command_for_tenant(
            &auth.tenant_id,
            CartCommand::GetCart(GetCartPayload { cart_id }),
            None,
        )
        .await?;
    let link = build_embedded_checkout_link(&state.discovery_base_url, cart_id);
    orchestrator_observability::incr("ucp_checkout_embedded_link_total");
    Ok(Json(UcpEmbeddedCheckoutLinkResponseDto {
        ucp: UcpEnvelopeDto::for_capabilities(&["dev.ucp.shopping.checkout.embedded"]),
        checkout_session_id: link.checkout_session_id,
        embedded_url: link.embedded_url,
        expires_at: link.expires_at,
    }))
}

async fn ucp_list_payment_handlers(
    AuthContextExtractor(_auth): AuthContextExtractor,
) -> Result<Json<Vec<PaymentHandlerDescriptor>>, ApiError> {
    orchestrator_observability::incr("ucp_payment_handlers_list_total");
    Ok(Json(default_payment_handlers()))
}

async fn ucp_get_payment_handler(
    AuthContextExtractor(_auth): AuthContextExtractor,
    Path(id): Path<String>,
) -> Result<Json<PaymentHandlerDescriptor>, ApiError> {
    let handler = default_payment_handlers()
        .into_iter()
        .find(|h| h.id == id)
        .ok_or_else(|| ApiError::NotFound(format!("payment handler '{id}' not found")))?;
    orchestrator_observability::incr("ucp_payment_handlers_get_total");
    Ok(Json(handler))
}

async fn acp_create_session(
    AuthContextExtractor(auth): AuthContextExtractor,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<AcpCheckoutSessionCreateRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    acp_version_from_headers(&headers)?;
    require_acp_idempotency_key(&headers)?;
    let mut projection = state
        .facade
        .dispatch_cart_command_for_tenant(&auth.tenant_id, create_cart_command(&req), None)
        .await?;
    let cart_id = projection.cart_id;
    for cmd in add_item_commands(&req) {
        projection = state
            .facade
            .dispatch_cart_command_for_tenant(&auth.tenant_id, cmd, Some(cart_id))
            .await?;
    }
    orchestrator_observability::incr("acp_checkout_session_create_total");
    Ok(Json(
        serde_json::to_value(projection_to_acp_session(&projection)).unwrap(),
    ))
}

async fn acp_get_session(
    AuthContextExtractor(auth): AuthContextExtractor,
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    acp_version_from_headers(&headers)?;
    let cart_id = parse_acp_session_id(&id).map_err(ApiError::BadRequest)?;
    let projection = state
        .facade
        .dispatch_cart_command_for_tenant(&auth.tenant_id, get_cart_command(cart_id), None)
        .await?;
    orchestrator_observability::incr("acp_checkout_session_get_total");
    Ok(Json(
        serde_json::to_value(projection_to_acp_session(&projection)).unwrap(),
    ))
}

async fn acp_update_session(
    AuthContextExtractor(auth): AuthContextExtractor,
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<AcpCheckoutSessionUpdateRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    acp_version_from_headers(&headers)?;
    let cart_id = parse_acp_session_id(&id).map_err(ApiError::BadRequest)?;
    let mut projection = state
        .facade
        .dispatch_cart_command_for_tenant(&auth.tenant_id, get_cart_command(cart_id), None)
        .await?;
    let desired: HashSet<String> = req
        .line_items
        .iter()
        .filter_map(|line| line.id.clone())
        .collect();
    let current_ids: Vec<String> = projection
        .lines
        .iter()
        .map(|line| line.line_id.clone())
        .collect();
    for line_id in current_ids {
        if !desired.contains(&line_id) {
            projection = state
                .facade
                .dispatch_cart_command_for_tenant(
                    &auth.tenant_id,
                    CartCommand::RemoveItem(RemoveItemPayload { line_id }),
                    Some(cart_id),
                )
                .await?;
        }
    }
    for line in req.line_items {
        projection = if let Some(line_id) = line.id {
            state
                .facade
                .dispatch_cart_command_for_tenant(
                    &auth.tenant_id,
                    CartCommand::UpdateItemQty(UpdateItemQtyPayload {
                        line_id,
                        quantity: line.quantity,
                    }),
                    Some(cart_id),
                )
                .await?
        } else {
            state
                .facade
                .dispatch_cart_command_for_tenant(
                    &auth.tenant_id,
                    CartCommand::AddItem(AddItemPayload {
                        item_id: line.item.id,
                        quantity: line.quantity,
                    }),
                    Some(cart_id),
                )
                .await?
        };
    }
    orchestrator_observability::incr("acp_checkout_session_update_total");
    Ok(Json(
        serde_json::to_value(projection_to_acp_session(&projection)).unwrap(),
    ))
}

async fn acp_complete_session(
    AuthContextExtractor(auth_ctx): AuthContextExtractor,
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<AcpCompleteSessionRequest>,
) -> Result<Json<TransactionResultDto>, ApiError> {
    acp_version_from_headers(&headers)?;
    require_acp_idempotency_key(&headers)?;
    let cart_id = parse_acp_session_id(&id).map_err(ApiError::BadRequest)?;
    let projection = state
        .facade
        .dispatch_cart_command_for_tenant(&auth_ctx.tenant_id, get_cart_command(cart_id), None)
        .await?;
    let _ = state
        .facade
        .dispatch_cart_command_for_tenant(
            &auth_ctx.tenant_id,
            start_checkout_command(cart_id, projection.version),
            Some(cart_id),
        )
        .await?;
    let checkout =
        complete_to_checkout_request(cart_id, projection.version, &projection.currency, &req);
    let result = state
        .facade
        .execute_checkout_authorized(&auth_ctx, checkout)
        .await?;
    orchestrator_observability::incr("acp_checkout_session_complete_total");
    Ok(Json(result.into()))
}

async fn acp_cancel_session(
    AuthContextExtractor(auth): AuthContextExtractor,
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    acp_version_from_headers(&headers)?;
    require_acp_idempotency_key(&headers)?;
    let cart_id = parse_acp_session_id(&id).map_err(ApiError::BadRequest)?;
    let projection = state
        .facade
        .dispatch_cart_command_for_tenant(&auth.tenant_id, cancel_cart_command(cart_id), None)
        .await?;
    orchestrator_observability::incr("acp_checkout_session_cancel_total");
    Ok(Json(
        serde_json::to_value(projection_to_acp_session(&projection)).unwrap(),
    ))
}

/// ACP Cart Capability (added `2026-04-17`): pre-checkout basket state, decoupled from the
/// checkout session. `POST /api/v1/acp/carts` creates a cart with estimated pricing.
async fn acp_create_cart(
    AuthContextExtractor(auth): AuthContextExtractor,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<AcpCartCreateRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    acp_version_from_headers(&headers)?;
    require_acp_idempotency_key(&headers)?;
    let mut projection = state
        .facade
        .dispatch_cart_command_for_tenant(
            &auth.tenant_id,
            create_cart_command_for_acp_cart(&req),
            None,
        )
        .await?;
    let cart_id = projection.cart_id;
    for cmd in add_item_commands_for_acp_cart(&req) {
        projection = state
            .facade
            .dispatch_cart_command_for_tenant(&auth.tenant_id, cmd, Some(cart_id))
            .await?;
    }
    orchestrator_observability::incr("acp_cart_create_total");
    Ok(Json(
        serde_json::to_value(projection_to_acp_cart(&projection)).unwrap(),
    ))
}

async fn acp_get_cart(
    AuthContextExtractor(auth): AuthContextExtractor,
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    acp_version_from_headers(&headers)?;
    let cart_id = parse_acp_cart_id(&id).map_err(ApiError::BadRequest)?;
    let projection = state
        .facade
        .dispatch_cart_command_for_tenant(&auth.tenant_id, get_cart_command(cart_id), None)
        .await?;
    orchestrator_observability::incr("acp_cart_get_total");
    Ok(Json(
        serde_json::to_value(projection_to_acp_cart(&projection)).unwrap(),
    ))
}

async fn acp_update_cart_endpoint(
    AuthContextExtractor(auth): AuthContextExtractor,
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<AcpCartUpdateRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    acp_version_from_headers(&headers)?;
    let cart_id = parse_acp_cart_id(&id).map_err(ApiError::BadRequest)?;
    let mut projection = state
        .facade
        .dispatch_cart_command_for_tenant(&auth.tenant_id, get_cart_command(cart_id), None)
        .await?;
    let desired: HashSet<String> = req
        .line_items
        .iter()
        .filter_map(|line| line.id.clone())
        .collect();
    let current_ids: Vec<String> = projection
        .lines
        .iter()
        .map(|line| line.line_id.clone())
        .collect();
    for line_id in current_ids {
        if !desired.contains(&line_id) {
            projection = state
                .facade
                .dispatch_cart_command_for_tenant(
                    &auth.tenant_id,
                    CartCommand::RemoveItem(RemoveItemPayload { line_id }),
                    Some(cart_id),
                )
                .await?;
        }
    }
    for line in req.line_items {
        projection = if let Some(line_id) = line.id {
            state
                .facade
                .dispatch_cart_command_for_tenant(
                    &auth.tenant_id,
                    CartCommand::UpdateItemQty(UpdateItemQtyPayload {
                        line_id,
                        quantity: line.quantity,
                    }),
                    Some(cart_id),
                )
                .await?
        } else {
            state
                .facade
                .dispatch_cart_command_for_tenant(
                    &auth.tenant_id,
                    CartCommand::AddItem(AddItemPayload {
                        item_id: line.item.id,
                        quantity: line.quantity,
                    }),
                    Some(cart_id),
                )
                .await?
        };
    }
    orchestrator_observability::incr("acp_cart_update_total");
    Ok(Json(
        serde_json::to_value(projection_to_acp_cart(&projection)).unwrap(),
    ))
}

async fn acp_cancel_cart_endpoint(
    AuthContextExtractor(auth): AuthContextExtractor,
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    acp_version_from_headers(&headers)?;
    require_acp_idempotency_key(&headers)?;
    let cart_id = parse_acp_cart_id(&id).map_err(ApiError::BadRequest)?;
    let projection = state
        .facade
        .dispatch_cart_command_for_tenant(&auth.tenant_id, cancel_cart_command(cart_id), None)
        .await?;
    orchestrator_observability::incr("acp_cart_cancel_total");
    Ok(Json(
        serde_json::to_value(projection_to_acp_cart(&projection)).unwrap(),
    ))
}

async fn acp_delegate_payment(
    AuthContextExtractor(auth): AuthContextExtractor,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<AcpDelegatePaymentRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    acp_version_from_headers(&headers)?;
    let idempotency_key = require_acp_idempotency_key(&headers)?;
    if req.payment_method.token.trim().is_empty() {
        return Err(ApiError::BadRequest(
            "payment_method.token is required".to_string(),
        ));
    }
    // Delegation mints a spendable token. Taking the tenant from the body would
    // let any authenticated caller mint one against someone else's account.
    if !req.tenant_id.trim().is_empty() && req.tenant_id != auth.tenant_id {
        return Err(ApiError::Forbidden("tenant mismatch".to_string()));
    }
    let delegated = state
        .facade
        .delegate_payment(delegation_request(&req, &auth.tenant_id, idempotency_key))
        .await?;
    orchestrator_observability::incr("acp_delegate_payment_total");
    Ok(Json(
        serde_json::to_value(delegate_payment_response(&delegated)).unwrap(),
    ))
}

async fn execute_checkout(
    AuthContextExtractor(auth_ctx): AuthContextExtractor,
    State(state): State<AppState>,
    Json(req): Json<CheckoutRequestDto>,
) -> Result<Json<TransactionResultDto>, ApiError> {
    let request = CheckoutRequest::try_from(req).map_err(ApiError::BadRequest)?;
    let redacted = redact_checkout_request(&request);
    tracing::info!(checkout_request = ?redacted, "checkout execute");
    let result = state
        .facade
        .execute_checkout_authorized(&auth_ctx, request)
        .await?;
    Ok(Json(result.into()))
}

/// POST /api/v1/a2a/checkout — A2A envelope: { "capability": "...", "payload": { CheckoutRequest } }. Same authz and idempotency as REST.
async fn a2a_execute_checkout(
    AuthContextExtractor(auth_ctx): AuthContextExtractor,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<TransactionResultDto>, ApiError> {
    let _version = a2a_version_from_headers(&headers)?;
    let request = normalize_a2a_checkout_envelope(&body).map_err(ApiError::BadRequest)?;
    let redacted = redact_checkout_request(&request);
    tracing::info!(checkout_request = ?redacted, "a2a checkout execute");
    let result = state
        .facade
        .execute_checkout_authorized(&auth_ctx, request)
        .await?;
    Ok(Json(result.into()))
}

/// POST /api/v1/a2a/cart — A2A envelope: { "capability": "...", "payload": { "command": { "kind": "...", ... }, "cart_id": "..."? } }. Same policy as REST.
async fn a2a_dispatch_cart_command(
    AuthContextExtractor(auth): AuthContextExtractor,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<CartProjectionDto>, ApiError> {
    let _version = a2a_version_from_headers(&headers)?;
    let (cmd, cart_id) = normalize_a2a_cart_envelope(&body).map_err(ApiError::BadRequest)?;
    let projection = state
        .facade
        .dispatch_cart_command_for_tenant(&auth.tenant_id, cmd, cart_id)
        .await?;
    Ok(Json(projection.into()))
}

/// POST /api/v1/a2a/identity/link — A2A envelope:
/// `{ "capability": "dev.ucp.identity.linking", "payload": { ... } }`.
async fn a2a_link_identity(
    AuthContextExtractor(auth): AuthContextExtractor,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<IdentityLinkResultDto>, ApiError> {
    let _version = a2a_version_from_headers(&headers)?;
    let mut request = normalize_a2a_identity_link_envelope(&body).map_err(ApiError::BadRequest)?;
    // The envelope is written by the caller; a link is created for the tenant they
    // actually authenticated as.
    if !request.tenant_id.trim().is_empty() && request.tenant_id != auth.tenant_id {
        return Err(ApiError::Forbidden("tenant mismatch".to_string()));
    }
    request.tenant_id = auth.tenant_id.clone();
    let result = state.facade.link_identity(request).await?;
    Ok(Json(result.into()))
}

async fn get_order(
    AuthContextExtractor(auth_ctx): AuthContextExtractor,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<OrderDto>, ApiError> {
    let order = state
        .facade
        .get_order(&id)
        .await?
        .ok_or_else(|| ApiError::NotFound("order not found".to_string()))?;
    if auth_ctx.tenant_id != order.tenant_id {
        return Err(ApiError::Forbidden("tenant mismatch".to_string()));
    }
    Ok(Json(order.into()))
}

async fn list_orders(
    AuthContextExtractor(auth_ctx): AuthContextExtractor,
    State(state): State<AppState>,
) -> Result<Json<Vec<OrderDto>>, ApiError> {
    let orders = state.facade.list_orders(&auth_ctx.tenant_id).await?;
    Ok(Json(orders.into_iter().map(OrderDto::from).collect()))
}

/// POST /api/v1/a2a/orders -- A2A envelope for order queries.
async fn a2a_get_order(
    AuthContextExtractor(auth_ctx): AuthContextExtractor,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let _version = a2a_version_from_headers(&headers)?;
    let order_id = body
        .get("payload")
        .and_then(|p| p.get("order_id"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::BadRequest("payload.order_id is required".to_string()))?;
    let order = state
        .facade
        .get_order(order_id)
        .await?
        .ok_or_else(|| ApiError::NotFound("order not found".to_string()))?;
    if auth_ctx.tenant_id != order.tenant_id {
        return Err(ApiError::Forbidden("tenant mismatch".to_string()));
    }
    let dto: OrderDto = order.into();
    Ok(Json(serde_json::json!({
        "capability": "dev.ucp.order.query",
        "result": dto
    })))
}

async fn register_webhook(
    AuthContextExtractor(auth_ctx): AuthContextExtractor,
    State(state): State<AppState>,
    Json(req): Json<WebhookRegistrationRequestDto>,
) -> Result<Json<WebhookRegistrationDto>, ApiError> {
    let registration = orchestrator_runtime::WebhookRegistration {
        id: format!("wh_{}", uuid::Uuid::new_v4()),
        tenant_id: auth_ctx.tenant_id,
        url: req.url,
        secret: req.secret,
        event_filter: req.event_filter,
        active: true,
    };
    let dto = WebhookRegistrationDto::from(registration.clone());
    state.facade.register_webhook(registration).await?;
    Ok(Json(dto))
}

async fn list_webhooks(
    AuthContextExtractor(auth_ctx): AuthContextExtractor,
    State(state): State<AppState>,
) -> Result<Json<Vec<WebhookRegistrationDto>>, ApiError> {
    let hooks = state.facade.list_webhooks(&auth_ctx.tenant_id).await?;
    Ok(Json(
        hooks
            .into_iter()
            .map(WebhookRegistrationDto::from)
            .collect(),
    ))
}

async fn unregister_webhook(
    AuthContextExtractor(_auth): AuthContextExtractor,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<WebhookUnregisterResponseDto>, ApiError> {
    let removed = state.facade.unregister_webhook(&id).await?;
    Ok(Json(WebhookUnregisterResponseDto { removed }))
}

async fn lookup_catalog_item(
    AuthContextExtractor(_auth): AuthContextExtractor,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<CatalogItemDto>, ApiError> {
    let item = state.facade.lookup_catalog_item(&id).await?;
    Ok(Json(item.into()))
}

async fn capture_payment(
    AuthContextExtractor(auth_ctx): AuthContextExtractor,
    State(state): State<AppState>,
    Json(req): Json<PaymentLifecycleRequestDto>,
) -> Result<Json<PaymentOperationResultDto>, ApiError> {
    if auth_ctx.tenant_id != req.tenant_id {
        return Err(ApiError::Forbidden("tenant mismatch".to_string()));
    }
    let request = req.into();
    let result = state.facade.capture_payment(&request).await?;
    Ok(Json(PaymentOperationResultDto {
        success: result.success,
        reference: result.reference,
    }))
}

async fn void_payment(
    AuthContextExtractor(auth_ctx): AuthContextExtractor,
    State(state): State<AppState>,
    Json(req): Json<PaymentLifecycleRequestDto>,
) -> Result<Json<PaymentOperationResultDto>, ApiError> {
    if auth_ctx.tenant_id != req.tenant_id {
        return Err(ApiError::Forbidden("tenant mismatch".to_string()));
    }
    let request = req.into();
    let result = state.facade.void_payment(&request).await?;
    Ok(Json(PaymentOperationResultDto {
        success: result.success,
        reference: result.reference,
    }))
}

async fn refund_payment(
    AuthContextExtractor(auth_ctx): AuthContextExtractor,
    State(state): State<AppState>,
    Json(req): Json<PaymentLifecycleRequestDto>,
) -> Result<Json<PaymentOperationResultDto>, ApiError> {
    if auth_ctx.tenant_id != req.tenant_id {
        return Err(ApiError::Forbidden("tenant mismatch".to_string()));
    }
    let request = req.into();
    let result = state.facade.refund_payment(&request).await?;
    Ok(Json(PaymentOperationResultDto {
        success: result.success,
        reference: result.reference,
    }))
}

/// POST /api/v1/mcp/message -- MCP JSON-RPC message endpoint.
async fn mcp_message(
    AuthContextExtractor(auth_ctx): AuthContextExtractor,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<orchestrator_mcp::JsonRpcRequest>,
) -> Json<orchestrator_mcp::JsonRpcResponse> {
    let header_version = headers
        .get("MCP-Protocol-Version")
        .or_else(|| headers.get("mcp-protocol-version"))
        .and_then(|v| v.to_str().ok());
    let response = orchestrator_mcp::handle_mcp_request_with_version(
        &request,
        &state.facade,
        &auth_ctx,
        header_version,
    )
    .await;
    Json(response)
}

async fn accept_incoming_event(
    AuthContextExtractor(_auth): AuthContextExtractor,
    State(state): State<AppState>,
    Json(req): Json<IncomingEventRequestDto>,
) -> Result<Json<IncomingEventResponseDto>, ApiError> {
    let accepted = state
        .facade
        .accept_incoming_event_once(&req.message_id)
        .await?;
    Ok(Json(IncomingEventResponseDto { accepted }))
}

async fn process_outbox(
    AuthContextExtractor(_auth): AuthContextExtractor,
    State(state): State<AppState>,
    Json(req): Json<ProcessOutboxRequestDto>,
) -> Result<Json<serde_json::Value>, ApiError> {
    state.facade.process_outbox_once(req.max_attempts).await?;
    Ok(Json(serde_json::json!({ "processed": true })))
}

async fn list_dead_letter(
    AuthContextExtractor(_auth): AuthContextExtractor,
    State(state): State<AppState>,
) -> Result<Json<Vec<DeadLetterEntryDto>>, ApiError> {
    let entries = state.facade.list_dead_letter().await;
    Ok(Json(
        entries
            .into_iter()
            .map(|m| DeadLetterEntryDto {
                id: m.id,
                topic: m.topic,
                correlation_id: m.correlation_id,
                attempts: m.attempts,
            })
            .collect(),
    ))
}

async fn replay_dead_letter(
    AuthContextExtractor(_auth): AuthContextExtractor,
    State(state): State<AppState>,
    Json(req): Json<ReplayDeadLetterRequestDto>,
) -> Result<Json<ReplayDeadLetterResponseDto>, ApiError> {
    let replayed = state
        .facade
        .replay_from_dead_letter(&req.message_id)
        .await?;
    Ok(Json(ReplayDeadLetterResponseDto { replayed }))
}

async fn run_reconciliation(
    AuthContextExtractor(_auth): AuthContextExtractor,
    State(state): State<AppState>,
    Json(req): Json<ReconciliationRequestDto>,
) -> Result<Json<ReconciliationReportDto>, ApiError> {
    let report = state.facade.run_reconciliation(&req.transaction_ids).await;
    Ok(Json(ReconciliationReportDto {
        mismatches: report
            .mismatches
            .into_iter()
            .map(|m| PaymentMismatchDto {
                transaction_id: m.transaction_id,
                our_state: format!("{:?}", m.our_state),
                provider_state: m.provider_state.map(|s| format!("{:?}", s)),
            })
            .collect(),
    }))
}
