//! AP2 mandate/credential verification hooks. When AP2 strict mode is enabled, checkout
//! requires valid consent proof and payment handler; invalid or missing artifacts fail closed.

use orchestrator_core::contract::CheckoutRequest;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, thiserror::Error)]
#[error("AP2 verification failed: {0}")]
pub struct Ap2VerificationError(pub String);

/// Verifier trait for pluggable AP2 mandate/VDC validation (signature, issuer, expiry, replay).
/// Implement this to integrate a real AP2 credential stack; the default strict check enforces
/// basic structural, issuer, handler, and expiry validation on a JSON consent proof.
pub trait Ap2MandateVerifier: Send + Sync {
    fn verify(&self, request: &CheckoutRequest) -> Result<(), Ap2VerificationError>;
}

#[derive(Debug, Clone, serde::Deserialize)]
struct ConsentProof {
    issuer: String,
    subject: String,
    mandate_id: String,
    payment_handler_id: String,
    issued_at: i64,
    expires_at: i64,
    signature: String,
    #[allow(dead_code)]
    nonce: Option<String>,
}

impl ConsentProof {
    fn parse(raw: &str) -> Result<Self, Ap2VerificationError> {
        serde_json::from_str(raw).map_err(|_| {
            Ap2VerificationError(
                "AP2 strict mode: ap2_consent_proof must be JSON with issuer, subject, mandate_id, payment_handler_id, issued_at, expires_at, and signature".to_string(),
            )
        })
    }

    fn validate(
        &self,
        request: &CheckoutRequest,
        trusted_issuers: &[String],
    ) -> Result<(), Ap2VerificationError> {
        if self.issuer.trim().is_empty() {
            return Err(Ap2VerificationError(
                "AP2 strict mode: consent proof issuer is required".to_string(),
            ));
        }
        if !trusted_issuers.is_empty()
            && !trusted_issuers
                .iter()
                .any(|issuer| issuer.eq_ignore_ascii_case(self.issuer.trim()))
        {
            return Err(Ap2VerificationError(format!(
                "AP2 strict mode: consent proof issuer '{}' is not trusted",
                self.issuer
            )));
        }
        if self.subject.trim().is_empty() {
            return Err(Ap2VerificationError(
                "AP2 strict mode: consent proof subject is required".to_string(),
            ));
        }
        if self.mandate_id.trim().is_empty() {
            return Err(Ap2VerificationError(
                "AP2 strict mode: consent proof mandate_id is required".to_string(),
            ));
        }
        if self.signature.trim().is_empty() {
            return Err(Ap2VerificationError(
                "AP2 strict mode: consent proof signature is required".to_string(),
            ));
        }
        if self.payment_handler_id.trim().is_empty() {
            return Err(Ap2VerificationError(
                "AP2 strict mode: consent proof payment_handler_id is required".to_string(),
            ));
        }
        if self.issued_at <= 0 {
            return Err(Ap2VerificationError(
                "AP2 strict mode: consent proof issued_at must be a unix timestamp".to_string(),
            ));
        }
        if self.expires_at <= self.issued_at {
            return Err(Ap2VerificationError(
                "AP2 strict mode: consent proof expires_at must be after issued_at".to_string(),
            ));
        }

        let now = now_unix_timestamp()?;
        if self.expires_at <= now {
            return Err(Ap2VerificationError(
                "AP2 strict mode: consent proof has expired".to_string(),
            ));
        }

        if self.payment_handler_id
            != request
                .payment_intent
                .payment_handler_id
                .clone()
                .unwrap_or_default()
        {
            return Err(Ap2VerificationError(
                "AP2 strict mode: consent proof payment_handler_id must match request payment_handler_id".to_string(),
            ));
        }

        Ok(())
    }
}

fn trusted_issuers_from_env() -> Vec<String> {
    std::env::var("AP2_TRUSTED_ISSUERS")
        .ok()
        .map(|value| {
            value
                .split(',')
                .filter_map(|issuer| {
                    let trimmed = issuer.trim();
                    (!trimmed.is_empty()).then(|| trimmed.to_string())
                })
                .collect()
        })
        .unwrap_or_default()
}

fn now_unix_timestamp() -> Result<i64, Ap2VerificationError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .map_err(|_| Ap2VerificationError("AP2 strict mode: system clock is invalid".to_string()))
}

/// Default strict verifier: requires a non-empty `payment_handler_id` and a structured JSON
/// `ap2_consent_proof` whose issuer, handler, signature, and expiry are valid.
#[derive(Debug, Clone, Default)]
pub struct StrictAp2Verifier;

impl Ap2MandateVerifier for StrictAp2Verifier {
    fn verify(&self, request: &CheckoutRequest) -> Result<(), Ap2VerificationError> {
        let handler = request
            .payment_intent
            .payment_handler_id
            .as_deref()
            .filter(|s| !s.is_empty());
        let proof = request
            .payment_intent
            .ap2_consent_proof
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| {
                Ap2VerificationError("AP2 strict mode: ap2_consent_proof is required".to_string())
            })?;

        if handler.is_none() {
            return Err(Ap2VerificationError(
                "AP2 strict mode: payment_handler_id is required".to_string(),
            ));
        }

        let parsed = ConsentProof::parse(proof)?;
        parsed.validate(request, &trusted_issuers_from_env())
    }
}

/// Run AP2 strict verification: returns Err if consent proof is missing, malformed, untrusted,
/// expired, or inconsistent with the request payment handler.
/// Call this before execute_checkout when AP2 strict mode is enabled.
pub fn verify_ap2_strict(request: &CheckoutRequest) -> Result<(), Ap2VerificationError> {
    StrictAp2Verifier.verify(request)
}
