//! HTTP adapters for PSP payment delegation and identity linking.

use async_trait::async_trait;
use provider_contracts::{
    DelegatedPayment, DelegationError, IdentityLinkProvider, IdentityLinkProviderError,
    IdentityLinkRecord, IdentityLinkRequest, PaymentDelegationProvider, PaymentDelegationRequest,
};
use std::collections::BTreeMap;

use crate::client::{build_client, post_json, ClientConfig, RequestOptions};
use crate::error::AdapterError;

#[derive(Debug, serde::Serialize)]
struct DelegateRequestBody<'a> {
    tenant_id: &'a str,
    token: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    method_type: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_amount_minor: Option<i64>,
    metadata: &'a BTreeMap<String, String>,
}

#[derive(Debug, serde::Deserialize)]
struct DelegateResponseBody {
    id: String,
    token: String,
    #[serde(default = "delegated_status")]
    status: String,
    #[serde(default)]
    expires_at: Option<String>,
}

fn delegated_status() -> String {
    "delegated".to_string()
}

/// Exchanges a caller credential for a delegated one at an external PSP.
#[derive(Clone)]
pub struct PaymentDelegationHttpAdapter {
    client: reqwest::Client,
    base_url: String,
    config: ClientConfig,
}

impl PaymentDelegationHttpAdapter {
    pub fn new(base_url: impl Into<String>, config: ClientConfig) -> Result<Self, AdapterError> {
        let client = build_client(&config)?;
        Ok(Self {
            client,
            base_url: base_url.into(),
            config,
        })
    }

    fn delegate_url(&self) -> String {
        format!("{}/delegate_payment", self.base_url.trim_end_matches('/'))
    }
}

#[async_trait]
impl PaymentDelegationProvider for PaymentDelegationHttpAdapter {
    async fn delegate(
        &self,
        request: &PaymentDelegationRequest,
    ) -> Result<DelegatedPayment, DelegationError> {
        let body = DelegateRequestBody {
            tenant_id: &request.tenant_id,
            token: &request.token,
            method_type: request.method_type.as_deref(),
            max_amount_minor: request.max_amount_minor,
            metadata: &request.metadata,
        };
        let options = RequestOptions::new().with_idempotency_key(&request.idempotency_key);
        let response = post_json(
            &self.client,
            &self.delegate_url(),
            &body,
            &self.config,
            &options,
        )
        .await
        .map_err(|e| DelegationError::Provider(e.to_string()))?;
        let parsed: DelegateResponseBody = response
            .json()
            .await
            .map_err(|e| DelegationError::Provider(format!("invalid delegation response: {e}")))?;
        Ok(DelegatedPayment {
            id: parsed.id,
            token: parsed.token,
            status: parsed.status,
            expires_at: parsed.expires_at,
        })
    }
}

#[derive(Debug, serde::Serialize)]
struct IdentityLinkRequestBody<'a> {
    tenant_id: &'a str,
    merchant_id: &'a str,
    agent_id: &'a str,
    link_token: &'a str,
}

#[derive(Debug, serde::Deserialize)]
struct IdentityLinkResponseBody {
    link_id: String,
    #[serde(default = "linked_status")]
    status: String,
    #[serde(default)]
    expires_at: Option<String>,
}

fn linked_status() -> String {
    "linked".to_string()
}

/// Binds an agent to a platform identity through an external identity system.
#[derive(Clone)]
pub struct IdentityLinkHttpAdapter {
    client: reqwest::Client,
    base_url: String,
    config: ClientConfig,
}

impl IdentityLinkHttpAdapter {
    pub fn new(base_url: impl Into<String>, config: ClientConfig) -> Result<Self, AdapterError> {
        let client = build_client(&config)?;
        Ok(Self {
            client,
            base_url: base_url.into(),
            config,
        })
    }

    fn link_url(&self) -> String {
        format!("{}/identity/link", self.base_url.trim_end_matches('/'))
    }
}

#[async_trait]
impl IdentityLinkProvider for IdentityLinkHttpAdapter {
    async fn link(
        &self,
        request: &IdentityLinkRequest,
    ) -> Result<IdentityLinkRecord, IdentityLinkProviderError> {
        let body = IdentityLinkRequestBody {
            tenant_id: &request.tenant_id,
            merchant_id: &request.merchant_id,
            agent_id: &request.agent_id,
            link_token: &request.link_token,
        };
        let response = post_json(
            &self.client,
            &self.link_url(),
            &body,
            &self.config,
            &RequestOptions::new(),
        )
        .await
        .map_err(|e| IdentityLinkProviderError::Provider(e.to_string()))?;
        let parsed: IdentityLinkResponseBody = response.json().await.map_err(|e| {
            IdentityLinkProviderError::Provider(format!("invalid identity link response: {e}"))
        })?;
        Ok(IdentityLinkRecord {
            link_id: parsed.link_id,
            status: parsed.status,
            expires_at: parsed.expires_at,
        })
    }
}
