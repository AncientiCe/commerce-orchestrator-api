//! AP2 mandate/credential verification hooks. When AP2 strict mode is enabled, checkout
//! requires valid consent proof and payment handler; invalid or missing artifacts fail closed.
//!
//! Targets AP2 **0.2**: closed Payment/Checkout mandates plus optional open (HNP) mandates
//! with constraint binding (`vct` values such as `mandate.payment.1`, `mandate.checkout.open.1`).

use orchestrator_core::contract::CheckoutRequest;
use std::time::{SystemTime, UNIX_EPOCH};

/// Claimed AP2 protocol version for discovery and docs.
pub const AP2_PROTOCOL_VERSION: &str = "0.2";

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
    /// Optional AP2 mandate type claim (e.g. `mandate.payment.1`, `mandate.checkout.open.1`).
    #[serde(default)]
    vct: Option<String>,
    issuer: String,
    subject: String,
    mandate_id: String,
    payment_handler_id: String,
    issued_at: i64,
    expires_at: i64,
    signature: String,
    #[serde(default)]
    #[allow(dead_code)]
    nonce: Option<String>,
    /// Optional open (HNP) mandate that authorizes this closed mandate.
    #[serde(default)]
    open_mandate: Option<OpenMandate>,
    /// Optional closed-checkout binding hash (AP2 checkout_hash).
    #[serde(default)]
    #[allow(dead_code)]
    checkout_hash: Option<String>,
    /// Optional amount ceiling from open-mandate constraints (minor units).
    #[serde(default)]
    max_amount_minor: Option<i64>,
    /// Optional currency constraint from open mandate.
    #[serde(default)]
    currency: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct OpenMandate {
    vct: String,
    issuer: String,
    subject: String,
    mandate_id: String,
    issued_at: i64,
    expires_at: i64,
    signature: String,
    /// Agent confirmation key binding (cnf) for HNP agent-signed closed mandates.
    #[serde(default)]
    cnf: Option<String>,
    #[serde(default)]
    max_amount_minor: Option<i64>,
    #[serde(default)]
    currency: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Ap2MandateRecord {
    pub mandate_id: String,
    pub expires_at: i64,
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
        if let Some(vct) = self.vct.as_deref() {
            if !is_supported_closed_vct(vct) && !is_supported_open_vct(vct) {
                return Err(Ap2VerificationError(format!(
                    "AP2 strict mode: unsupported mandate vct '{vct}'"
                )));
            }
            // Closed mandates may be carried at top-level; open-only top-level is invalid for checkout.
            if is_supported_open_vct(vct) && self.open_mandate.is_none() {
                return Err(Ap2VerificationError(
                    "AP2 strict mode: open mandate alone cannot authorize checkout; closed mandate required".to_string(),
                ));
            }
        }

        validate_party_fields(
            &self.issuer,
            &self.subject,
            &self.mandate_id,
            &self.signature,
            &self.payment_handler_id,
            self.issued_at,
            self.expires_at,
            trusted_issuers,
        )?;

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

        if let Some(open) = &self.open_mandate {
            validate_open_mandate(open, self, request, trusted_issuers)?;
        } else if let Some(max) = self.max_amount_minor {
            if request.payment_intent.amount_minor > max {
                return Err(Ap2VerificationError(
                    "AP2 strict mode: payment amount exceeds mandate max_amount_minor".to_string(),
                ));
            }
        }

        if let Some(currency) = self.currency.as_deref() {
            if !currency.eq_ignore_ascii_case(&request.currency) {
                return Err(Ap2VerificationError(
                    "AP2 strict mode: mandate currency does not match checkout currency"
                        .to_string(),
                ));
            }
        }

        Ok(())
    }
}

fn is_supported_closed_vct(vct: &str) -> bool {
    matches!(
        vct,
        "mandate.payment.1" | "mandate.checkout.1" | "mandate.checkout.closed.1"
    )
}

fn is_supported_open_vct(vct: &str) -> bool {
    matches!(
        vct,
        "mandate.checkout.open.1" | "mandate.payment.open.1" | "mandate.payment.open"
    )
}

#[allow(clippy::too_many_arguments)]
fn validate_party_fields(
    issuer: &str,
    subject: &str,
    mandate_id: &str,
    signature: &str,
    payment_handler_id: &str,
    issued_at: i64,
    expires_at: i64,
    trusted_issuers: &[String],
) -> Result<(), Ap2VerificationError> {
    if issuer.trim().is_empty() {
        return Err(Ap2VerificationError(
            "AP2 strict mode: consent proof issuer is required".to_string(),
        ));
    }
    if !trusted_issuers.is_empty()
        && !trusted_issuers
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(issuer.trim()))
    {
        return Err(Ap2VerificationError(format!(
            "AP2 strict mode: consent proof issuer '{issuer}' is not trusted"
        )));
    }
    if subject.trim().is_empty() {
        return Err(Ap2VerificationError(
            "AP2 strict mode: consent proof subject is required".to_string(),
        ));
    }
    if mandate_id.trim().is_empty() {
        return Err(Ap2VerificationError(
            "AP2 strict mode: consent proof mandate_id is required".to_string(),
        ));
    }
    if signature.trim().is_empty() {
        return Err(Ap2VerificationError(
            "AP2 strict mode: consent proof signature is required".to_string(),
        ));
    }
    if payment_handler_id.trim().is_empty() {
        return Err(Ap2VerificationError(
            "AP2 strict mode: consent proof payment_handler_id is required".to_string(),
        ));
    }
    if issued_at <= 0 {
        return Err(Ap2VerificationError(
            "AP2 strict mode: consent proof issued_at must be a unix timestamp".to_string(),
        ));
    }
    if expires_at <= issued_at {
        return Err(Ap2VerificationError(
            "AP2 strict mode: consent proof expires_at must be after issued_at".to_string(),
        ));
    }

    let now = now_unix_timestamp()?;
    if expires_at <= now {
        return Err(Ap2VerificationError(
            "AP2 strict mode: consent proof has expired".to_string(),
        ));
    }
    Ok(())
}

fn validate_open_mandate(
    open: &OpenMandate,
    closed: &ConsentProof,
    request: &CheckoutRequest,
    trusted_issuers: &[String],
) -> Result<(), Ap2VerificationError> {
    if !is_supported_open_vct(&open.vct) {
        return Err(Ap2VerificationError(format!(
            "AP2 strict mode: unsupported open mandate vct '{}'",
            open.vct
        )));
    }
    if open.cnf.as_deref().unwrap_or("").trim().is_empty() {
        return Err(Ap2VerificationError(
            "AP2 strict mode: open mandate cnf (agent key binding) is required for HNP".to_string(),
        ));
    }
    validate_party_fields(
        &open.issuer,
        &open.subject,
        &open.mandate_id,
        &open.signature,
        &closed.payment_handler_id,
        open.issued_at,
        open.expires_at,
        trusted_issuers,
    )?;

    let max = open
        .max_amount_minor
        .or(closed.max_amount_minor)
        .unwrap_or(i64::MAX);
    if request.payment_intent.amount_minor > max {
        return Err(Ap2VerificationError(
            "AP2 strict mode: closed mandate amount exceeds open mandate max_amount_minor"
                .to_string(),
        ));
    }

    if let Some(currency) = open.currency.as_deref().or(closed.currency.as_deref()) {
        if !currency.eq_ignore_ascii_case(&request.currency) {
            return Err(Ap2VerificationError(
                "AP2 strict mode: open mandate currency constraint not satisfied".to_string(),
            ));
        }
    }

    Ok(())
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

/// Extract mandate replay metadata from strict AP2 consent proof.
pub fn extract_ap2_mandate_record(
    request: &CheckoutRequest,
) -> Result<Ap2MandateRecord, Ap2VerificationError> {
    let proof = request
        .payment_intent
        .ap2_consent_proof
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| {
            Ap2VerificationError("AP2 strict mode: ap2_consent_proof is required".to_string())
        })?;
    let parsed = ConsentProof::parse(proof)?;
    Ok(Ap2MandateRecord {
        mandate_id: parsed.mandate_id,
        expires_at: parsed.expires_at,
    })
}
