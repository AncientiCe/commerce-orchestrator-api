//! API v1 routes: cart, checkout, payments, events, operations.

use crate::auth::AuthContextExtractor;
use crate::dto::*;
use crate::error::ApiError;
use crate::state::AppState;
use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use orchestrator_api::redact_checkout_request;
use orchestrator_core::contract::{CartCommand, CartId, CheckoutRequest};
use std::str::FromStr;
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/cart/commands", post(dispatch_cart_command))
        .route("/checkout/execute", post(execute_checkout))
        .route("/payments/capture", post(capture_payment))
        .route("/payments/void", post(void_payment))
        .route("/payments/refund", post(refund_payment))
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
