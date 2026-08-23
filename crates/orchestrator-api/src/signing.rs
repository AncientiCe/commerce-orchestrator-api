//! Ed25519 signing material for UCP request/response signatures.
//!
//! The orchestrator advertises its public keys in `/.well-known/ucp` and signs
//! outbound UCP responses with the active key. Keys are supplied by the operator;
//! there is deliberately no built-in default, because a shared default key is
//! indistinguishable from no signing at all.
//!
//! Inbound agent signatures are verified against a [`VerifyingKeyring`] built
//! from the public keys the agent publishes. Both directions sign the same shape
//! of canonical string, so a signature cannot be replayed against a different
//! method, path, status, body, or moment in time.

use base64::engine::general_purpose::{STANDARD as BASE64_STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use thiserror::Error;

use crate::ucp_mapping::SigningKeyDescriptor;

/// Header carrying `kid`, `alg`, and `sig` for a UCP message.
pub const SIGNATURE_HEADER: &str = "signature";
/// Header carrying the unix-seconds moment the message was signed.
pub const TIMESTAMP_HEADER: &str = "timestamp";
/// The only signature algorithm UCP 2026-04-08 defines for these headers.
pub const SIGNATURE_ALGORITHM: &str = "ed25519";
/// How far a signature timestamp may drift from our clock, in either direction.
pub const MAX_SIGNATURE_SKEW_SECONDS: i64 = 300;

#[derive(Debug, Error)]
pub enum SigningKeyError {
    #[error("signing key id must not be empty")]
    MissingKid,
    #[error("signing key '{kid}' is not a usable Ed25519 seed: {reason}")]
    InvalidKeyMaterial { kid: String, reason: String },
    #[error("signing key id '{0}' is used more than once")]
    DuplicateKid(String),
    #[error("unknown signing key id '{0}'")]
    UnknownKid(String),
    #[error("signature is not valid base64")]
    MalformedSignature,
    #[error("signature verification failed")]
    VerificationFailed,
}

/// A signature produced by the active key, ready for a `Signature` header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UcpSignature {
    pub kid: String,
    /// Base64url (unpadded) Ed25519 signature.
    pub signature: String,
}

struct Key {
    kid: String,
    signing: SigningKey,
    verifying: VerifyingKey,
}

/// The active signing key plus any predecessors still trusted for verification.
///
/// Rotation is additive: put the new key first and keep the old one listed until
/// every counterparty has re-read discovery.
#[derive(Clone)]
pub struct SigningKeyring {
    keys: Arc<Vec<Key>>,
}

impl std::fmt::Debug for SigningKeyring {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SigningKeyring")
            .field(
                "kids",
                &self.keys.iter().map(|k| &k.kid).collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl SigningKeyring {
    /// Build a keyring from an active key and zero or more previous keys.
    ///
    /// Seeds are 32-byte Ed25519 private seeds, base64 encoded (standard or
    /// url-safe, padded or not) — the shape `openssl rand -base64 32` produces.
    pub fn from_seeds(
        active_kid: &str,
        active_seed: &str,
        previous: &[(&str, &str)],
    ) -> Result<Self, SigningKeyError> {
        let mut keys = vec![build_key(active_kid, active_seed)?];
        for (kid, seed) in previous {
            let key = build_key(kid, seed)?;
            if keys.iter().any(|existing| existing.kid == key.kid) {
                return Err(SigningKeyError::DuplicateKid(key.kid));
            }
            keys.push(key);
        }
        Ok(Self {
            keys: Arc::new(keys),
        })
    }

    /// The kid new signatures are produced with.
    pub fn active_kid(&self) -> &str {
        &self.keys[0].kid
    }

    /// Sign a payload with the active key.
    pub fn sign(&self, payload: &[u8]) -> UcpSignature {
        let key = &self.keys[0];
        let signature = key.signing.sign(payload);
        UcpSignature {
            kid: key.kid.clone(),
            signature: URL_SAFE_NO_PAD.encode(signature.to_bytes()),
        }
    }

    /// Verify a base64 signature made by one of the keys in this ring.
    pub fn verify(
        &self,
        kid: &str,
        payload: &[u8],
        signature: &str,
    ) -> Result<(), SigningKeyError> {
        let key = self
            .keys
            .iter()
            .find(|k| k.kid == kid)
            .ok_or_else(|| SigningKeyError::UnknownKid(kid.to_string()))?;
        let raw = decode_base64(signature).ok_or(SigningKeyError::MalformedSignature)?;
        let bytes: [u8; 64] = raw
            .try_into()
            .map_err(|_| SigningKeyError::MalformedSignature)?;
        key.verifying
            .verify(payload, &Signature::from_bytes(&bytes))
            .map_err(|_| SigningKeyError::VerificationFailed)
    }

    /// Public halves, active key first, as advertised in discovery.
    pub fn public_jwks(&self) -> Vec<SigningKeyDescriptor> {
        self.keys
            .iter()
            .map(|key| SigningKeyDescriptor {
                kid: key.kid.clone(),
                kty: "OKP".to_string(),
                crv: Some("Ed25519".to_string()),
                alg: "EdDSA".to_string(),
                x: Some(URL_SAFE_NO_PAD.encode(key.verifying.to_bytes())),
                use_: Some("sig".to_string()),
            })
            .collect()
    }
}

/// Why an inbound signature was not accepted. Each variant maps to a stable
/// error code so callers can tell a missing signature from a forged one.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SignatureError {
    #[error("request is missing the Signature header")]
    MissingSignature,
    #[error("request is missing the Timestamp header")]
    MissingTimestamp,
    #[error("Signature header is malformed: {0}")]
    MalformedHeader(String),
    #[error("unsupported signature algorithm '{0}'")]
    UnsupportedAlgorithm(String),
    #[error("Timestamp header must be unix seconds")]
    InvalidTimestamp,
    #[error("signature timestamp is {skew_seconds}s away from now, outside the {MAX_SIGNATURE_SKEW_SECONDS}s window")]
    StaleTimestamp { skew_seconds: i64 },
    #[error("unknown signing key id '{0}'")]
    UnknownKid(String),
    #[error("signature is not valid base64")]
    MalformedSignature,
    #[error("signature verification failed")]
    VerificationFailed,
}

impl SignatureError {
    /// Stable machine-readable code, surfaced in API error bodies.
    pub fn code(&self) -> &'static str {
        match self {
            Self::MissingSignature => "SIGNATURE_REQUIRED",
            Self::MissingTimestamp => "SIGNATURE_TIMESTAMP_REQUIRED",
            Self::MalformedHeader(_) => "SIGNATURE_MALFORMED",
            Self::UnsupportedAlgorithm(_) => "SIGNATURE_ALGORITHM_UNSUPPORTED",
            Self::InvalidTimestamp => "SIGNATURE_TIMESTAMP_INVALID",
            Self::StaleTimestamp { .. } => "SIGNATURE_TIMESTAMP_STALE",
            Self::UnknownKid(_) => "SIGNATURE_UNKNOWN_KID",
            Self::MalformedSignature => "SIGNATURE_MALFORMED",
            Self::VerificationFailed => "SIGNATURE_INVALID",
        }
    }

    /// Short label used as a metric dimension.
    pub fn metric_suffix(&self) -> &'static str {
        match self {
            Self::MissingSignature => "missing",
            Self::MissingTimestamp | Self::InvalidTimestamp => "timestamp",
            Self::MalformedHeader(_) | Self::MalformedSignature => "malformed",
            Self::UnsupportedAlgorithm(_) => "algorithm",
            Self::StaleTimestamp { .. } => "stale",
            Self::UnknownKid(_) => "unknown_kid",
            Self::VerificationFailed => "invalid",
        }
    }
}

impl From<SigningKeyError> for SignatureError {
    fn from(error: SigningKeyError) -> Self {
        match error {
            SigningKeyError::UnknownKid(kid) => Self::UnknownKid(kid),
            SigningKeyError::MalformedSignature => Self::MalformedSignature,
            _ => Self::VerificationFailed,
        }
    }
}

/// The parsed `Signature` header: `kid="...",alg="ed25519",sig="..."`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureHeader {
    pub kid: String,
    pub algorithm: String,
    /// Base64url (unpadded) Ed25519 signature.
    pub signature: String,
}

impl SignatureHeader {
    pub fn new(kid: String, signature: String) -> Self {
        Self {
            kid,
            algorithm: SIGNATURE_ALGORITHM.to_string(),
            signature,
        }
    }

    /// Parse the header, tolerating quoted or bare parameter values.
    pub fn parse(value: &str) -> Result<Self, SignatureError> {
        let mut kid = None;
        let mut algorithm = None;
        let mut signature = None;
        for part in value.split(',') {
            let (name, raw) = part.trim().split_once('=').ok_or_else(|| {
                SignatureError::MalformedHeader(format!("'{part}' is not key=value"))
            })?;
            let raw = raw.trim().trim_matches('"').to_string();
            match name.trim().to_ascii_lowercase().as_str() {
                "kid" => kid = Some(raw),
                "alg" | "algorithm" => algorithm = Some(raw),
                "sig" | "signature" => signature = Some(raw),
                _ => {}
            }
        }
        let kid = kid.ok_or_else(|| SignatureError::MalformedHeader("missing kid".to_string()))?;
        let signature =
            signature.ok_or_else(|| SignatureError::MalformedHeader("missing sig".to_string()))?;
        let algorithm = algorithm.unwrap_or_else(|| SIGNATURE_ALGORITHM.to_string());
        if !algorithm.eq_ignore_ascii_case(SIGNATURE_ALGORITHM) {
            return Err(SignatureError::UnsupportedAlgorithm(algorithm));
        }
        Ok(Self {
            kid,
            algorithm: algorithm.to_ascii_lowercase(),
            signature,
        })
    }
}

impl std::fmt::Display for SignatureHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "kid=\"{}\",alg=\"{}\",sig=\"{}\"",
            self.kid, self.algorithm, self.signature
        )
    }
}

/// Canonical bytes signed for a request: method, path, timestamp, body digest.
pub fn request_signing_base(method: &str, path: &str, timestamp: i64, body: &[u8]) -> Vec<u8> {
    format!(
        "{}\n{}\n{}\n{}",
        method.to_ascii_uppercase(),
        path,
        timestamp,
        sha256_hex(body)
    )
    .into_bytes()
}

/// Canonical bytes signed for a response: status, timestamp, body digest.
pub fn response_signing_base(status: u16, timestamp: i64, body: &[u8]) -> Vec<u8> {
    format!("{}\n{}\n{}", status, timestamp, sha256_hex(body)).into_bytes()
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Public keys of counterparties whose signed requests we accept.
///
/// Rotation is the same story as [`SigningKeyring`]: list both kids while the
/// agent switches over, then drop the retired one.
#[derive(Clone, Default)]
pub struct VerifyingKeyring {
    keys: Arc<Vec<(String, VerifyingKey)>>,
}

impl std::fmt::Debug for VerifyingKeyring {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VerifyingKeyring")
            .field(
                "kids",
                &self.keys.iter().map(|(kid, _)| kid).collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl VerifyingKeyring {
    /// Build from `(kid, base64 Ed25519 public key)` pairs, as published in a JWK `x`.
    pub fn from_public_keys(keys: &[(&str, &str)]) -> Result<Self, SigningKeyError> {
        let mut parsed: Vec<(String, VerifyingKey)> = Vec::with_capacity(keys.len());
        for (kid, encoded) in keys {
            if kid.trim().is_empty() {
                return Err(SigningKeyError::MissingKid);
            }
            if parsed.iter().any(|(existing, _)| existing == kid) {
                return Err(SigningKeyError::DuplicateKid((*kid).to_string()));
            }
            let raw =
                decode_base64(encoded).ok_or_else(|| SigningKeyError::InvalidKeyMaterial {
                    kid: (*kid).to_string(),
                    reason: "expected base64-encoded public key".to_string(),
                })?;
            let bytes: [u8; 32] =
                raw.try_into()
                    .map_err(|_| SigningKeyError::InvalidKeyMaterial {
                        kid: (*kid).to_string(),
                        reason: "expected 32 bytes of Ed25519 public key".to_string(),
                    })?;
            let key = VerifyingKey::from_bytes(&bytes).map_err(|error| {
                SigningKeyError::InvalidKeyMaterial {
                    kid: (*kid).to_string(),
                    reason: error.to_string(),
                }
            })?;
            parsed.push(((*kid).to_string(), key));
        }
        Ok(Self {
            keys: Arc::new(parsed),
        })
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Key ids this ring will accept, for diagnostics.
    pub fn kids(&self) -> Vec<&str> {
        self.keys.iter().map(|(kid, _)| kid.as_str()).collect()
    }

    /// Verify a signed request: header shape, clock skew, then the signature itself.
    pub fn verify_request(
        &self,
        method: &str,
        path: &str,
        signature_header: &str,
        timestamp_header: &str,
        body: &[u8],
        now: i64,
    ) -> Result<String, SignatureError> {
        let header = SignatureHeader::parse(signature_header)?;
        let timestamp: i64 = timestamp_header
            .trim()
            .parse()
            .map_err(|_| SignatureError::InvalidTimestamp)?;
        let skew = now - timestamp;
        if skew.abs() > MAX_SIGNATURE_SKEW_SECONDS {
            return Err(SignatureError::StaleTimestamp { skew_seconds: skew });
        }
        let key = self
            .keys
            .iter()
            .find(|(kid, _)| kid == &header.kid)
            .map(|(_, key)| key)
            .ok_or_else(|| SignatureError::UnknownKid(header.kid.clone()))?;
        let raw = decode_base64(&header.signature).ok_or(SignatureError::MalformedSignature)?;
        let bytes: [u8; 64] = raw
            .try_into()
            .map_err(|_| SignatureError::MalformedSignature)?;
        key.verify(
            &request_signing_base(method, path, timestamp, body),
            &Signature::from_bytes(&bytes),
        )
        .map_err(|_| SignatureError::VerificationFailed)?;
        Ok(header.kid)
    }
}

fn build_key(kid: &str, seed: &str) -> Result<Key, SigningKeyError> {
    if kid.trim().is_empty() {
        return Err(SigningKeyError::MissingKid);
    }
    let raw = decode_base64(seed).ok_or_else(|| SigningKeyError::InvalidKeyMaterial {
        kid: kid.to_string(),
        reason: "expected base64-encoded key material".to_string(),
    })?;
    let bytes: [u8; 32] = raw
        .try_into()
        .map_err(|_| SigningKeyError::InvalidKeyMaterial {
            kid: kid.to_string(),
            reason: "expected 32 bytes of Ed25519 seed".to_string(),
        })?;
    let signing = SigningKey::from_bytes(&bytes);
    let verifying = signing.verifying_key();
    Ok(Key {
        kid: kid.to_string(),
        signing,
        verifying,
    })
}

/// Accept whichever base64 flavour the operator's tooling produced.
fn decode_base64(value: &str) -> Option<Vec<u8>> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    URL_SAFE_NO_PAD
        .decode(trimmed.trim_end_matches('='))
        .or_else(|_| BASE64_STANDARD.decode(trimmed))
        .ok()
}
