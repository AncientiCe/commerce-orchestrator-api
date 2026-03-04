//! Phase 2 adapter scaffolding for UCP/A2A/AP2 interop.

use orchestrator_core::contract::CheckoutRequest;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UcpCheckoutEnvelope {
    pub capability: String,
    pub payload: CheckoutRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct A2AHandoffProfile {
    pub protocol: String,
    pub version: String,
    pub delegated_capability: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ap2PaymentMetadata {
    pub handler_id: Option<String>,
    pub consent_proof: Option<String>,
}

pub fn extract_ap2_metadata(request: &CheckoutRequest) -> Ap2PaymentMetadata {
    Ap2PaymentMetadata {
        handler_id: request.payment_intent.payment_handler_id.clone(),
        consent_proof: request.payment_intent.ap2_consent_proof.clone(),
    }
}
