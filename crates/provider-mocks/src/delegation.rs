//! Mock PSP delegation and identity link providers.

use async_trait::async_trait;
use provider_contracts::{
    DelegatedPayment, DelegationError, IdentityLinkProvider, IdentityLinkProviderError,
    IdentityLinkRecord, IdentityLinkRequest, PaymentDelegationProvider, PaymentDelegationRequest,
};
use uuid::Uuid;

/// Mints a token distinct from the caller's, as a real PSP would.
#[derive(Debug, Default, Clone)]
pub struct MockPaymentDelegationProvider;

#[async_trait]
impl PaymentDelegationProvider for MockPaymentDelegationProvider {
    async fn delegate(
        &self,
        request: &PaymentDelegationRequest,
    ) -> Result<DelegatedPayment, DelegationError> {
        if request.token.trim().is_empty() {
            return Err(DelegationError::Rejected("empty token".to_string()));
        }
        Ok(DelegatedPayment {
            id: format!("dpay_{}", Uuid::new_v4()),
            token: format!("delegated_{}", Uuid::new_v4()),
            status: "delegated".to_string(),
            expires_at: None,
        })
    }
}

#[derive(Debug, Default, Clone)]
pub struct MockIdentityLinkProvider;

#[async_trait]
impl IdentityLinkProvider for MockIdentityLinkProvider {
    async fn link(
        &self,
        _request: &IdentityLinkRequest,
    ) -> Result<IdentityLinkRecord, IdentityLinkProviderError> {
        Ok(IdentityLinkRecord {
            link_id: format!("idlink_{}", Uuid::new_v4()),
            status: "linked".to_string(),
            expires_at: None,
        })
    }
}
