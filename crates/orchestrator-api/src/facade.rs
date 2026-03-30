//! Library facade: single entrypoint for agents/apps.

use crate::adapters::IdentityLinkRequest;
use crate::ap2_verification::{
    extract_ap2_mandate_record, verify_ap2_strict, Ap2VerificationError,
};
use crate::authz::{authorize_checkout, AuthContext, AuthzError};
use orchestrator_core::contract::{
    CartCommand, CartId, CartProjection, CheckoutRequest, PaymentLifecycleRequest, PaymentState,
    TransactionResult,
};
use orchestrator_core::policy::PolicyEngine;
use orchestrator_core::{UCP_LATEST_VERSION, UCP_SUPPORTED_VERSIONS};
use orchestrator_runtime::{ProviderSet, Runner, RunnerError};
use provider_contracts::{
    CatalogProvider, GeoProvider, PaymentOperationResult, PaymentProvider, PricingProvider,
    ReceiptProvider, TaxProvider,
};
use std::sync::Arc;
use std::time::Instant;

/// Orchestrator facade: cart commands and checkout execution.
#[derive(Clone)]
pub struct OrchestratorFacade {
    runner: Runner,
    /// When true, checkout requires valid AP2 artifacts (consent proof, payment_handler_id); fail closed if missing.
    ap2_strict: bool,
}

impl OrchestratorFacade {
    pub fn new(
        catalog: Arc<dyn CatalogProvider>,
        pricing: Arc<dyn PricingProvider>,
        tax: Arc<dyn TaxProvider>,
        geo: Arc<dyn GeoProvider>,
        payment: Arc<dyn PaymentProvider>,
        receipt: Arc<dyn ReceiptProvider>,
        policy: PolicyEngine,
    ) -> Self {
        let providers = ProviderSet {
            catalog,
            pricing,
            tax,
            geo,
            payment,
            receipt,
        };
        Self {
            runner: Runner::new(providers, policy),
            ap2_strict: false,
        }
    }

    /// Enable AP2 strict mode: checkout will fail if the request is missing a payment handler
    /// or if the consent proof is missing, malformed, expired, or bound to a different handler.
    pub fn with_ap2_strict(mut self, strict: bool) -> Self {
        self.ap2_strict = strict;
        self
    }

    /// Create a facade with persistent file-backed stores (for production).
    #[allow(clippy::too_many_arguments)]
    pub async fn new_persistent(
        catalog: Arc<dyn CatalogProvider>,
        pricing: Arc<dyn PricingProvider>,
        tax: Arc<dyn TaxProvider>,
        geo: Arc<dyn GeoProvider>,
        payment: Arc<dyn PaymentProvider>,
        receipt: Arc<dyn ReceiptProvider>,
        policy: PolicyEngine,
        base_path: impl AsRef<std::path::Path>,
    ) -> Result<Self, std::io::Error> {
        let providers = ProviderSet {
            catalog,
            pricing,
            tax,
            geo,
            payment,
            receipt,
        };
        let runner = Runner::new_persistent(providers, policy, base_path).await?;
        Ok(Self {
            runner,
            ap2_strict: false,
        })
    }

    /// Create a facade with PostgreSQL-backed stores (for production).
    #[allow(clippy::too_many_arguments)]
    pub async fn new_postgres(
        catalog: Arc<dyn CatalogProvider>,
        pricing: Arc<dyn PricingProvider>,
        tax: Arc<dyn TaxProvider>,
        geo: Arc<dyn GeoProvider>,
        payment: Arc<dyn PaymentProvider>,
        receipt: Arc<dyn ReceiptProvider>,
        policy: PolicyEngine,
        database_url: &str,
    ) -> Result<Self, std::io::Error> {
        let providers = ProviderSet {
            catalog,
            pricing,
            tax,
            geo,
            payment,
            receipt,
        };
        let runner = Runner::new_postgres(providers, policy, database_url).await?;
        Ok(Self {
            runner,
            ap2_strict: false,
        })
    }

    /// Dispatch a cart command.
    pub async fn dispatch_cart_command(
        &self,
        cmd: CartCommand,
        cart_id: Option<CartId>,
    ) -> Result<CartProjection, FacadeError> {
        self.runner
            .dispatch_cart_command(cmd, cart_id)
            .await
            .map_err(FacadeError::Runner)
    }

    /// Execute checkout for a cart.
    pub async fn execute_checkout(
        &self,
        request: CheckoutRequest,
    ) -> Result<TransactionResult, FacadeError> {
        if self.ap2_strict {
            verify_ap2_strict(&request).map_err(FacadeError::Ap2Verification)?;
            let record =
                extract_ap2_mandate_record(&request).map_err(FacadeError::Ap2Verification)?;
            let accepted = self
                .runner
                .record_mandate(&record.mandate_id, record.expires_at)
                .await
                .map_err(FacadeError::Runner)?;
            if !accepted {
                return Err(FacadeError::Ap2Verification(Ap2VerificationError(
                    "AP2 strict mode: mandate replay detected".to_string(),
                )));
            }
        }
        self.runner
            .execute_checkout(request)
            .await
            .map_err(FacadeError::Runner)
    }

    /// Execute checkout with explicit authz and tenant boundary enforcement.
    pub async fn execute_checkout_authorized(
        &self,
        context: &AuthContext,
        request: CheckoutRequest,
    ) -> Result<TransactionResult, FacadeError> {
        authorize_checkout(context, &request).map_err(FacadeError::Authz)?;
        self.execute_checkout(request).await
    }

    pub async fn capture_payment(
        &self,
        request: &PaymentLifecycleRequest,
    ) -> Result<PaymentOperationResult, FacadeError> {
        self.runner
            .capture_payment(request)
            .await
            .map_err(FacadeError::Runner)
    }

    pub async fn void_payment(
        &self,
        request: &PaymentLifecycleRequest,
    ) -> Result<PaymentOperationResult, FacadeError> {
        self.runner
            .void_payment(request)
            .await
            .map_err(FacadeError::Runner)
    }

    pub async fn refund_payment(
        &self,
        request: &PaymentLifecycleRequest,
    ) -> Result<PaymentOperationResult, FacadeError> {
        self.runner
            .refund_payment(request)
            .await
            .map_err(FacadeError::Runner)
    }

    /// Run payment reconciliation for the given transaction IDs.
    pub async fn run_reconciliation(
        &self,
        transaction_ids: &[String],
    ) -> orchestrator_runtime::ReconciliationReport {
        self.runner.run_reconciliation(transaction_ids).await
    }

    /// Read our stored payment state for one transaction.
    pub async fn get_payment_state(&self, transaction_id: &str) -> Option<PaymentState> {
        self.runner.get_payment_state(transaction_id).await
    }

    /// Process one outbox message; after max_attempts failures it is moved to dead-letter.
    pub async fn process_outbox_once(&self, max_attempts: u32) -> Result<(), FacadeError> {
        self.runner
            .process_outbox_once(max_attempts)
            .await
            .map_err(FacadeError::Runner)
    }

    /// List dead-letter entries for diagnostics (id, topic, correlation_id, attempts).
    pub async fn list_dead_letter(&self) -> Vec<orchestrator_runtime::OutboxMessage> {
        self.runner.list_dead_letter().await
    }

    /// Replay a message from dead-letter back to the outbox.
    pub async fn replay_from_dead_letter(&self, message_id: &str) -> Result<bool, FacadeError> {
        self.runner
            .replay_from_dead_letter(message_id)
            .await
            .map_err(FacadeError::Runner)
    }

    /// Accept an incoming event once (idempotent dedupe for webhooks). Returns true if accepted, false if duplicate.
    pub async fn accept_incoming_event_once(&self, message_id: &str) -> Result<bool, FacadeError> {
        self.runner
            .accept_incoming_event_once(message_id)
            .await
            .map_err(FacadeError::Runner)
    }

    /// Link a platform identity to an agent-facing commerce context.
    /// This lightweight endpoint keeps compatibility while exposing standardized identity-linking.
    pub async fn link_identity(
        &self,
        request: IdentityLinkRequest,
    ) -> Result<IdentityLinkResult, FacadeError> {
        let started = Instant::now();
        orchestrator_observability::incr("identity_link_requests_total");

        if request.tenant_id.trim().is_empty()
            || request.merchant_id.trim().is_empty()
            || request.agent_id.trim().is_empty()
            || request.link_token.trim().is_empty()
        {
            orchestrator_observability::incr("identity_link_errors_total");
            orchestrator_observability::observe_operation(
                "identity_link",
                "error",
                started.elapsed().as_secs_f64(),
            );
            return Err(FacadeError::IdentityLink(
                "tenant_id, merchant_id, agent_id, and link_token are required".to_string(),
            ));
        }

        let result = IdentityLinkResult {
            ucp_version: UCP_LATEST_VERSION.to_string(),
            supported_versions: UCP_SUPPORTED_VERSIONS
                .iter()
                .map(|v| (*v).to_string())
                .collect(),
            link_id: format!("idlink_{}", uuid::Uuid::new_v4()),
            status: "linked".to_string(),
        };
        orchestrator_observability::incr("identity_link_success_total");
        orchestrator_observability::observe_operation(
            "identity_link",
            "success",
            started.elapsed().as_secs_f64(),
        );
        Ok(result)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IdentityLinkResult {
    pub ucp_version: String,
    pub supported_versions: Vec<String>,
    pub link_id: String,
    pub status: String,
}

#[derive(Debug, thiserror::Error)]
pub enum FacadeError {
    #[error("orchestrator runner failed: {0}")]
    Runner(#[from] RunnerError),
    #[error("authorization failed: {0}")]
    Authz(#[from] AuthzError),
    #[error("AP2 verification failed: {0}")]
    Ap2Verification(#[from] Ap2VerificationError),
    #[error("identity linking failed: {0}")]
    IdentityLink(String),
}
