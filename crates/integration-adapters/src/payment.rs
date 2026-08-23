//! HTTP adapter for the payment component API.

use async_trait::async_trait;
use orchestrator_core::contract::{
    CheckoutRequest, PaymentLifecycleRequest, PaymentMethodType, PaymentState,
};
use provider_contracts::{AuthResult, PaymentError, PaymentOperationResult, PaymentProvider};

use crate::client::{build_client, get_with_retry, post_json, ClientConfig, RequestOptions};
use crate::error::AdapterError;

/// Response DTO for authorize.
#[derive(Debug, serde::Deserialize)]
pub struct AuthResponse {
    pub authorized: bool,
    pub reference: String,
}

impl From<AuthResponse> for AuthResult {
    fn from(r: AuthResponse) -> Self {
        AuthResult {
            authorized: r.authorized,
            reference: r.reference,
        }
    }
}

/// Response DTO for capture/void/refund.
#[derive(Debug, serde::Deserialize)]
pub struct OperationResponse {
    pub success: bool,
    pub reference: String,
}

impl From<OperationResponse> for PaymentOperationResult {
    fn from(r: OperationResponse) -> Self {
        PaymentOperationResult {
            success: r.success,
            reference: r.reference,
        }
    }
}

/// Payment provider that calls an external payment service over HTTP.
#[derive(Clone)]
pub struct PaymentHttpAdapter {
    client: reqwest::Client,
    base_url: String,
    config: ClientConfig,
}

impl PaymentHttpAdapter {
    pub fn new(base_url: impl Into<String>, config: ClientConfig) -> Result<Self, AdapterError> {
        let client = build_client(&config)?;
        Ok(Self {
            client,
            base_url: base_url.into(),
            config,
        })
    }

    fn base(&self) -> &str {
        self.base_url.trim_end_matches('/')
    }

    fn authorize_url(&self) -> String {
        format!("{}/authorize", self.base())
    }

    fn capture_url(&self) -> String {
        format!("{}/capture", self.base())
    }

    fn void_url(&self) -> String {
        format!("{}/void", self.base())
    }

    fn refund_url(&self) -> String {
        format!("{}/refund", self.base())
    }

    fn state_url(&self, transaction_id: &str) -> String {
        format!("{}/state/{}", self.base(), transaction_id)
    }

    fn mpp_headers(&self, request: &CheckoutRequest) -> Vec<(String, String)> {
        if request.payment_intent.payment_method_type != Some(PaymentMethodType::Mpp) {
            return Vec::new();
        }
        let mut headers = vec![("X-Payment-Method".to_string(), "mpp".to_string())];
        if let Some(method) = request.payment_intent.mpp_method.as_ref() {
            headers.push(("X-Mpp-Method".to_string(), method.clone()));
        }
        if let Some(intent) = request.payment_intent.mpp_intent.as_ref() {
            headers.push(("X-Mpp-Intent".to_string(), intent.clone()));
        }
        headers
    }
}

#[async_trait]
impl PaymentProvider for PaymentHttpAdapter {
    async fn authorize(&self, request: &CheckoutRequest) -> Result<AuthResult, PaymentError> {
        let url = self.authorize_url();
        let headers = self.mpp_headers(request);
        let options = RequestOptions::new()
            .with_idempotency_key(&request.idempotency_key)
            .with_headers(&headers);
        let resp = post_json(&self.client, &url, request, &self.config, &options)
            .await
            .map_err(PaymentError::from)?;
        let body: AuthResponse = resp
            .json()
            .await
            .map_err(|e| PaymentError::AuthFailed(format!("invalid authorize response: {}", e)))?;
        Ok(body.into())
    }

    async fn capture(
        &self,
        request: &PaymentLifecycleRequest,
    ) -> Result<PaymentOperationResult, PaymentError> {
        let url = self.capture_url();
        let options = RequestOptions::new().with_idempotency_key(&request.idempotency_key);
        let resp = post_json(&self.client, &url, request, &self.config, &options)
            .await
            .map_err(PaymentError::from)?;
        let body: OperationResponse = resp
            .json()
            .await
            .map_err(|e| PaymentError::Unsupported(format!("invalid capture response: {}", e)))?;
        Ok(body.into())
    }

    async fn void(
        &self,
        request: &PaymentLifecycleRequest,
    ) -> Result<PaymentOperationResult, PaymentError> {
        let url = self.void_url();
        let options = RequestOptions::new().with_idempotency_key(&request.idempotency_key);
        let resp = post_json(&self.client, &url, request, &self.config, &options)
            .await
            .map_err(PaymentError::from)?;
        let body: OperationResponse = resp
            .json()
            .await
            .map_err(|e| PaymentError::Unsupported(format!("invalid void response: {}", e)))?;
        Ok(body.into())
    }

    async fn refund(
        &self,
        request: &PaymentLifecycleRequest,
    ) -> Result<PaymentOperationResult, PaymentError> {
        let url = self.refund_url();
        let options = RequestOptions::new().with_idempotency_key(&request.idempotency_key);
        let resp = post_json(&self.client, &url, request, &self.config, &options)
            .await
            .map_err(PaymentError::from)?;
        let body: OperationResponse = resp
            .json()
            .await
            .map_err(|e| PaymentError::Unsupported(format!("invalid refund response: {}", e)))?;
        Ok(body.into())
    }

    async fn get_payment_state(&self, transaction_id: &str) -> Option<PaymentState> {
        let url = self.state_url(transaction_id);
        let resp = get_with_retry(&self.client, &url, None::<&str>, &self.config)
            .await
            .ok()?;
        let status = resp.status();
        if status.as_u16() == 404 {
            return None;
        }
        if !status.is_success() {
            return None;
        }
        let body: PaymentStateDto = resp.json().await.ok()?;
        body.to_payment_state()
    }
}

/// Wire format for payment state (string enum).
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
struct PaymentStateDto {
    state: String,
}

impl PaymentStateDto {
    fn to_payment_state(&self) -> Option<PaymentState> {
        match self.state.as_str() {
            "authorized" => Some(PaymentState::Authorized),
            "captured" => Some(PaymentState::Captured),
            "voided" => Some(PaymentState::Voided),
            "refund_pending" | "refundpending" => Some(PaymentState::RefundPending),
            "refunded" => Some(PaymentState::Refunded),
            "reconciled" => Some(PaymentState::Reconciled),
            "failed" => Some(PaymentState::Failed),
            _ => None,
        }
    }
}
