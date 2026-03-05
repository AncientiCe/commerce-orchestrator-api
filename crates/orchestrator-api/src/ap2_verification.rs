//! AP2 mandate/credential verification hooks. When AP2 strict mode is enabled, checkout
//! requires valid consent proof and payment handler; invalid or missing artifacts fail closed.

use orchestrator_core::contract::CheckoutRequest;

#[derive(Debug, Clone, thiserror::Error)]
#[error("AP2 verification failed: {0}")]
pub struct Ap2VerificationError(pub String);

/// Verifier trait for pluggable AP2 mandate/VDC validation (signature, issuer, expiry, replay).
/// Implement this to integrate a real AP2 credential stack; the default strict check only enforces presence.
pub trait Ap2MandateVerifier: Send + Sync {
    fn verify(&self, request: &CheckoutRequest) -> Result<(), Ap2VerificationError>;
}

/// Default strict verifier: requires non-empty `ap2_consent_proof` and `payment_handler_id`
/// when AP2 strict mode is on. Use this or a custom implementation for fail-closed behavior.
#[derive(Debug, Clone, Default)]
pub struct StrictAp2Verifier;

impl Ap2MandateVerifier for StrictAp2Verifier {
    fn verify(&self, request: &CheckoutRequest) -> Result<(), Ap2VerificationError> {
        let proof = request
            .payment_intent
            .ap2_consent_proof
            .as_deref()
            .filter(|s| !s.is_empty());
        let handler = request
            .payment_intent
            .payment_handler_id
            .as_deref()
            .filter(|s| !s.is_empty());
        if proof.is_none() {
            return Err(Ap2VerificationError(
                "AP2 strict mode: ap2_consent_proof is required".to_string(),
            ));
        }
        if handler.is_none() {
            return Err(Ap2VerificationError(
                "AP2 strict mode: payment_handler_id is required".to_string(),
            ));
        }
        Ok(())
    }
}

/// Run AP2 strict verification: returns Err if consent proof or payment handler is missing/empty.
/// Call this before execute_checkout when AP2 strict mode is enabled.
pub fn verify_ap2_strict(request: &CheckoutRequest) -> Result<(), Ap2VerificationError> {
    StrictAp2Verifier.verify(request)
}
