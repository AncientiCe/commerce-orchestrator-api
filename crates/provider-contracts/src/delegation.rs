//! Payment delegation and identity linking contracts.
//!
//! Both are optional: a deployment that has not configured them advertises
//! neither capability and returns `501` rather than fabricating a result.

use async_trait::async_trait;
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DelegationError {
    #[error("payment delegation rejected: {0}")]
    Rejected(String),
    #[error("payment delegation provider failed: {0}")]
    Provider(String),
}

/// Ask the PSP to mint a scoped token an agent may spend on the caller's behalf.
#[derive(Debug, Clone)]
pub struct PaymentDelegationRequest {
    pub tenant_id: String,
    /// The caller's own payment credential, exchanged for a delegated one.
    pub token: String,
    pub method_type: Option<String>,
    /// Upper bound on what the delegated token may authorize.
    pub max_amount_minor: Option<i64>,
    pub idempotency_key: String,
    pub metadata: BTreeMap<String, String>,
}

/// The PSP's answer: a distinct token with its own identity and lifetime.
#[derive(Debug, Clone)]
pub struct DelegatedPayment {
    pub id: String,
    pub token: String,
    pub status: String,
    pub expires_at: Option<String>,
}

#[async_trait]
pub trait PaymentDelegationProvider: Send + Sync {
    async fn delegate(
        &self,
        request: &PaymentDelegationRequest,
    ) -> Result<DelegatedPayment, DelegationError>;
}

#[derive(Debug, Error)]
pub enum IdentityLinkProviderError {
    #[error("identity link rejected: {0}")]
    Rejected(String),
    #[error("identity link provider failed: {0}")]
    Provider(String),
}

/// Bind a platform identity to an agent-facing commerce context.
#[derive(Debug, Clone)]
pub struct IdentityLinkRequest {
    pub tenant_id: String,
    pub merchant_id: String,
    pub agent_id: String,
    pub link_token: String,
}

#[derive(Debug, Clone)]
pub struct IdentityLinkRecord {
    pub link_id: String,
    pub status: String,
    pub expires_at: Option<String>,
}

#[async_trait]
pub trait IdentityLinkProvider: Send + Sync {
    async fn link(
        &self,
        request: &IdentityLinkRequest,
    ) -> Result<IdentityLinkRecord, IdentityLinkProviderError>;
}
