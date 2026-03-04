//! API authn/authz guard helpers for tenant-safe orchestration access.

use orchestrator_core::contract::CheckoutRequest;

#[derive(Debug, Clone)]
pub struct AuthContext {
    pub caller_id: String,
    pub tenant_id: String,
    pub scopes: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum AuthzError {
    #[error("missing required scope: {0}")]
    MissingScope(String),
    #[error("tenant mismatch between caller and request")]
    TenantMismatch,
}

pub fn authorize_checkout(
    context: &AuthContext,
    request: &CheckoutRequest,
) -> Result<(), AuthzError> {
    if !context
        .scopes
        .iter()
        .any(|scope| scope == "checkout:execute")
    {
        return Err(AuthzError::MissingScope("checkout:execute".to_string()));
    }
    if context.tenant_id != request.tenant_id {
        return Err(AuthzError::TenantMismatch);
    }
    Ok(())
}
