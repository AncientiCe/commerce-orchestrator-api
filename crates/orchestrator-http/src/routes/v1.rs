//! API v1 routes: cart, checkout, payments, events, operations.

use crate::auth::AuthContextExtractor;
use crate::dto::{
    CartCommandRequest, CartProjectionDto, CatalogItemDto, CheckoutRequestDto, DeadLetterEntryDto,
    IdentityLinkResultDto, IncomingEventRequestDto, IncomingEventResponseDto, OrderDto,
    PaymentLifecycleRequestDto, PaymentMismatchDto, PaymentOperationResultDto,
    ProcessOutboxRequestDto, ReconciliationReportDto, ReconciliationRequestDto,
    ReplayDeadLetterRequestDto, ReplayDeadLetterResponseDto, TransactionResultDto,
    WebhookRegistrationDto, WebhookRegistrationRequestDto, WebhookUnregisterResponseDto,
};
use crate::error::ApiError;
use crate::state::AppState;
use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use orchestrator_api::{
    normalize_a2a_cart_envelope, normalize_a2a_checkout_envelope,
    normalize_a2a_identity_link_envelope, redact_checkout_request,
};
use orchestrator_core::contract::{CartCommand, CartId, CheckoutRequest};
use std::str::FromStr;
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/cart/commands", post(dispatch_cart_command))
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
    AuthContextExtractor(_auth): AuthContextExtractor,
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
    let projection = state.facade.dispatch_cart_command(cmd, cart_id).await?;
    Ok(Json(projection.into()))
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
    Json(body): Json<serde_json::Value>,
) -> Result<Json<TransactionResultDto>, ApiError> {
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
    AuthContextExtractor(_auth): AuthContextExtractor,
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<CartProjectionDto>, ApiError> {
    let (cmd, cart_id) = normalize_a2a_cart_envelope(&body).map_err(ApiError::BadRequest)?;
    let projection = state.facade.dispatch_cart_command(cmd, cart_id).await?;
    Ok(Json(projection.into()))
}

/// POST /api/v1/a2a/identity/link — A2A envelope:
/// `{ "capability": "dev.ucp.identity.linking", "payload": { ... } }`.
async fn a2a_link_identity(
    AuthContextExtractor(_auth): AuthContextExtractor,
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<IdentityLinkResultDto>, ApiError> {
    let request = normalize_a2a_identity_link_envelope(&body).map_err(ApiError::BadRequest)?;
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
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, ApiError> {
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
    Json(request): Json<orchestrator_mcp::JsonRpcRequest>,
) -> Json<orchestrator_mcp::JsonRpcResponse> {
    let response = orchestrator_mcp::handle_mcp_request(&request, &state.facade, &auth_ctx).await;
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
