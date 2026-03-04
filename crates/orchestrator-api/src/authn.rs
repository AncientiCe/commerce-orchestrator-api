//! Authentication integration for API middleware.

use crate::authz::AuthContext;

/// Resolves a bearer token or other credential into an auth context.
/// Implement this in your API layer (e.g. HTTP middleware) and call
/// before execute_checkout_authorized.
pub trait AuthnResolver: Send + Sync {
    /// Resolve the given bearer token to an auth context, or None if invalid/expired.
    fn resolve_bearer(&self, token: &str) -> Option<AuthContext>;
}
