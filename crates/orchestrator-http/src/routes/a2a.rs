//! A2A 1.0: the Agent Card and the HTTP+JSON protocol binding.
//!
//! The orchestrator's skills are synchronous commerce operations, so
//! `SendMessage` answers with an agent `Message` rather than a `Task`: the
//! request's DataPart carries a capability envelope (the same shape the
//! `/a2a/{cart,checkout,orders,identity/link}` routes accept), and the reply's
//! DataPart carries the result. No task is ever created, which is why the task
//! operations answer `TASK_NOT_FOUND`.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use orchestrator_api::AuthContext;
use serde_json::{json, Value};

use super::discovery::DISCOVERY_CACHE_CONTROL;
use super::v1::{run_a2a_cart, run_a2a_checkout, run_a2a_identity_link, run_a2a_order_query};
use crate::auth::AuthContextExtractor;
use crate::error::ApiError;
use crate::state::AppState;

/// The only A2A version this binding speaks (`Major.Minor`, per A2A 1.0).
const A2A_BINDING_VERSION: &str = "1.0";
const A2A_MEDIA_TYPE: &str = "application/a2a+json";
const A2A_ERROR_DOMAIN: &str = "a2a-protocol.org";
const IDENTITY_CAPABILITIES: &[&str] = &[
    "dev.ucp.identity.linking",
    "dev.ucp.common.identity_linking",
];

/// GET /.well-known/agent-card.json — A2A 1.0 Agent Card. Public, like UCP discovery.
pub async fn agent_card(State(state): State<AppState>) -> impl IntoResponse {
    orchestrator_observability::incr("a2a_agent_card_total");
    let base = state.discovery_base_url.trim_end_matches('/');
    let mut skills = vec![
        skill(
            "cart",
            "Cart",
            "Create and change a cart. DataPart: {\"capability\": \"dev.ucp.shopping.checkout\", \
             \"payload\": {\"command\": {\"kind\": \"create_cart\" | \"add_item\" | ...}, \"cart_id\"?}}.",
            &["commerce", "cart", "ucp"],
        ),
        skill(
            "checkout",
            "Checkout",
            "Execute checkout for a cart with a payment intent. DataPart: {\"capability\": \
             \"dev.ucp.shopping.checkout\", \"payload\": <checkout request>}.",
            &["commerce", "checkout", "payments", "ap2", "mpp"],
        ),
        skill(
            "order-lookup",
            "Order lookup",
            "Read an order. DataPart: {\"capability\": \"dev.ucp.order.query\", \"payload\": \
             {\"order_id\": \"...\"}}.",
            &["commerce", "orders"],
        ),
    ];
    if state.facade.supports_identity_linking() {
        skills.push(skill(
            "identity-linking",
            "Identity linking",
            "Link an agent identity to a shopper account. DataPart: {\"capability\": \
             \"dev.ucp.common.identity_linking\", \"payload\": <identity link request>}.",
            &["identity", "account-linking"],
        ));
    }
    let mut card = json!({
        "name": "Commerce Orchestrator",
        "description": "Cart, checkout, order and identity operations over a merchant's \
                        commerce providers, with UCP, ACP, AP2 and MPP support.",
        "supportedInterfaces": [{
            "url": format!("{base}/api/v1/a2a"),
            "protocolBinding": "HTTP+JSON",
            "protocolVersion": A2A_BINDING_VERSION,
        }],
        "version": env!("CARGO_PKG_VERSION"),
        "documentationUrl":
            "https://github.com/AncientiCe/commerce-orchestrator-api/blob/main/docs/consumption-guide.md",
        "capabilities": {
            "streaming": false,
            "pushNotifications": false,
            "extendedAgentCard": false,
        },
        "defaultInputModes": ["application/json"],
        "defaultOutputModes": ["application/json"],
        "skills": skills,
    });
    if state.authn.is_some() {
        card["securitySchemes"] = json!({
            "bearer": { "httpAuthSecurityScheme": { "scheme": "Bearer" } }
        });
        card["securityRequirements"] = json!([{ "schemes": { "bearer": { "list": [] } } }]);
    }
    (DISCOVERY_CACHE_CONTROL, axum::Json(card))
}

fn skill(id: &str, name: &str, description: &str, tags: &[&str]) -> Value {
    json!({
        "id": id,
        "name": name,
        "description": description,
        "tags": tags,
        "inputModes": ["application/json"],
        "outputModes": ["application/json"],
    })
}

/// POST /api/v1/a2a/{operation} — `message:send` is the only operation; anything
/// else (including `message:stream`, which this agent does not offer) is unsupported.
pub async fn post_operation(
    AuthContextExtractor(auth): AuthContextExtractor,
    State(state): State<AppState>,
    Path(operation): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if operation != "message:send" {
        return a2a_error(
            StatusCode::BAD_REQUEST,
            "UNSUPPORTED_OPERATION",
            &format!("'{operation}' is not supported by this agent"),
        );
    }
    let started = std::time::Instant::now();
    let response = send_message(&auth, &state, &headers, &body).await;
    let status = if response.status().is_success() {
        "success"
    } else {
        "error"
    };
    orchestrator_observability::observe_operation(
        "a2a_send_message",
        status,
        started.elapsed().as_secs_f64(),
    );
    response
}

async fn send_message(
    auth: &AuthContext,
    state: &AppState,
    headers: &HeaderMap,
    body: &[u8],
) -> Response {
    if let Some(response) = version_error(headers) {
        return response;
    }
    let request: Value = match serde_json::from_slice(body) {
        Ok(value) => value,
        Err(error) => return invalid_params(&format!("body is not JSON: {error}")),
    };
    let message = &request["message"];
    if message["messageId"].as_str().is_none_or(str::is_empty) {
        return invalid_params("message.messageId is required");
    }
    if message["role"] != "ROLE_USER" {
        return invalid_params("message.role must be ROLE_USER");
    }
    if let Some(task_id) = message["taskId"].as_str() {
        return task_not_found(task_id);
    }
    let Some(envelope) = message["parts"]
        .as_array()
        .into_iter()
        .flatten()
        .find_map(|part| part.get("data").filter(|data| data.is_object()))
    else {
        return invalid_params(
            "message.parts must include a DataPart carrying {capability, payload}",
        );
    };

    let skill = skill_for(envelope);
    orchestrator_observability::incr(&format!(
        "a2a_message_send_{}_total",
        skill.replace('-', "_")
    ));
    let result = match skill {
        "identity-linking" => run_a2a_identity_link(auth, state, envelope)
            .await
            .and_then(to_value),
        "order-lookup" => run_a2a_order_query(auth, state, envelope).await,
        "cart" => run_a2a_cart(auth, state, envelope).await.and_then(to_value),
        _ => run_a2a_checkout(auth, state, envelope)
            .await
            .and_then(to_value),
    };
    match result {
        Ok(data) => {
            let context_id = message["contextId"]
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            a2a_json(
                StatusCode::OK,
                json!({
                    "message": {
                        "messageId": uuid::Uuid::new_v4().to_string(),
                        "contextId": context_id,
                        "role": "ROLE_AGENT",
                        "parts": [{ "data": data, "mediaType": "application/json" }],
                    }
                }),
            )
        }
        Err(error) => {
            let (status, code, message) = error.parts();
            orchestrator_observability::incr("a2a_message_send_error_total");
            error_body(status, &code, "commerce-orchestrator", &message)
        }
    }
}

/// Which skill an envelope asks for, read from its capability and payload.
fn skill_for(envelope: &Value) -> &'static str {
    let capability = envelope["capability"].as_str().unwrap_or_default();
    let payload = &envelope["payload"];
    if IDENTITY_CAPABILITIES.contains(&capability) {
        "identity-linking"
    } else if payload.get("order_id").is_some() {
        "order-lookup"
    } else if payload.get("command").is_some() {
        "cart"
    } else {
        "checkout"
    }
}

fn to_value<T: serde::Serialize>(dto: T) -> Result<Value, ApiError> {
    serde_json::to_value(dto).map_err(|_| ApiError::Internal)
}

/// GET /api/v1/a2a/tasks/{id} — this agent never creates tasks.
pub async fn get_task(
    AuthContextExtractor(_auth): AuthContextExtractor,
    Path(task): Path<String>,
    headers: HeaderMap,
) -> Response {
    if let Some(response) = version_error(&headers) {
        return response;
    }
    task_not_found(&task)
}

/// POST /api/v1/a2a/tasks/{id}:cancel (and `:subscribe`) — no task exists to act on.
pub async fn post_task(
    AuthContextExtractor(_auth): AuthContextExtractor,
    Path(task): Path<String>,
    headers: HeaderMap,
) -> Response {
    if let Some(response) = version_error(&headers) {
        return response;
    }
    match task.rsplit_once(':') {
        Some((id, "cancel" | "subscribe")) => task_not_found(id),
        _ => a2a_error(
            StatusCode::BAD_REQUEST,
            "UNSUPPORTED_OPERATION",
            "unknown task operation",
        ),
    }
}

/// The `A2A-Version` service parameter, as a header or query parameter. An absent
/// value means `0.3`, which this `1.0` interface does not serve.
fn version_error(headers: &HeaderMap) -> Option<Response> {
    let version = headers
        .get("a2a-version")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or("0.3");
    if version == A2A_BINDING_VERSION {
        None
    } else {
        orchestrator_observability::incr("a2a_version_unsupported_total");
        Some(a2a_error(
            StatusCode::BAD_REQUEST,
            "VERSION_NOT_SUPPORTED",
            &format!("A2A-Version '{version}' is not supported; this interface speaks {A2A_BINDING_VERSION}"),
        ))
    }
}

fn invalid_params(message: &str) -> Response {
    a2a_error(StatusCode::BAD_REQUEST, "INVALID_PARAMS", message)
}

fn task_not_found(task_id: &str) -> Response {
    orchestrator_observability::incr("a2a_task_not_found_total");
    a2a_error(
        StatusCode::NOT_FOUND,
        "TASK_NOT_FOUND",
        &format!("task '{task_id}' not found; this agent replies with messages, not tasks"),
    )
}

fn a2a_error(status: StatusCode, reason: &str, message: &str) -> Response {
    error_body(status, reason, A2A_ERROR_DOMAIN, message)
}

/// A2A HTTP+JSON errors are `google.rpc.Status` with a `google.rpc.ErrorInfo` detail.
fn error_body(status: StatusCode, reason: &str, domain: &str, message: &str) -> Response {
    a2a_json(
        status,
        json!({
            "error": {
                "code": status.as_u16(),
                "status": rpc_status(status),
                "message": message,
                "details": [{
                    "@type": "type.googleapis.com/google.rpc.ErrorInfo",
                    "reason": reason,
                    "domain": domain,
                }],
            }
        }),
    )
}

fn rpc_status(status: StatusCode) -> &'static str {
    match status.as_u16() {
        400 => "INVALID_ARGUMENT",
        401 => "UNAUTHENTICATED",
        403 => "PERMISSION_DENIED",
        404 => "NOT_FOUND",
        409 => "ABORTED",
        422 => "FAILED_PRECONDITION",
        501 => "UNIMPLEMENTED",
        502..=504 => "UNAVAILABLE",
        _ => "INTERNAL",
    }
}

fn a2a_json(status: StatusCode, body: Value) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, A2A_MEDIA_TYPE)],
        body.to_string(),
    )
        .into_response()
}
