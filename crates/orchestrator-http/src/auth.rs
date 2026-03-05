//! Auth extractors and enforcement for API routes.

use axum::{
    async_trait,
    extract::FromRequestParts,
    http::{header::AUTHORIZATION, request::Parts},
};
use orchestrator_api::{AuthContext, AuthnResolver};
use std::sync::Arc;

use crate::error::ApiError;
use crate::state::AppState;

/// Default dev context when allow_dev_auth is true and no AuthnResolver is configured.
fn dev_auth_context() -> AuthContext {
    AuthContext {
        caller_id: "dev".to_string(),
        tenant_id: "dev".to_string(),
        scopes: vec!["checkout:execute".to_string()],
    }
}

/// Extractor that resolves the request's Bearer token to an AuthContext.
/// When authn is configured: returns 401 if Authorization is missing or invalid.
/// When allow_dev_auth is true and authn is None: returns a default dev context (dev only).
/// When allow_dev_auth is false (production): requires authn to be configured and valid token; returns 401 otherwise.
pub struct AuthContextExtractor(pub AuthContext);

#[async_trait]
impl FromRequestParts<AppState> for AuthContextExtractor {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let auth_header = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| {
                let s = s.trim();
                s.strip_prefix("Bearer ").map(str::trim)
            });

        match &state.authn {
            None => {
                if state.allow_dev_auth {
                    Ok(AuthContextExtractor(dev_auth_context()))
                } else {
                    Err(ApiError::Unauthorized)
                }
            }
            Some(resolver) => {
                let token = auth_header.ok_or(ApiError::Unauthorized)?;
                let context = resolver
                    .resolve_bearer(token)
                    .ok_or(ApiError::Unauthorized)?;
                Ok(AuthContextExtractor(context))
            }
        }
    }
}

/// Resolves a single static bearer token from environment (AUTH_BEARER_TOKEN) to a fixed context.
/// Optional AUTH_TENANT_ID and AUTH_CALLER_ID; defaults to "prod".
/// For production use: set ENV AUTH_BEARER_TOKEN to a secret and optionally AUTH_TENANT_ID / AUTH_CALLER_ID.
pub struct StaticTokenAuthnResolver {
    token: String,
    context: AuthContext,
}

impl StaticTokenAuthnResolver {
    /// Build from environment: AUTH_BEARER_TOKEN (required), AUTH_TENANT_ID, AUTH_CALLER_ID (optional).
    pub fn from_env() -> Option<Arc<Self>> {
        let token = std::env::var("AUTH_BEARER_TOKEN").ok()?;
        let token = token.trim().to_string();
        if token.is_empty() {
            return None;
        }
        let tenant_id = std::env::var("AUTH_TENANT_ID").unwrap_or_else(|_| "prod".to_string());
        let caller_id = std::env::var("AUTH_CALLER_ID").unwrap_or_else(|_| "prod".to_string());
        Some(Arc::new(Self::new(token, tenant_id, caller_id)))
    }

    /// Build from explicit token and context (e.g. from ProductionConfig).
    pub fn new(token: String, tenant_id: String, caller_id: String) -> Self {
        Self {
            token,
            context: AuthContext {
                caller_id,
                tenant_id,
                scopes: vec!["checkout:execute".to_string()],
            },
        }
    }
}

impl AuthnResolver for StaticTokenAuthnResolver {
    fn resolve_bearer(&self, token: &str) -> Option<AuthContext> {
        if token == self.token {
            Some(self.context.clone())
        } else {
            None
        }
    }
}

/// Optional auth: returns None when no resolver or no/invalid token.
pub struct OptionalAuthContext(pub Option<AuthContext>);

#[async_trait]
impl FromRequestParts<AppState> for OptionalAuthContext {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let auth_header = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| {
                let s = s.trim();
                s.strip_prefix("Bearer ").map(str::trim)
            });

        let context = state
            .authn
            .as_ref()
            .and_then(|resolver| auth_header.and_then(|t| resolver.resolve_bearer(t)));
        Ok(OptionalAuthContext(context))
    }
}
