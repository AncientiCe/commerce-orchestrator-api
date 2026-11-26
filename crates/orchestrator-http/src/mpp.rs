//! MPP: the `Payment` HTTP authentication scheme (draft-httpauth-payment) on
//! checkout.
//!
//! An MPP checkout without a credential is answered with `402` and a challenge
//! that is HMAC-bound to the realm, payment method, intent, amount, an expiry,
//! and the request body's `Content-Digest`. API authentication already uses
//! `Authorization`, so challenges select the `Payment-Authorization` header for
//! the credential. A credential that answers a challenge for this exact request
//! is used once, handed to the payment provider (which verifies the
//! method-specific proof), and answered with a `Payment-Receipt`.

use axum::{
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use orchestrator_api::mpp::{encode_json, MppChallenge, MppCredential, MppReceipt};
use orchestrator_core::contract::{CheckoutRequest, TransactionResult, TransactionStatus};
use std::time::Instant;

use crate::error::ApiError;
use crate::state::AppState;

const CREDENTIAL_HEADER: &str = "Payment-Authorization";
const PROBLEM_BASE: &str = "https://paymentauth.org/problems/";

/// Challenge secret, realm and challenge lifetime. Debug never shows the secret.
#[derive(Clone)]
pub struct MppSettings {
    secret: Vec<u8>,
    realm: String,
    ttl_secs: u64,
}

impl std::fmt::Debug for MppSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MppSettings")
            .field("realm", &self.realm)
            .field("ttl_secs", &self.ttl_secs)
            .finish_non_exhaustive()
    }
}

impl MppSettings {
    pub fn new(secret: Vec<u8>, realm: String, ttl_secs: u64) -> Result<Self, String> {
        if secret.is_empty() {
            return Err("the MPP challenge secret must not be empty".into());
        }
        if realm.is_empty() || !realm.bytes().all(|b| (33..127).contains(&b) && b != b'"') {
            return Err("the MPP realm must be printable ASCII without spaces or quotes".into());
        }
        if ttl_secs == 0 || ttl_secs > 3600 {
            return Err("the MPP challenge lifetime must be 1 to 3600 seconds".into());
        }
        Ok(Self {
            secret,
            realm,
            ttl_secs,
        })
    }

    pub fn realm(&self) -> &str {
        &self.realm
    }
}

/// What the gate decided for one checkout request.
pub(crate) enum Gate {
    /// The credential checked out and was claimed; run checkout with it.
    Paid(Box<Paid>),
    /// Answer with this response instead of running checkout.
    Respond(Response),
}

pub(crate) struct Paid {
    settings: MppSettings,
    challenge: MppChallenge,
    method: String,
    body: Vec<u8>,
}

/// The challenge this request deserves, freshly bound.
fn challenge_for(
    settings: &MppSettings,
    request: &CheckoutRequest,
    body: &[u8],
) -> Result<MppChallenge, ApiError> {
    let intent = &request.payment_intent;
    let (Some(method), Some(kind)) = (intent.mpp_method.clone(), intent.mpp_intent.clone()) else {
        return Err(ApiError::BadRequest(
            "mpp_method and mpp_intent are required for payment_method_type=mpp".into(),
        ));
    };
    let charge = serde_json::json!({
        "amount": intent.amount_minor.to_string(),
        "currency": request.currency.to_ascii_lowercase(),
    });
    let expires = jiff::Timestamp::now()
        .checked_add(jiff::SignedDuration::from_secs(settings.ttl_secs as i64))
        .map_err(|_| ApiError::Internal)?;
    let mut challenge = MppChallenge {
        id: String::new(),
        realm: settings.realm.clone(),
        method,
        intent: kind,
        request: encode_json(&charge).map_err(|_| ApiError::Internal)?,
        expires: Some(expires.strftime("%Y-%m-%dT%H:%M:%SZ").to_string()),
        digest: Some(orchestrator_api::content_digest(body)),
        // A per-challenge nonce: without it two challenges for the same request
        // in the same second would share an id, and the second would already be used.
        opaque: Some(
            encode_json(&serde_json::json!({ "nonce": uuid::Uuid::new_v4().simple().to_string() }))
                .map_err(|_| ApiError::Internal)?,
        ),
        header: Some(CREDENTIAL_HEADER.to_string()),
        description: None,
    };
    challenge
        .bind(&settings.secret)
        .map_err(|e| ApiError::BadRequestWithCode(e.to_string(), "method-unsupported".into()))?;
    Ok(challenge)
}

/// `402` with a fresh challenge and an RFC 9457 problem body.
fn payment_required(
    challenge: &MppChallenge,
    problem: &str,
    detail: &str,
) -> Result<Response, ApiError> {
    let www_authenticate = challenge
        .www_authenticate()
        .map_err(|_| ApiError::Internal)?;
    orchestrator_observability::incr("mpp_challenge_issued_total");
    if problem != "payment-required" {
        orchestrator_observability::incr(&format!(
            "mpp_credential_rejected_{}_total",
            problem.replace('-', "_")
        ));
    }
    let body = serde_json::json!({
        "type": format!("{PROBLEM_BASE}{problem}"),
        "title": "Payment Required",
        "status": 402,
        "detail": detail,
    });
    let mut response = (StatusCode::PAYMENT_REQUIRED, Json(body)).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::WWW_AUTHENTICATE,
        HeaderValue::from_str(&www_authenticate).map_err(|_| ApiError::Internal)?,
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/problem+json"),
    );
    Ok(response)
}

/// Check the `Payment-Authorization` credential on an MPP checkout. On success
/// the credential replaces `token_or_reference`, so the payment provider gets it.
pub(crate) async fn gate(
    settings: &MppSettings,
    state: &AppState,
    request: &mut CheckoutRequest,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<Gate, ApiError> {
    let started = Instant::now();
    let outcome = gate_inner(settings, state, request, headers, body).await;
    let status = match &outcome {
        Ok(Gate::Paid(_)) => "accepted",
        Ok(Gate::Respond(_)) => "challenged",
        Err(_) => "error",
    };
    orchestrator_observability::observe_operation(
        "mpp_credential_check",
        status,
        started.elapsed().as_secs_f64(),
    );
    outcome
}

async fn gate_inner(
    settings: &MppSettings,
    state: &AppState,
    request: &mut CheckoutRequest,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<Gate, ApiError> {
    let fresh = challenge_for(settings, request, body)?;
    let reject = |problem: &str, detail: &str| -> Result<Gate, ApiError> {
        payment_required(&fresh, problem, detail).map(Gate::Respond)
    };

    let mut presented = headers.get_all(CREDENTIAL_HEADER).iter();
    let Some(value) = presented.next() else {
        return reject("payment-required", "This checkout requires payment.");
    };
    if presented.next().is_some() {
        return Err(ApiError::BadRequestWithCode(
            "send exactly one Payment credential".into(),
            "bad-request".into(),
        ));
    }
    let Ok(value) = value.to_str() else {
        return reject("malformed-credential", "The credential is not valid text.");
    };
    let Ok(credential) = MppCredential::from_header(value) else {
        return reject(
            "malformed-credential",
            "The credential could not be decoded.",
        );
    };

    let echoed = &credential.challenge;
    if echoed.realm != fresh.realm
        || echoed.method != fresh.method
        || echoed.intent != fresh.intent
        || echoed.request != fresh.request
        || echoed.header != fresh.header
    {
        return reject(
            "invalid-challenge",
            "The credential answers a different challenge.",
        );
    }
    let now = jiff::Timestamp::now().as_second();
    if let Err(error) = echoed.validate_request(&settings.secret, now, body) {
        return if error.0 == "challenge expired" {
            reject("payment-expired", "The challenge has expired.")
        } else {
            reject(
                "invalid-challenge",
                "The challenge was not issued for this request.",
            )
        };
    }

    let expires_at = echoed
        .expires
        .as_deref()
        .and_then(|v| v.parse::<jiff::Timestamp>().ok())
        .map_or(now + settings.ttl_secs as i64, |t| t.as_second());
    let first_use = state
        .facade
        .claim_payment_challenge(&format!("mpp:{}", echoed.id), expires_at)
        .await?;
    if !first_use {
        return reject("invalid-challenge", "The challenge has already been used.");
    }

    request.payment_intent.token_or_reference = value
        .split_once(' ')
        .map_or("", |(_, data)| data.trim())
        .to_string();
    Ok(Gate::Paid(Box::new(Paid {
        settings: settings.clone(),
        method: echoed.method.clone(),
        challenge: echoed.clone(),
        body: body.to_vec(),
    })))
}

impl Paid {
    /// Turn the checkout outcome into the response: a receipt when it settled,
    /// a fresh challenge when the payment provider refused the proof.
    pub(crate) fn respond(
        self,
        request: &CheckoutRequest,
        result: TransactionResult,
        dto: crate::dto::TransactionResultDto,
    ) -> Result<Response, ApiError> {
        match result.status {
            TransactionStatus::AuthFailed => {
                let fresh = challenge_for(&self.settings, request, &self.body)?;
                payment_required(
                    &fresh,
                    "verification-failed",
                    "The payment proof was not accepted.",
                )
            }
            TransactionStatus::Completed => {
                let receipt = MppReceipt {
                    status: "success".into(),
                    method: self.method,
                    timestamp: jiff::Timestamp::now()
                        .strftime("%Y-%m-%dT%H:%M:%SZ")
                        .to_string(),
                    reference: result
                        .payment_reference
                        .clone()
                        .unwrap_or_else(|| result.transaction_id.clone()),
                    challenge_id: self.challenge.id,
                }
                .to_header()
                .map_err(|_| ApiError::Internal)?;
                orchestrator_observability::incr("mpp_receipt_issued_total");
                let mut response = Json(dto).into_response();
                let headers = response.headers_mut();
                headers.insert(
                    "payment-receipt",
                    HeaderValue::from_str(&receipt).map_err(|_| ApiError::Internal)?,
                );
                headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("private"));
                Ok(response)
            }
            _ => Ok(Json(dto).into_response()),
        }
    }
}
