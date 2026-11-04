//! MPP HTTP challenge/credential/receipt wire types and authenticated binding.
//! Method-specific payment proof verification belongs to the payment provider.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Sha256;

#[derive(Debug, thiserror::Error)]
#[error("invalid machine-payment message: {0}")]
pub struct MppError(pub &'static str);

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MppChallenge {
    pub id: String,
    pub realm: String,
    pub method: String,
    pub intent: String,
    pub request: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub expires: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub opaque: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub header: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub description: Option<String>,
}
/// A credential is secret payment material; Debug deliberately redacts it.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MppCredential {
    pub challenge: MppChallenge,
    pub payload: Value,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub source: Option<String>,
}
impl std::fmt::Debug for MppCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MppCredential([redacted])")
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MppReceipt {
    pub status: String,
    pub method: String,
    pub timestamp: String,
    pub reference: String,
    #[serde(rename = "challengeId")]
    pub challenge_id: String,
}

pub fn encode_json(value: &impl Serialize) -> Result<String, MppError> {
    Ok(URL_SAFE_NO_PAD
        .encode(serde_jcs::to_vec(value).map_err(|_| MppError("JSON canonicalization failed"))?))
}
fn decoded(value: &str) -> Result<Vec<u8>, MppError> {
    if value.is_empty()
        || value.len() > 65536
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(MppError("invalid base64url message"));
    }
    URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| MppError("invalid base64url message"))
}
fn canonical_json(value: &str) -> Result<(), MppError> {
    let parsed: Value =
        serde_json::from_slice(&decoded(value)?).map_err(|_| MppError("invalid JSON"))?;
    if !parsed.is_object() || encode_json(&parsed)? != value {
        return Err(MppError("payment data must be a canonical JSON object"));
    }
    Ok(())
}
fn ascii(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|b| (32..127).contains(&b) && b != b'|')
}
impl MppChallenge {
    fn binding(&self) -> Result<String, MppError> {
        if !ascii(&self.realm)
            || self.method.is_empty()
            || !self.method.bytes().all(|b| b.is_ascii_lowercase())
            || self.intent.is_empty()
            || !self
                .intent
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err(MppError("invalid challenge identity"));
        }
        canonical_json(&self.request)?;
        if let Some(opaque) = &self.opaque {
            canonical_json(opaque)?;
        }
        if self
            .header
            .as_deref()
            .is_some_and(|v| v != "Payment-Authorization")
        {
            return Err(MppError("invalid credential header"));
        }
        if let Some(expires) = &self.expires {
            chrono::DateTime::parse_from_rfc3339(expires)
                .map_err(|_| MppError("invalid challenge expiry"))?;
        }
        if self.digest.as_deref().is_some_and(|v| !ascii(v))
            || self.description.as_deref().is_some_and(|v| !ascii(v))
        {
            return Err(MppError("invalid challenge text"));
        }
        let mut values = vec![
            self.realm.as_str(),
            self.method.as_str(),
            self.intent.as_str(),
            self.request.as_str(),
            self.expires.as_deref().unwrap_or(""),
            self.digest.as_deref().unwrap_or(""),
        ];
        if let Some(header) = &self.header {
            values.push(header);
        }
        values.push(self.opaque.as_deref().unwrap_or(""));
        Ok(values.join("|"))
    }
    pub fn bind(&mut self, secret: &[u8]) -> Result<(), MppError> {
        if secret.is_empty() {
            return Err(MppError("challenge secret is required"));
        }
        let mut mac =
            Hmac::<Sha256>::new_from_slice(secret).map_err(|_| MppError("invalid binding key"))?;
        mac.update(self.binding()?.as_bytes());
        self.id = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
        Ok(())
    }
    pub fn verify_binding(&self, secret: &[u8]) -> Result<(), MppError> {
        let started = std::time::Instant::now();
        let result = self.verify_binding_inner(secret);
        orchestrator_observability::observe_operation(
            "mpp_challenge_verify",
            if result.is_ok() { "success" } else { "error" },
            started.elapsed().as_secs_f64(),
        );
        result
    }
    fn verify_binding_inner(&self, secret: &[u8]) -> Result<(), MppError> {
        if secret.is_empty() {
            return Err(MppError("challenge secret is required"));
        }
        let mut mac =
            Hmac::<Sha256>::new_from_slice(secret).map_err(|_| MppError("invalid binding key"))?;
        mac.update(self.binding()?.as_bytes());
        mac.verify_slice(&decoded(&self.id)?)
            .map_err(|_| MppError("challenge binding mismatch"))
    }
    /// Validate the original request body and expiry after checking the HMAC.
    pub fn validate_request(&self, secret: &[u8], now: i64, body: &[u8]) -> Result<(), MppError> {
        self.verify_binding(secret)?;
        if let Some(expires) = &self.expires {
            if chrono::DateTime::parse_from_rfc3339(expires)
                .map_err(|_| MppError("invalid expiry"))?
                .timestamp()
                <= now
            {
                return Err(MppError("challenge expired"));
            }
        }
        if let Some(digest) = &self.digest {
            crate::verify_content_digest(digest, body)
                .map_err(|_| MppError("request body changed"))?;
        }
        Ok(())
    }
    pub fn www_authenticate(&self) -> Result<String, MppError> {
        self.binding()?;
        if self.id.is_empty() {
            return Err(MppError("challenge id is required"));
        }
        let value = serde_json::to_value(self).map_err(|_| MppError("invalid challenge"))?;
        let fields = value.as_object().ok_or(MppError("invalid challenge"))?;
        let mut parts = Vec::new();
        for (name, value) in fields {
            let text = value.as_str().ok_or(MppError("invalid challenge field"))?;
            if !ascii(text) {
                return Err(MppError("invalid header characters"));
            }
            parts.push(format!(
                "{}=\"{}\"",
                name,
                text.replace('\\', "\\\\").replace('"', "\\\"")
            ));
        }
        Ok(format!("Payment {}", parts.join(", ")))
    }
}
impl MppCredential {
    pub fn from_header(value: &str) -> Result<Self, MppError> {
        let (scheme, data) = value
            .split_once(' ')
            .ok_or(MppError("missing Payment scheme"))?;
        if !scheme.eq_ignore_ascii_case("Payment") {
            return Err(MppError("invalid credential scheme"));
        }
        let credential: Self = serde_json::from_slice(&decoded(data.trim_start())?)
            .map_err(|_| MppError("invalid credential JSON"))?;
        if !credential.payload.is_object() {
            return Err(MppError("payment payload must be an object"));
        }
        credential.challenge.binding()?;
        if credential.challenge.id.is_empty() {
            return Err(MppError("challenge id is required"));
        }
        Ok(credential)
    }
    pub fn to_header(&self) -> Result<String, MppError> {
        if !self.payload.is_object() {
            return Err(MppError("payment payload must be an object"));
        }
        self.challenge.binding()?;
        Ok(format!("Payment {}", encode_json(self)?))
    }
}
impl MppReceipt {
    pub fn to_header(&self) -> Result<String, MppError> {
        if self.status != "success"
            || self.reference.is_empty()
            || self.challenge_id.is_empty()
            || self.method.is_empty()
            || !self.method.bytes().all(|b| b.is_ascii_lowercase())
        {
            return Err(MppError("receipt requires successful settlement"));
        }
        chrono::DateTime::parse_from_rfc3339(&self.timestamp)
            .map_err(|_| MppError("invalid settlement timestamp"))?;
        encode_json(self)
    }
}
