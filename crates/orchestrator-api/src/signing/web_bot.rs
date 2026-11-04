//! Web Bot Auth draft-ietf-webbotauth-httpsig-protocol-01 verification.
//! Directory resolution belongs to the HTTP boundary. A caller must supply only
//! the JWK Set it actually resolved from this proof's URL over authenticated TLS.
use super::*;
use serde_json::{json, Value};

/// A candidate signature and the URL from which its keys must be resolved.
#[derive(Debug, Clone)]
pub struct WebBotSignature {
    pub directory_url: String,
    label: String,
    params: String,
    selector: String,
}

/// Cryptographically verified operator identity, independent of account authority.
#[derive(Debug, Clone)]
pub struct WebBotIdentity {
    pub agent_url: String,
    pub key_thumbprint: String,
    pub expires: i64,
}

fn invalid(reason: &str) -> SignatureError {
    SignatureError::Malformed(reason.into())
}

impl WebBotSignature {
    /// Parse modern dictionary-form Signature-Agent and select compatible signatures.
    pub fn parse(input: &str, agent: &str) -> Result<Vec<Self>, SignatureError> {
        let agents = parse_dictionary(agent)?;
        let mut candidates = Vec::new();
        for (label, params) in parse_dictionary(input)? {
            let parsed = parse_signature_params(&params)?;
            let tail = params
                .split_once(')')
                .ok_or_else(|| invalid("missing signature parameters"))?
                .1;
            let tags: Vec<_> = tail
                .split(';')
                .filter_map(|p| p.strip_prefix("tag="))
                .collect();
            if tags != ["\"web-bot-auth\""] || parsed.created.is_none() || parsed.expires.is_none()
            {
                continue;
            }
            let matching: Vec<_> = agents.iter().filter(|(name, _)| name == &label).collect();
            if matching.len() != 1 {
                continue;
            }
            let value = &matching[0].1;
            let Some(value) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) else {
                continue;
            };
            let Ok(url) = url::Url::parse(value) else {
                continue;
            };
            if url.scheme() != "https"
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.fragment().is_some()
                || value.contains(['"', '\\'])
            {
                continue;
            }
            let selector = format!("\"signature-agent\";key=\"{label}\"");
            if !parsed.covered.contains(&selector) {
                continue;
            }
            if !parsed
                .covered
                .iter()
                .any(|c| c == "@authority" || c == "@target-uri")
            {
                continue;
            }
            candidates.push(Self {
                directory_url: url.to_string(),
                label,
                params,
                selector,
            });
        }
        if candidates.is_empty() {
            return Err(invalid("no usable Web Bot Auth signature"));
        }
        Ok(candidates)
    }

    /// Verify against the directory resolved for this URL. Keys are selected by
    /// RFC 7638 thumbprint, never by another directory's operator-chosen key id.
    pub fn verify(
        &self,
        components: &MessageComponents,
        signature: &str,
        jwks: &Value,
        now: i64,
    ) -> Result<WebBotIdentity, SignatureError> {
        let started = std::time::Instant::now();
        let result = self.verify_inner(components, signature, jwks, now);
        orchestrator_observability::observe_operation(
            "web_bot_auth_verify",
            if result.is_ok() { "success" } else { "error" },
            started.elapsed().as_secs_f64(),
        );
        result
    }

    fn verify_inner(
        &self,
        components: &MessageComponents,
        signature: &str,
        jwks: &Value,
        now: i64,
    ) -> Result<WebBotIdentity, SignatureError> {
        let params = parse_signature_params(&self.params)?;
        let created = params
            .created
            .ok_or_else(|| invalid("created is required"))?;
        let expires = params
            .expires
            .ok_or_else(|| invalid("expires is required"))?;
        if expires <= created || now >= expires {
            return Err(SignatureError::Expired);
        }
        let url =
            url::Url::parse(&self.directory_url).map_err(|_| invalid("invalid directory URL"))?;
        let well_known = url.path() == "/.well-known/http-message-signatures-directory";
        let mut keys = Vec::new();
        for key in jwks
            .get("keys")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid("directory must be a JWK Set"))?
        {
            if key.get("use").is_some_and(|v| v != "sig") {
                continue;
            }
            let mut eligible = true;
            for (field, is_expiry) in [("nbf", false), ("exp", true)] {
                if let Some(value) = key.get(field) {
                    let time = value
                        .as_i64()
                        .ok_or_else(|| invalid("invalid key validity time"))?;
                    if (is_expiry && now >= time) || (!is_expiry && now < time) {
                        eligible = false;
                    }
                }
            }
            if !eligible {
                continue;
            }
            let fields: &[&str] = match (
                key.get("kty").and_then(Value::as_str),
                key.get("crv").and_then(Value::as_str),
            ) {
                (Some("OKP"), Some("Ed25519")) => &["crv", "kty", "x"],
                (Some("EC"), Some("P-256")) => &["crv", "kty", "x", "y"],
                _ => continue,
            };
            let mut canonical = BTreeMap::new();
            for field in fields {
                let value = key
                    .get(*field)
                    .and_then(Value::as_str)
                    .ok_or_else(|| invalid("missing JWK coordinate"))?;
                canonical.insert(*field, value);
            }
            let thumb = URL_SAFE_NO_PAD.encode(Sha256::digest(
                serde_json::to_vec(&canonical).map_err(|_| invalid("invalid JWK"))?,
            ));
            if well_known
                && key
                    .get("kid")
                    .is_some_and(|v| v != &Value::String(thumb.clone()))
            {
                return Err(invalid("directory kid must equal key thumbprint"));
            }
            let mut key = key.clone();
            key["kid"] = Value::String(thumb);
            keys.push(key);
        }
        let verifier = VerifyingKeyring::from_jwks(&json!({"keys":keys}).to_string())
            .map_err(|_| invalid("invalid key directory"))?;
        let required = vec![self.selector.clone()];
        let kid = verifier.verify_message(
            components,
            &format!("{}={}", self.label, self.params),
            signature,
            &required,
            now,
        )?;
        let mut identifier = url;
        identifier.set_query(None);
        Ok(WebBotIdentity {
            agent_url: identifier.to_string(),
            key_thumbprint: kid,
            expires,
        })
    }
}
