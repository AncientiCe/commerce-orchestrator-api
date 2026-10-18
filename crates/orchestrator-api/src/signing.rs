//! UCP message signatures: RFC 9421 HTTP Message Signatures over an RFC 9530
//! `Content-Digest`.
//!
//! The orchestrator advertises its public keys as a JWK Set in `/.well-known/ucp`
//! and signs responses with the active key. Keys are supplied by the operator;
//! there is deliberately no built-in default, because a shared default key is
//! indistinguishable from no signing at all.
//!
//! Inbound agent signatures are verified against a [`VerifyingKeyring`] built
//! from the JWKs the agent publishes. Ed25519 and ES256 are supported in both
//! directions; ES256 is the algorithm every UCP verifier must accept, so it is
//! the safer choice for an operator whose counterparties are unknown.
//!
//! The algorithm is never read from the message: it is derived from the key the
//! `keyid` names, so a signer cannot downgrade a verifier to a weaker algorithm.

use base64::engine::general_purpose::{STANDARD as BASE64_STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::Arc;
use thiserror::Error;

use crate::ucp_mapping::SigningKeyDescriptor;

/// RFC 9421 header naming the covered components and signature parameters.
pub const SIGNATURE_INPUT_HEADER: &str = "signature-input";
/// RFC 9421 header carrying the signature bytes.
pub const SIGNATURE_HEADER: &str = "signature";
/// RFC 9530 header carrying the body digest.
pub const CONTENT_DIGEST_HEADER: &str = "content-digest";
/// Label for the signatures this orchestrator produces.
pub const SIGNATURE_LABEL: &str = "sig1";
/// How far a `created` parameter may drift from our clock, in either direction.
pub const MAX_SIGNATURE_SKEW_SECONDS: i64 = 300;

/// Signature algorithm of a key, fixed by the key type rather than the message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAlgorithm {
    /// Ed25519 (RFC 8032), JOSE `EdDSA`, RFC 9421 `ed25519`.
    EdDsa,
    /// ECDSA P-256 with SHA-256, JOSE `ES256`, RFC 9421 `ecdsa-p256-sha256`.
    Es256,
}

impl KeyAlgorithm {
    /// Parse an operator-supplied algorithm name (JOSE or RFC 9421 spelling).
    pub fn parse(value: &str) -> Result<Self, SigningKeyError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "eddsa" | "ed25519" => Ok(Self::EdDsa),
            "es256" | "ecdsa-p256-sha256" => Ok(Self::Es256),
            _ => Err(SigningKeyError::UnsupportedAlgorithm(
                value.trim().to_string(),
            )),
        }
    }

    /// JOSE `alg` published in the JWK.
    pub fn jose_name(self) -> &'static str {
        match self {
            Self::EdDsa => "EdDSA",
            Self::Es256 => "ES256",
        }
    }

    /// RFC 9421 HTTP Signature Algorithms registry name.
    pub fn http_signature_name(self) -> &'static str {
        match self {
            Self::EdDsa => "ed25519",
            Self::Es256 => "ecdsa-p256-sha256",
        }
    }
}

#[derive(Debug, Error)]
pub enum SigningKeyError {
    #[error("signing key id must not be empty")]
    MissingKid,
    #[error("signing key '{kid}' is not usable: {reason}")]
    InvalidKeyMaterial { kid: String, reason: String },
    #[error("signing key id '{0}' is used more than once")]
    DuplicateKid(String),
    #[error("unknown signing key id '{0}'")]
    UnknownKid(String),
    #[error("unsupported signing algorithm '{0}'; expected EdDSA or ES256")]
    UnsupportedAlgorithm(String),
    #[error("JWK Set is not valid: {0}")]
    InvalidJwks(String),
    #[error("signature is not valid base64")]
    MalformedSignature,
    #[error("signature verification failed")]
    VerificationFailed,
}

/// A detached signature by the active key over an arbitrary payload, used where
/// the payload is not an HTTP message (embedded checkout handoff tokens).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UcpSignature {
    pub kid: String,
    /// Base64url (unpadded) signature bytes.
    pub signature: String,
}

/// Header values for a signed HTTP message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedMessage {
    /// `Signature-Input` value, e.g. `sig1=("@status");created=1;keyid="k"`.
    pub signature_input: String,
    /// `Signature` value, e.g. `sig1=:base64:`.
    pub signature: String,
}

/// Operator-supplied private key material.
#[derive(Debug, Clone, Copy)]
pub struct KeySpec<'a> {
    pub kid: &'a str,
    pub algorithm: KeyAlgorithm,
    /// Base64 32 bytes: an Ed25519 seed or a P-256 private scalar.
    pub secret: &'a str,
}

enum PrivateKey {
    Ed25519(ed25519_dalek::SigningKey),
    P256(p256::ecdsa::SigningKey),
}

#[derive(Clone)]
enum PublicKey {
    Ed25519(ed25519_dalek::VerifyingKey),
    P256(p256::ecdsa::VerifyingKey),
}

impl PrivateKey {
    fn sign(&self, payload: &[u8]) -> Vec<u8> {
        match self {
            Self::Ed25519(key) => {
                use ed25519_dalek::Signer;
                key.sign(payload).to_bytes().to_vec()
            }
            Self::P256(key) => {
                use p256::ecdsa::signature::Signer;
                let signature: p256::ecdsa::Signature = key.sign(payload);
                signature.to_bytes().to_vec()
            }
        }
    }

    fn public(&self) -> PublicKey {
        match self {
            Self::Ed25519(key) => PublicKey::Ed25519(key.verifying_key()),
            Self::P256(key) => PublicKey::P256(*key.verifying_key()),
        }
    }
}

impl PublicKey {
    fn algorithm(&self) -> KeyAlgorithm {
        match self {
            Self::Ed25519(_) => KeyAlgorithm::EdDsa,
            Self::P256(_) => KeyAlgorithm::Es256,
        }
    }

    /// Ed25519 signatures are the 64-byte RFC 8032 output; ECDSA ones are the
    /// fixed-width `r||s`, never DER.
    fn verify(&self, payload: &[u8], signature: &[u8]) -> bool {
        match self {
            Self::Ed25519(key) => {
                use ed25519_dalek::Verifier;
                let Ok(bytes) = <[u8; 64]>::try_from(signature) else {
                    return false;
                };
                key.verify(payload, &ed25519_dalek::Signature::from_bytes(&bytes))
                    .is_ok()
            }
            Self::P256(key) => {
                use p256::ecdsa::signature::Verifier;
                let Ok(signature) = p256::ecdsa::Signature::from_slice(signature) else {
                    return false;
                };
                key.verify(payload, &signature).is_ok()
            }
        }
    }

    fn jwk(&self, kid: &str) -> SigningKeyDescriptor {
        match self {
            Self::Ed25519(key) => SigningKeyDescriptor {
                kid: kid.to_string(),
                kty: "OKP".to_string(),
                crv: Some("Ed25519".to_string()),
                alg: KeyAlgorithm::EdDsa.jose_name().to_string(),
                x: Some(URL_SAFE_NO_PAD.encode(key.to_bytes())),
                y: None,
                use_: Some("sig".to_string()),
            },
            Self::P256(key) => {
                let point = key.to_sec1_point(false);
                SigningKeyDescriptor {
                    kid: kid.to_string(),
                    kty: "EC".to_string(),
                    crv: Some("P-256".to_string()),
                    alg: KeyAlgorithm::Es256.jose_name().to_string(),
                    x: point.x().map(|x| URL_SAFE_NO_PAD.encode(x)),
                    y: point.y().map(|y| URL_SAFE_NO_PAD.encode(y)),
                    use_: Some("sig".to_string()),
                }
            }
        }
    }

    fn from_jwk(jwk: &SigningKeyDescriptor) -> Result<Self, SigningKeyError> {
        let invalid = |reason: &str| SigningKeyError::InvalidKeyMaterial {
            kid: jwk.kid.clone(),
            reason: reason.to_string(),
        };
        let coordinate = |value: &Option<String>, name: &str| {
            value
                .as_deref()
                .and_then(decode_base64)
                .ok_or_else(|| invalid(&format!("missing or non-base64 '{name}'")))
        };
        match (jwk.kty.as_str(), jwk.crv.as_deref()) {
            ("OKP", Some("Ed25519")) => ed25519_public_key(&jwk.kid, &coordinate(&jwk.x, "x")?),
            ("EC", Some("P-256")) => {
                let mut sec1 = vec![0x04];
                sec1.extend(coordinate(&jwk.x, "x")?);
                sec1.extend(coordinate(&jwk.y, "y")?);
                p256::ecdsa::VerifyingKey::from_sec1_bytes(&sec1)
                    .map(Self::P256)
                    .map_err(|e| invalid(&e.to_string()))
            }
            (kty, crv) => Err(SigningKeyError::UnsupportedAlgorithm(format!(
                "kty={kty} crv={}",
                crv.unwrap_or("none")
            ))),
        }
    }
}

struct Key {
    kid: String,
    private: PrivateKey,
}

/// The active signing key plus any predecessors still published for verification.
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
    /// Build an Ed25519 keyring from an active seed and zero or more previous ones.
    ///
    /// Seeds are 32 bytes, base64 encoded (standard or url-safe, padded or not) —
    /// the shape `openssl rand -base64 32` produces.
    pub fn from_seeds(
        active_kid: &str,
        active_seed: &str,
        previous: &[(&str, &str)],
    ) -> Result<Self, SigningKeyError> {
        let ed25519 = |kid, secret| KeySpec {
            kid,
            algorithm: KeyAlgorithm::EdDsa,
            secret,
        };
        let previous: Vec<KeySpec> = previous
            .iter()
            .map(|(kid, seed)| ed25519(kid, seed))
            .collect();
        Self::from_specs(ed25519(active_kid, active_seed), &previous)
    }

    /// Build a keyring whose keys may use different algorithms.
    pub fn from_specs(active: KeySpec, previous: &[KeySpec]) -> Result<Self, SigningKeyError> {
        let mut keys = vec![build_key(active)?];
        for spec in previous {
            let key = build_key(*spec)?;
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

    /// Detached signature over a payload with the active key.
    pub fn sign(&self, payload: &[u8]) -> UcpSignature {
        let key = &self.keys[0];
        UcpSignature {
            kid: key.kid.clone(),
            signature: URL_SAFE_NO_PAD.encode(key.private.sign(payload)),
        }
    }

    /// Verify a detached base64 signature made by one of the keys in this ring.
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
        if key.private.public().verify(payload, &raw) {
            Ok(())
        } else {
            Err(SigningKeyError::VerificationFailed)
        }
    }

    /// Sign an HTTP message over `covered` components with the active key.
    pub fn sign_message(
        &self,
        components: &MessageComponents,
        covered: &[&str],
        created: i64,
    ) -> Result<SignedMessage, SignatureError> {
        self.sign_message_with_params(components, covered, created, None)
    }

    /// As [`Self::sign_message`], with an optional `expires` parameter.
    pub fn sign_message_with_params(
        &self,
        components: &MessageComponents,
        covered: &[&str],
        created: i64,
        expires: Option<i64>,
    ) -> Result<SignedMessage, SignatureError> {
        let key = &self.keys[0];
        let mut params = format!(
            "({});created={created};keyid=\"{}\"",
            covered
                .iter()
                .map(|c| format!("\"{c}\""))
                .collect::<Vec<_>>()
                .join(" "),
            key.kid
        );
        if let Some(expires) = expires {
            params.push_str(&format!(";expires={expires}"));
        }
        let covered: Vec<String> = covered.iter().map(|c| (*c).to_string()).collect();
        let base = signature_base(components, &covered, &params)?;
        let signature = BASE64_STANDARD.encode(key.private.sign(base.as_bytes()));
        Ok(SignedMessage {
            signature_input: format!("{SIGNATURE_LABEL}={params}"),
            signature: format!("{SIGNATURE_LABEL}=:{signature}:"),
        })
    }

    /// Public halves, active key first, as advertised in discovery.
    pub fn public_jwks(&self) -> Vec<SigningKeyDescriptor> {
        self.keys
            .iter()
            .map(|key| key.private.public().jwk(&key.kid))
            .collect()
    }
}

/// Why an inbound signature was not accepted. Each variant maps to a UCP error
/// code so a client can tell a missing signature from a forged one.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SignatureError {
    #[error("request is missing the Signature-Input or Signature header")]
    Missing,
    #[error("signature header is malformed: {0}")]
    Malformed(String),
    #[error("signature does not cover required component '{0}'")]
    CoverageInsufficient(String),
    #[error("signature created {skew_seconds}s away from now, outside the {MAX_SIGNATURE_SKEW_SECONDS}s window")]
    Stale { skew_seconds: i64 },
    #[error("signature has expired")]
    Expired,
    #[error("no key with keyid '{0}'")]
    KeyNotFound(String),
    #[error("unsupported signature algorithm '{0}'")]
    AlgorithmUnsupported(String),
    #[error("body does not match the sha-256 Content-Digest")]
    DigestMismatch,
    #[error("signature verification failed")]
    VerificationFailed,
}

impl SignatureError {
    /// UCP error code surfaced in API error bodies.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Missing => "signature_missing",
            Self::KeyNotFound(_) => "key_not_found",
            Self::AlgorithmUnsupported(_) => "algorithm_unsupported",
            Self::DigestMismatch => "digest_mismatch",
            Self::Malformed(_)
            | Self::CoverageInsufficient(_)
            | Self::Stale { .. }
            | Self::Expired
            | Self::VerificationFailed => "signature_invalid",
        }
    }

    /// HTTP status UCP assigns to the code.
    pub fn http_status(&self) -> u16 {
        match self {
            Self::DigestMismatch | Self::AlgorithmUnsupported(_) => 400,
            _ => 401,
        }
    }

    /// Short label used as a metric dimension.
    pub fn metric_suffix(&self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Malformed(_) => "malformed",
            Self::CoverageInsufficient(_) => "coverage",
            Self::Stale { .. } | Self::Expired => "stale",
            Self::KeyNotFound(_) => "unknown_kid",
            Self::AlgorithmUnsupported(_) => "algorithm",
            Self::DigestMismatch => "digest",
            Self::VerificationFailed => "invalid",
        }
    }
}

/// Component values of one HTTP message: derived components (`@method`,
/// `@authority`, `@path`, `@query`, `@status`) and lowercased header fields.
#[derive(Debug, Clone, Default)]
pub struct MessageComponents {
    values: BTreeMap<String, String>,
}

impl MessageComponents {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a component, normalising the name to lowercase and trimming the value.
    pub fn with(mut self, name: &str, value: &str) -> Self {
        self.insert(name, value);
        self
    }

    pub fn insert(&mut self, name: &str, value: &str) {
        self.values
            .insert(name.to_ascii_lowercase(), value.trim().to_string());
    }

    pub fn contains(&self, name: &str) -> bool {
        self.values.contains_key(name)
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }
}

/// RFC 9421 §2.5 signature base. `params` is the serialized inner list and
/// parameters exactly as they appear in `Signature-Input`.
fn signature_base(
    components: &MessageComponents,
    covered: &[String],
    params: &str,
) -> Result<String, SignatureError> {
    let mut base = String::new();
    for name in covered {
        let value = components.get(name).ok_or_else(|| {
            SignatureError::Malformed(format!(
                "covered component '{name}' is absent from the message"
            ))
        })?;
        base.push_str(&format!("\"{name}\": {value}\n"));
    }
    base.push_str(&format!("\"@signature-params\": {params}"));
    Ok(base)
}

/// RFC 9530 `Content-Digest` value for a body: `sha-256=:<base64>:`.
pub fn content_digest(body: &[u8]) -> String {
    format!("sha-256=:{}:", BASE64_STANDARD.encode(Sha256::digest(body)))
}

/// Check a `Content-Digest` header against the raw body bytes. `sha-256` is
/// mandatory; a header offering only other algorithms does not match.
pub fn verify_content_digest(header: &str, body: &[u8]) -> Result<(), SignatureError> {
    let expected = Sha256::digest(body);
    let matches = parse_dictionary(header)?
        .into_iter()
        .find(|(name, _)| name == "sha-256")
        .and_then(|(_, value)| parse_byte_sequence(&value))
        .is_some_and(|digest| digest.as_slice() == expected.as_slice());
    if matches {
        Ok(())
    } else {
        Err(SignatureError::DigestMismatch)
    }
}

/// Public keys of counterparties whose signed requests we accept.
///
/// Rotation is the same story as [`SigningKeyring`]: list both kids while the
/// agent switches over, then drop the retired one.
#[derive(Clone, Default)]
pub struct VerifyingKeyring {
    keys: Arc<Vec<(String, PublicKey)>>,
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
        let mut parsed = Vec::with_capacity(keys.len());
        for (kid, encoded) in keys {
            let raw =
                decode_base64(encoded).ok_or_else(|| SigningKeyError::InvalidKeyMaterial {
                    kid: (*kid).to_string(),
                    reason: "expected base64-encoded public key".to_string(),
                })?;
            parsed.push(((*kid).to_string(), ed25519_public_key(kid, &raw)?));
        }
        Self::from_entries(parsed)
    }

    /// Build from an RFC 7517 JWK Set (`{"keys": [...]}` or a bare array), the
    /// shape an agent publishes in its UCP profile `keys[]`.
    pub fn from_jwks(json: &str) -> Result<Self, SigningKeyError> {
        let value: serde_json::Value =
            serde_json::from_str(json).map_err(|e| SigningKeyError::InvalidJwks(e.to_string()))?;
        let keys = value.get("keys").cloned().unwrap_or(value);
        let jwks: Vec<SigningKeyDescriptor> = serde_json::from_value(keys)
            .map_err(|e| SigningKeyError::InvalidJwks(e.to_string()))?;
        let mut parsed = Vec::with_capacity(jwks.len());
        for jwk in &jwks {
            match PublicKey::from_jwk(jwk) {
                Ok(key) => parsed.push((jwk.kid.clone(), key)),
                // UCP: tolerate key types we do not recognise; only a signature
                // naming such a key fails, as key_not_found.
                Err(SigningKeyError::UnsupportedAlgorithm(_)) => {}
                Err(error) => return Err(error),
            }
        }
        Self::from_entries(parsed)
    }

    /// Combine two keyrings, refusing a kid that appears in both.
    pub fn merge(self, other: Self) -> Result<Self, SigningKeyError> {
        let mut entries: Vec<(String, PublicKey)> = self.keys.iter().cloned().collect();
        entries.extend(other.keys.iter().cloned());
        Self::from_entries(entries)
    }

    fn from_entries(entries: Vec<(String, PublicKey)>) -> Result<Self, SigningKeyError> {
        let mut seen: Vec<&str> = Vec::with_capacity(entries.len());
        for (kid, _) in &entries {
            if kid.trim().is_empty() {
                return Err(SigningKeyError::MissingKid);
            }
            if seen.contains(&kid.as_str()) {
                return Err(SigningKeyError::DuplicateKid(kid.clone()));
            }
            seen.push(kid);
        }
        Ok(Self {
            keys: Arc::new(entries),
        })
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Key ids this ring will accept, for diagnostics.
    pub fn kids(&self) -> Vec<&str> {
        self.keys.iter().map(|(kid, _)| kid.as_str()).collect()
    }

    /// Verify an RFC 9421 signed message. The message is authenticated when at
    /// least one of its signatures verifies; otherwise the first failure is
    /// returned. Returns the `keyid` that verified.
    pub fn verify_message(
        &self,
        components: &MessageComponents,
        signature_input: &str,
        signature: &str,
        required: &[String],
        now: i64,
    ) -> Result<String, SignatureError> {
        let inputs = parse_dictionary(signature_input)?;
        let signatures = parse_dictionary(signature)?;
        if inputs.is_empty() || signatures.is_empty() {
            return Err(SignatureError::Missing);
        }
        let mut first_error = None;
        for (label, raw_params) in inputs {
            let value = signatures
                .iter()
                .find(|(name, _)| *name == label)
                .map(|(_, value)| value.as_str());
            match self.verify_one(components, &raw_params, value, required, now) {
                Ok(kid) => return Ok(kid),
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
        Err(first_error.unwrap_or(SignatureError::Missing))
    }

    fn verify_one(
        &self,
        components: &MessageComponents,
        raw_params: &str,
        signature: Option<&str>,
        required: &[String],
        now: i64,
    ) -> Result<String, SignatureError> {
        let params = parse_signature_params(raw_params)?;
        if let Some(missing) = required.iter().find(|r| !params.covered.contains(r)) {
            return Err(SignatureError::CoverageInsufficient(missing.clone()));
        }
        if let Some(created) = params.created {
            let skew = now - created;
            if skew.abs() > MAX_SIGNATURE_SKEW_SECONDS {
                return Err(SignatureError::Stale { skew_seconds: skew });
            }
        }
        if params.expires.is_some_and(|expires| now > expires) {
            return Err(SignatureError::Expired);
        }
        let key = self
            .keys
            .iter()
            .find(|(kid, _)| *kid == params.keyid)
            .map(|(_, key)| key)
            .ok_or_else(|| SignatureError::KeyNotFound(params.keyid.clone()))?;
        if let Some(alg) = &params.alg {
            if alg != key.algorithm().http_signature_name() {
                return Err(SignatureError::AlgorithmUnsupported(alg.clone()));
            }
        }
        let signature = signature
            .and_then(parse_byte_sequence)
            .ok_or(SignatureError::Missing)?;
        let base = signature_base(components, &params.covered, raw_params)?;
        if key.verify(base.as_bytes(), &signature) {
            Ok(params.keyid)
        } else {
            Err(SignatureError::VerificationFailed)
        }
    }
}

struct SignatureParams {
    covered: Vec<String>,
    created: Option<i64>,
    expires: Option<i64>,
    keyid: String,
    alg: Option<String>,
}

/// Parse `("@method" "@path");created=1;keyid="k"` — the subset of RFC 8941
/// inner lists RFC 9421 uses. Component parameters (`;sf`, `;key=`) are refused
/// rather than silently ignored, because ignoring them would change the base.
fn parse_signature_params(raw: &str) -> Result<SignatureParams, SignatureError> {
    let malformed = |reason: &str| SignatureError::Malformed(reason.to_string());
    let rest = raw
        .trim()
        .strip_prefix('(')
        .ok_or_else(|| malformed("Signature-Input member must be an inner list"))?;
    let close = rest
        .find(')')
        .ok_or_else(|| malformed("unterminated inner list"))?;
    let (list, params) = (&rest[..close], &rest[close + 1..]);
    let mut covered = Vec::new();
    for item in list.split_whitespace() {
        let name = item
            .strip_prefix('"')
            .and_then(|i| i.strip_suffix('"'))
            .ok_or_else(|| {
                malformed("covered components must be quoted strings without parameters")
            })?;
        covered.push(name.to_string());
    }
    let mut parsed = SignatureParams {
        covered,
        created: None,
        expires: None,
        keyid: String::new(),
        alg: None,
    };
    for param in params.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        let (name, value) = param.split_once('=').unwrap_or((param, "?1"));
        let string = || {
            value
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .map(str::to_string)
                .ok_or_else(|| malformed(&format!("'{name}' must be a string")))
        };
        let integer = || {
            value
                .parse::<i64>()
                .map_err(|_| malformed(&format!("'{name}' must be an integer")))
        };
        match name {
            "created" => parsed.created = Some(integer()?),
            "expires" => parsed.expires = Some(integer()?),
            "keyid" => parsed.keyid = string()?,
            "alg" => parsed.alg = Some(string()?),
            // nonce and tag are application-defined; they stay in the base.
            _ => {}
        }
    }
    if parsed.keyid.is_empty() {
        return Err(malformed("keyid parameter is required"));
    }
    Ok(parsed)
}

/// Split an RFC 8941 dictionary into `(key, raw member value)` pairs, honouring
/// quoted strings, byte sequences and inner lists so their commas do not split.
fn parse_dictionary(header: &str) -> Result<Vec<(String, String)>, SignatureError> {
    let mut members = Vec::new();
    let mut current = String::new();
    let (mut in_string, mut in_bytes, mut depth) = (false, false, 0u32);
    let mut escaped = false;
    for ch in header.chars() {
        match ch {
            _ if escaped => escaped = false,
            '\\' if in_string => escaped = true,
            '"' if !in_bytes => in_string = !in_string,
            ':' if !in_string => in_bytes = !in_bytes,
            '(' if !in_string && !in_bytes => depth += 1,
            ')' if !in_string && !in_bytes => depth = depth.saturating_sub(1),
            ',' if !in_string && !in_bytes && depth == 0 => {
                members.push(std::mem::take(&mut current));
                continue;
            }
            _ => {}
        }
        current.push(ch);
    }
    if in_string || in_bytes || depth != 0 {
        return Err(SignatureError::Malformed(
            "unbalanced structured field".to_string(),
        ));
    }
    members.push(current);
    members
        .into_iter()
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty())
        .map(|member| {
            let (key, value) = member
                .split_once('=')
                .ok_or_else(|| SignatureError::Malformed(format!("'{member}' is not key=value")))?;
            Ok((key.trim().to_string(), value.trim().to_string()))
        })
        .collect()
}

/// Decode an RFC 8941 byte sequence `:base64:` (parameters, if any, ignored).
fn parse_byte_sequence(value: &str) -> Option<Vec<u8>> {
    let inner = value.trim().strip_prefix(':')?;
    let end = inner.find(':')?;
    BASE64_STANDARD.decode(&inner[..end]).ok()
}

fn ed25519_public_key(kid: &str, raw: &[u8]) -> Result<PublicKey, SigningKeyError> {
    let invalid = |reason: String| SigningKeyError::InvalidKeyMaterial {
        kid: kid.to_string(),
        reason,
    };
    let bytes: [u8; 32] = raw
        .try_into()
        .map_err(|_| invalid("expected 32 bytes of Ed25519 public key".to_string()))?;
    ed25519_dalek::VerifyingKey::from_bytes(&bytes)
        .map(PublicKey::Ed25519)
        .map_err(|e| invalid(e.to_string()))
}

fn build_key(spec: KeySpec) -> Result<Key, SigningKeyError> {
    if spec.kid.trim().is_empty() {
        return Err(SigningKeyError::MissingKid);
    }
    let invalid = |reason: String| SigningKeyError::InvalidKeyMaterial {
        kid: spec.kid.to_string(),
        reason,
    };
    let raw = decode_base64(spec.secret)
        .ok_or_else(|| invalid("expected base64-encoded key material".to_string()))?;
    let bytes: [u8; 32] = raw
        .try_into()
        .map_err(|_| invalid("expected 32 bytes of private key material".to_string()))?;
    let private = match spec.algorithm {
        KeyAlgorithm::EdDsa => PrivateKey::Ed25519(ed25519_dalek::SigningKey::from_bytes(&bytes)),
        KeyAlgorithm::Es256 => p256::ecdsa::SigningKey::from_slice(&bytes)
            .map(PrivateKey::P256)
            .map_err(|e| invalid(format!("not a valid P-256 private scalar: {e}")))?,
    };
    Ok(Key {
        kid: spec.kid.to_string(),
        private,
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
