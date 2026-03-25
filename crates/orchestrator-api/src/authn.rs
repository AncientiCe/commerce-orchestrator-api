//! Authentication integration for API middleware.

use crate::authz::AuthContext;

/// Resolves a bearer token or other credential into an auth context.
/// Implement this in your API layer (e.g. HTTP middleware) and call
/// before execute_checkout_authorized.
pub trait AuthnResolver: Send + Sync {
    /// Resolve the given bearer token to an auth context, or None if invalid/expired.
    fn resolve_bearer(&self, token: &str) -> Option<AuthContext>;
}

#[derive(Debug, Clone, serde::Deserialize)]
struct JwtClaims {
    sub: String,
    tenant_id: String,
    #[serde(default)]
    scopes: Vec<String>,
    iss: Option<String>,
}

/// JWT bearer resolver (HS256) for production API auth.
pub struct JwtAuthnResolver {
    decoding_key: jsonwebtoken::DecodingKey,
    validation: jsonwebtoken::Validation,
    trusted_issuers: Vec<String>,
}

impl JwtAuthnResolver {
    pub fn new_hs256(secret: &str, trusted_issuers: Vec<String>) -> Self {
        let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::HS256);
        validation.validate_exp = true;
        Self {
            decoding_key: jsonwebtoken::DecodingKey::from_secret(secret.as_bytes()),
            validation,
            trusted_issuers,
        }
    }
}

impl AuthnResolver for JwtAuthnResolver {
    fn resolve_bearer(&self, token: &str) -> Option<AuthContext> {
        let decoded =
            jsonwebtoken::decode::<JwtClaims>(token, &self.decoding_key, &self.validation).ok()?;
        let claims = decoded.claims;
        if !self.trusted_issuers.is_empty() {
            let iss = claims.iss.as_deref()?;
            if !self
                .trusted_issuers
                .iter()
                .any(|trusted| trusted.eq_ignore_ascii_case(iss))
            {
                return None;
            }
        }
        let mut scopes = claims.scopes;
        if scopes.is_empty() {
            scopes.push("checkout:execute".to_string());
        }
        Some(AuthContext {
            caller_id: claims.sub,
            tenant_id: claims.tenant_id,
            scopes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Serialize)]
    struct TestClaims {
        sub: String,
        tenant_id: String,
        scopes: Vec<String>,
        iss: String,
        exp: usize,
    }

    #[test]
    fn jwt_resolver_accepts_valid_token() {
        let resolver = JwtAuthnResolver::new_hs256("secret", vec!["issuer.example".to_string()]);
        let claims = TestClaims {
            sub: "caller_1".to_string(),
            tenant_id: "tenant_1".to_string(),
            scopes: vec!["checkout:execute".to_string()],
            iss: "issuer.example".to_string(),
            exp: (std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time after epoch")
                .as_secs()
                + 3600) as usize,
        };
        let token = jsonwebtoken::encode(
            &jsonwebtoken::Header::default(),
            &claims,
            &jsonwebtoken::EncodingKey::from_secret("secret".as_bytes()),
        )
        .expect("encode token");
        let context = resolver.resolve_bearer(&token).expect("resolve context");
        assert_eq!(context.caller_id, "caller_1");
        assert_eq!(context.tenant_id, "tenant_1");
    }
}
