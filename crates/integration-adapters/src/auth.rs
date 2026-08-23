//! Outbound authentication to downstream component APIs.
//!
//! Secrets are never rendered by `Debug`; see the manual implementations below.

use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use reqwest::{Client, RequestBuilder};
use tokio::sync::RwLock;

use crate::error::AdapterError;

/// Refresh an OAuth2 token this long before its stated expiry.
const TOKEN_EXPIRY_SKEW: Duration = Duration::from_secs(60);

/// How the client authenticates to the OAuth2 token endpoint.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OAuth2ClientAuth {
    /// HTTP Basic credentials (RFC 6749 section 2.3.1); supported by all authorization servers.
    #[default]
    Basic,
    /// `client_id` / `client_secret` in the form body.
    RequestBody,
}

impl OAuth2ClientAuth {
    /// Parse from config: `basic` (default) or `body`.
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "basic" => Ok(Self::Basic),
            "body" | "request_body" | "post" => Ok(Self::RequestBody),
            other => Err(format!(
                "unsupported oauth client auth '{}' (expected basic or body)",
                other
            )),
        }
    }
}

/// OAuth2 client-credentials grant parameters.
#[derive(Clone)]
pub struct OAuth2ClientCredentials {
    pub token_url: String,
    pub client_id: String,
    pub client_secret: String,
    pub scope: Option<String>,
    pub client_auth: OAuth2ClientAuth,
}

impl fmt::Debug for OAuth2ClientCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OAuth2ClientCredentials")
            .field("token_url", &self.token_url)
            .field("client_id", &self.client_id)
            .field("client_secret", &"<redacted>")
            .field("scope", &self.scope)
            .field("client_auth", &self.client_auth)
            .finish()
    }
}

#[derive(Debug, serde::Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    expires_in: Option<u64>,
}

struct CachedToken {
    access_token: String,
    /// `None` means the authorization server did not state an expiry; treat as long-lived.
    expires_at: Option<Instant>,
}

impl CachedToken {
    fn is_usable(&self) -> bool {
        match self.expires_at {
            None => true,
            Some(at) => Instant::now() + TOKEN_EXPIRY_SKEW < at,
        }
    }
}

/// Caching client-credentials token source. Shared across adapter clones via `Arc`.
pub struct OAuth2TokenSource {
    credentials: OAuth2ClientCredentials,
    cache: RwLock<Option<CachedToken>>,
}

impl fmt::Debug for OAuth2TokenSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OAuth2TokenSource")
            .field("credentials", &self.credentials)
            .finish_non_exhaustive()
    }
}

impl OAuth2TokenSource {
    pub fn new(credentials: OAuth2ClientCredentials) -> Self {
        Self {
            credentials,
            cache: RwLock::new(None),
        }
    }

    /// Return a cached token when still valid, otherwise mint a new one.
    pub async fn access_token(&self, client: &Client) -> Result<String, AdapterError> {
        if let Some(token) = self.cache.read().await.as_ref() {
            if token.is_usable() {
                return Ok(token.access_token.clone());
            }
        }

        let mut cache = self.cache.write().await;
        // Another task may have refreshed while we waited for the write lock.
        if let Some(token) = cache.as_ref() {
            if token.is_usable() {
                return Ok(token.access_token.clone());
            }
        }

        let minted = self.mint(client).await?;
        let access_token = minted.access_token.clone();
        *cache = Some(minted);
        Ok(access_token)
    }

    async fn mint(&self, client: &Client) -> Result<CachedToken, AdapterError> {
        let mut form: Vec<(&str, &str)> = vec![("grant_type", "client_credentials")];
        if let Some(scope) = self.credentials.scope.as_deref() {
            form.push(("scope", scope));
        }
        if self.credentials.client_auth == OAuth2ClientAuth::RequestBody {
            form.push(("client_id", &self.credentials.client_id));
            form.push(("client_secret", &self.credentials.client_secret));
        }

        let mut request = client.post(&self.credentials.token_url).form(&form);
        if self.credentials.client_auth == OAuth2ClientAuth::Basic {
            request = request.basic_auth(
                &self.credentials.client_id,
                Some(&self.credentials.client_secret),
            );
        }

        let response = request.send().await.map_err(|e| {
            AdapterError::Config(format!(
                "oauth token request to {} failed: {}",
                self.credentials.token_url, e
            ))
        })?;

        let status = response.status();
        if !status.is_success() {
            // Deliberately does not include the response body: token endpoints echo credentials.
            return Err(AdapterError::Config(format!(
                "oauth token endpoint {} returned status {}",
                self.credentials.token_url,
                status.as_u16()
            )));
        }

        let body: TokenResponse = response.json().await.map_err(|e| {
            AdapterError::Config(format!("oauth token response was not valid JSON: {}", e))
        })?;
        if body.access_token.is_empty() {
            return Err(AdapterError::Config(
                "oauth token response contained an empty access_token".to_string(),
            ));
        }

        Ok(CachedToken {
            access_token: body.access_token,
            expires_at: body
                .expires_in
                .map(|secs| Instant::now() + Duration::from_secs(secs)),
        })
    }
}

/// How the orchestrator authenticates to a downstream provider.
#[derive(Clone, Default)]
pub enum OutboundAuth {
    /// No credentials sent. Only appropriate for unauthenticated internal services.
    #[default]
    None,
    /// `Authorization: Bearer <token>`.
    Bearer { token: String },
    /// A fixed value in a caller-chosen header, e.g. `X-API-Key`.
    ApiKey { header: String, value: String },
    /// OAuth2 client-credentials grant with a shared token cache.
    OAuth2(Arc<OAuth2TokenSource>),
}

impl fmt::Debug for OutboundAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => write!(f, "None"),
            Self::Bearer { .. } => write!(f, "Bearer(<redacted>)"),
            Self::ApiKey { header, .. } => {
                write!(f, "ApiKey {{ header: {:?}, value: <redacted> }}", header)
            }
            Self::OAuth2(source) => write!(f, "OAuth2({:?})", source),
        }
    }
}

impl OutboundAuth {
    pub fn bearer(token: impl Into<String>) -> Self {
        Self::Bearer {
            token: token.into(),
        }
    }

    pub fn api_key(header: impl Into<String>, value: impl Into<String>) -> Self {
        Self::ApiKey {
            header: header.into(),
            value: value.into(),
        }
    }

    pub fn oauth2(credentials: OAuth2ClientCredentials) -> Self {
        Self::OAuth2(Arc::new(OAuth2TokenSource::new(credentials)))
    }

    pub fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }

    /// Attach credentials to an outbound request, minting an OAuth2 token if required.
    pub async fn apply(
        &self,
        request: RequestBuilder,
        client: &Client,
    ) -> Result<RequestBuilder, AdapterError> {
        match self {
            Self::None => Ok(request),
            Self::Bearer { token } => Ok(request.bearer_auth(token)),
            Self::ApiKey { header, value } => Ok(request.header(header.as_str(), value.as_str())),
            Self::OAuth2(source) => {
                let token = source.access_token(client).await?;
                Ok(request.bearer_auth(token))
            }
        }
    }
}

/// TLS material for outbound connections, including mutual TLS.
#[derive(Clone, Debug, Default)]
pub struct TlsConfig {
    /// Client certificate and private key, concatenated in PEM form.
    pub client_identity_pem: Option<Vec<u8>>,
    /// Additional trust roots in PEM form, for privately-issued provider certificates.
    pub ca_bundle_pem: Option<Vec<u8>>,
}

impl TlsConfig {
    pub fn is_empty(&self) -> bool {
        self.client_identity_pem.is_none() && self.ca_bundle_pem.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_auth_parses_known_values() {
        assert_eq!(
            OAuth2ClientAuth::parse("basic"),
            Ok(OAuth2ClientAuth::Basic)
        );
        assert_eq!(
            OAuth2ClientAuth::parse("BODY"),
            Ok(OAuth2ClientAuth::RequestBody)
        );
        assert!(OAuth2ClientAuth::parse("mtls").is_err());
    }

    #[test]
    fn cached_token_without_expiry_stays_usable() {
        let token = CachedToken {
            access_token: "t".to_string(),
            expires_at: None,
        };
        assert!(token.is_usable());
    }

    #[test]
    fn cached_token_within_skew_is_not_usable() {
        let token = CachedToken {
            access_token: "t".to_string(),
            expires_at: Some(Instant::now() + Duration::from_secs(10)),
        };
        assert!(!token.is_usable());
    }
}
