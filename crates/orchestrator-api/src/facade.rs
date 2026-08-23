//! Library facade: single entrypoint for agents/apps.

use crate::adapters::IdentityLinkRequest;
use crate::ap2_verification::{
    extract_ap2_mandate_record, verify_ap2_strict, Ap2VerificationError,
};
use crate::authz::{authorize_checkout, AuthContext, AuthzError};
use orchestrator_core::contract::{
    CartCommand, CartId, CartProjection, CheckoutRequest, OrderRecord, PaymentLifecycleRequest,
    PaymentState, TransactionResult, TransactionStatus,
};
use orchestrator_core::policy::PolicyEngine;
use orchestrator_core::{UCP_LATEST_VERSION, UCP_SUPPORTED_VERSIONS};
use orchestrator_runtime::{ProviderSet, Runner, RunnerError};
use provider_contracts::{
    CatalogProvider, DelegatedPayment, FulfillmentProvider, GeoProvider, IdentityLinkProvider,
    IdentityLinkRequest as ProviderIdentityLinkRequest, PaymentDelegationProvider,
    PaymentDelegationRequest, PaymentOperationResult, PaymentProvider, PricingProvider,
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
    /// PSP that mints delegated payment tokens. `None` means this deployment does
    /// not offer delegation, and says so in discovery and at the endpoint.
    payment_delegation: Option<Arc<dyn PaymentDelegationProvider>>,
    /// Identity system that binds an agent to a platform identity. `None` means
    /// identity linking is not offered here.
    identity_link: Option<Arc<dyn IdentityLinkProvider>>,
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
            fulfillment: None,
        };
        Self {
            runner: Runner::new(providers, policy),
            ap2_strict: false,
            payment_delegation: None,
            identity_link: None,
        }
    }

    /// Enable AP2 strict mode: checkout will fail if the request is missing a payment handler
    /// or if the consent proof is missing, malformed, expired, or bound to a different handler.
    pub fn with_ap2_strict(mut self, strict: bool) -> Self {
        self.ap2_strict = strict;
        self
    }

    /// Route fulfillment rating to a provider instead of the built-in static rate table.
    pub fn with_fulfillment_provider(mut self, provider: Arc<dyn FulfillmentProvider>) -> Self {
        self.runner = self.runner.with_fulfillment_provider(provider);
        self
    }

    /// Delegate payment tokens through a real PSP, enabling `POST /acp/delegate_payment`.
    pub fn with_payment_delegation(mut self, provider: Arc<dyn PaymentDelegationProvider>) -> Self {
        self.payment_delegation = Some(provider);
        self
    }

    /// Link identities through a real identity system, enabling identity linking.
    pub fn with_identity_link_provider(mut self, provider: Arc<dyn IdentityLinkProvider>) -> Self {
        self.identity_link = Some(provider);
        self
    }

    /// Whether this deployment can actually delegate payment.
    pub fn supports_payment_delegation(&self) -> bool {
        self.payment_delegation.is_some()
    }

    /// Whether this deployment can actually link identities.
    pub fn supports_identity_linking(&self) -> bool {
        self.identity_link.is_some()
    }

    /// Deliver outbox messages to this orchestrator's registered webhooks.
    pub fn with_webhook_delivery(mut self) -> Self {
        let deliverer = orchestrator_runtime::webhook_deliverer_for(&self.runner);
        self.runner = self.runner.with_outbox_deliverer(deliverer);
        self
    }

    /// Start the background outbox processor. The returned handle completes once
    /// `shutdown` is set and the queue has drained.
    pub fn spawn_outbox_processor(
        &self,
        config: orchestrator_runtime::OutboxProcessorConfig,
        shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> tokio::task::JoinHandle<()> {
        orchestrator_runtime::OutboxProcessor::new(self.runner.clone(), config).spawn(shutdown)
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
            fulfillment: None,
        };
        let runner = Runner::new_persistent(providers, policy, base_path).await?;
        Ok(Self {
            runner,
            ap2_strict: false,
            payment_delegation: None,
            identity_link: None,
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
            fulfillment: None,
        };
        let runner = Runner::new_postgres(providers, policy, database_url).await?;
        Ok(Self {
            runner,
            ap2_strict: false,
            payment_delegation: None,
            identity_link: None,
        })
    }

    /// Dispatch a cart command.
    pub async fn dispatch_cart_command(
        &self,
        cmd: CartCommand,
        cart_id: Option<CartId>,
    ) -> Result<CartProjection, FacadeError> {
        let op = cart_command_operation(&cmd);
        let started = Instant::now();
        let result = self
            .runner
            .dispatch_cart_command(cmd, cart_id)
            .await
            .map_err(FacadeError::Runner);
        let status = if result.is_ok() { "success" } else { "error" };
        orchestrator_observability::observe_operation(op, status, started.elapsed().as_secs_f64());
        result
    }

    /// Dispatch a cart command on behalf of an authenticated caller.
    ///
    /// A cart id is guessable-in-principle and is presented by whoever holds it,
    /// so it is not an authorization. Creation stamps the caller's tenant onto the
    /// cart; every other command is refused unless the cart already belongs to
    /// that tenant. Protocol shims (REST, UCP, ACP, A2A, MCP) must use this
    /// instead of [`Self::dispatch_cart_command`], which trusts its caller.
    pub async fn dispatch_cart_command_for_tenant(
        &self,
        tenant_id: &str,
        cmd: CartCommand,
        cart_id: Option<CartId>,
    ) -> Result<CartProjection, FacadeError> {
        let cmd = cmd.with_tenant(tenant_id);
        if let Some(target) = cart_id.or_else(|| cart_id_from_command(&cmd)) {
            self.assert_cart_belongs_to(tenant_id, &target).await?;
        }
        self.dispatch_cart_command(cmd, cart_id).await
    }

    /// Refuse to touch a cart owned by another tenant. A cart with no owner
    /// recorded is treated as inaccessible rather than public.
    async fn assert_cart_belongs_to(
        &self,
        tenant_id: &str,
        cart_id: &CartId,
    ) -> Result<(), FacadeError> {
        let Some(cart) = self.runner.cart_snapshot(cart_id).await else {
            // Let the command itself report "not found", so behaviour for a
            // missing cart is unchanged.
            return Ok(());
        };
        if cart.tenant_id.as_deref() == Some(tenant_id) {
            return Ok(());
        }
        orchestrator_observability::incr("cart_tenant_mismatch_total");
        Err(FacadeError::Authz(AuthzError::TenantMismatch))
    }

    /// Execute checkout for a cart.
    pub async fn execute_checkout(
        &self,
        request: CheckoutRequest,
    ) -> Result<TransactionResult, FacadeError> {
        let started = Instant::now();
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
                orchestrator_observability::observe_operation(
                    "checkout_execute",
                    "rejected",
                    started.elapsed().as_secs_f64(),
                );
                return Err(FacadeError::Ap2Verification(Ap2VerificationError(
                    "AP2 strict mode: mandate replay detected".to_string(),
                )));
            }
        }
        let result = self
            .runner
            .execute_checkout(request)
            .await
            .map_err(FacadeError::Runner);
        let status = match &result {
            Ok(r) => checkout_status_label(r),
            Err(_) => "error",
        };
        orchestrator_observability::observe_operation(
            "checkout_execute",
            status,
            started.elapsed().as_secs_f64(),
        );
        result
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
        let started = Instant::now();
        let result = self
            .runner
            .capture_payment(request)
            .await
            .map_err(FacadeError::Runner);
        let status = if result.is_ok() { "success" } else { "error" };
        orchestrator_observability::observe_operation(
            "payment_capture",
            status,
            started.elapsed().as_secs_f64(),
        );
        result
    }

    pub async fn void_payment(
        &self,
        request: &PaymentLifecycleRequest,
    ) -> Result<PaymentOperationResult, FacadeError> {
        let started = Instant::now();
        let result = self
            .runner
            .void_payment(request)
            .await
            .map_err(FacadeError::Runner);
        let status = if result.is_ok() { "success" } else { "error" };
        orchestrator_observability::observe_operation(
            "payment_void",
            status,
            started.elapsed().as_secs_f64(),
        );
        result
    }

    pub async fn refund_payment(
        &self,
        request: &PaymentLifecycleRequest,
    ) -> Result<PaymentOperationResult, FacadeError> {
        let started = Instant::now();
        let result = self
            .runner
            .refund_payment(request)
            .await
            .map_err(FacadeError::Runner);
        let status = if result.is_ok() { "success" } else { "error" };
        orchestrator_observability::observe_operation(
            "payment_refund",
            status,
            started.elapsed().as_secs_f64(),
        );
        result
    }

    /// Run payment reconciliation for the given transaction IDs.
    pub async fn run_reconciliation(
        &self,
        transaction_ids: &[String],
    ) -> orchestrator_runtime::ReconciliationReport {
        let started = Instant::now();
        let report = self.runner.run_reconciliation(transaction_ids).await;
        orchestrator_observability::observe_operation(
            "reconciliation",
            "success",
            started.elapsed().as_secs_f64(),
        );
        report
    }

    /// Read our stored payment state for one transaction.
    pub async fn get_payment_state(&self, transaction_id: &str) -> Option<PaymentState> {
        self.runner.get_payment_state(transaction_id).await
    }

    /// Retrieve an order by ID.
    pub async fn get_order(&self, order_id: &str) -> Result<Option<OrderRecord>, FacadeError> {
        let started = Instant::now();
        let result = self.runner.get_order(order_id).await;
        let status = if result.is_some() {
            "success"
        } else {
            "not_found"
        };
        orchestrator_observability::observe_operation(
            "order_get",
            status,
            started.elapsed().as_secs_f64(),
        );
        Ok(result)
    }

    /// List orders for a tenant, most recent first.
    pub async fn list_orders(&self, tenant_id: &str) -> Result<Vec<OrderRecord>, FacadeError> {
        let started = Instant::now();
        let result = self
            .runner
            .list_orders(tenant_id)
            .await
            .map_err(FacadeError::Runner);
        let status = if result.is_ok() { "success" } else { "error" };
        orchestrator_observability::observe_operation(
            "order_list",
            status,
            started.elapsed().as_secs_f64(),
        );
        result
    }

    /// Process one outbox message; after max_attempts failures it is moved to dead-letter.
    pub async fn process_outbox_once(&self, max_attempts: u32) -> Result<(), FacadeError> {
        let started = Instant::now();
        let result = self
            .runner
            .process_outbox_once(max_attempts)
            .await
            .map_err(FacadeError::Runner);
        let status = if result.is_ok() { "success" } else { "error" };
        orchestrator_observability::observe_operation(
            "outbox_process",
            status,
            started.elapsed().as_secs_f64(),
        );
        result
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

    /// Register a webhook for event delivery.
    pub async fn register_webhook(
        &self,
        registration: orchestrator_runtime::WebhookRegistration,
    ) -> Result<(), FacadeError> {
        self.runner
            .register_webhook(registration)
            .await
            .map_err(FacadeError::Runner)
    }

    /// Unregister a webhook by ID.
    pub async fn unregister_webhook(&self, id: &str) -> Result<bool, FacadeError> {
        self.runner
            .unregister_webhook(id)
            .await
            .map_err(FacadeError::Runner)
    }

    /// List webhooks for a tenant.
    pub async fn list_webhooks(
        &self,
        tenant_id: &str,
    ) -> Result<Vec<orchestrator_runtime::WebhookRegistration>, FacadeError> {
        self.runner
            .list_webhooks(tenant_id)
            .await
            .map_err(FacadeError::Runner)
    }

    /// Look up a catalog item by ID (passthrough to catalog provider).
    pub async fn lookup_catalog_item(
        &self,
        item_id: &str,
    ) -> Result<provider_contracts::CatalogItem, FacadeError> {
        let started = Instant::now();
        let result = self
            .runner
            .lookup_catalog_item(item_id)
            .await
            .map_err(FacadeError::Runner);
        let status = if result.is_ok() { "success" } else { "error" };
        orchestrator_observability::observe_operation(
            "catalog_lookup",
            status,
            started.elapsed().as_secs_f64(),
        );
        result
    }

    /// Look up multiple catalog items by ID.
    pub async fn lookup_catalog_items(
        &self,
        item_ids: &[String],
    ) -> Result<Vec<provider_contracts::CatalogItem>, FacadeError> {
        let started = Instant::now();
        let result = self
            .runner
            .lookup_catalog_items(item_ids)
            .await
            .map_err(FacadeError::Runner);
        let status = if result.is_ok() { "success" } else { "error" };
        orchestrator_observability::observe_operation(
            "catalog_batch_lookup",
            status,
            started.elapsed().as_secs_f64(),
        );
        result
    }

    /// Search catalog items.
    pub async fn search_catalog_items(
        &self,
        query: Option<&str>,
    ) -> Result<Vec<provider_contracts::CatalogItem>, FacadeError> {
        let started = Instant::now();
        let result = self
            .runner
            .search_catalog_items(query)
            .await
            .map_err(FacadeError::Runner);
        let status = if result.is_ok() { "success" } else { "error" };
        orchestrator_observability::observe_operation(
            "catalog_search",
            status,
            started.elapsed().as_secs_f64(),
        );
        result
    }

    /// Link a platform identity to an agent-facing commerce context via the
    /// configured identity provider. Without one, this deployment does not offer
    /// identity linking and says so rather than inventing a link id.
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

        let Some(provider) = self.identity_link.as_ref() else {
            orchestrator_observability::incr("identity_link_not_configured_total");
            orchestrator_observability::observe_operation(
                "identity_link",
                "not_configured",
                started.elapsed().as_secs_f64(),
            );
            return Err(FacadeError::NotConfigured(
                "identity linking (set IDENTITY_LINK_BASE_URL)",
            ));
        };

        let record = provider
            .link(&ProviderIdentityLinkRequest {
                tenant_id: request.tenant_id,
                merchant_id: request.merchant_id,
                agent_id: request.agent_id,
                link_token: request.link_token,
            })
            .await;
        let record = match record {
            Ok(record) => record,
            Err(error) => {
                orchestrator_observability::incr("identity_link_errors_total");
                orchestrator_observability::observe_operation(
                    "identity_link",
                    "error",
                    started.elapsed().as_secs_f64(),
                );
                return Err(FacadeError::IdentityLink(error.to_string()));
            }
        };

        orchestrator_observability::incr("identity_link_success_total");
        orchestrator_observability::observe_operation(
            "identity_link",
            "success",
            started.elapsed().as_secs_f64(),
        );
        Ok(IdentityLinkResult {
            ucp_version: UCP_LATEST_VERSION.to_string(),
            supported_versions: UCP_SUPPORTED_VERSIONS
                .iter()
                .map(|v| (*v).to_string())
                .collect(),
            link_id: record.link_id,
            status: record.status,
        })
    }

    /// Exchange the caller's payment credential for a delegated one at the PSP.
    pub async fn delegate_payment(
        &self,
        request: PaymentDelegationRequest,
    ) -> Result<DelegatedPayment, FacadeError> {
        let started = Instant::now();
        orchestrator_observability::incr("payment_delegation_requests_total");

        let Some(provider) = self.payment_delegation.as_ref() else {
            orchestrator_observability::incr("payment_delegation_not_configured_total");
            orchestrator_observability::observe_operation(
                "payment_delegation",
                "not_configured",
                started.elapsed().as_secs_f64(),
            );
            return Err(FacadeError::NotConfigured(
                "payment delegation (set PAYMENT_DELEGATION_BASE_URL)",
            ));
        };

        let result = provider
            .delegate(&request)
            .await
            .map_err(|error| FacadeError::PaymentDelegation(error.to_string()));
        let status = if result.is_ok() { "success" } else { "error" };
        if result.is_err() {
            orchestrator_observability::incr("payment_delegation_errors_total");
        } else {
            orchestrator_observability::incr("payment_delegation_success_total");
        }
        orchestrator_observability::observe_operation(
            "payment_delegation",
            status,
            started.elapsed().as_secs_f64(),
        );
        result
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
    #[error("payment delegation failed: {0}")]
    PaymentDelegation(String),
    #[error("this deployment does not offer {0}")]
    NotConfigured(&'static str),
}

/// The cart a command addresses, for commands that carry the id in their payload.
fn cart_id_from_command(cmd: &CartCommand) -> Option<CartId> {
    match cmd {
        CartCommand::GetCart(payload) => Some(payload.cart_id),
        CartCommand::StartCheckout(payload) => Some(payload.cart_id),
        CartCommand::CancelCart(payload) => Some(payload.cart_id),
        _ => None,
    }
}

fn cart_command_operation(cmd: &CartCommand) -> &'static str {
    match cmd {
        CartCommand::CreateCart(_) => "cart_create",
        CartCommand::AddItem(_) => "cart_add_item",
        CartCommand::UpdateItemQty(_) => "cart_update_qty",
        CartCommand::RemoveItem(_) => "cart_remove_item",
        CartCommand::ApplyAdjustment(_) => "cart_apply_adjustment",
        CartCommand::GetCart(_) => "cart_get",
        CartCommand::StartCheckout(_) => "cart_start_checkout",
        CartCommand::CancelCart(_) => "cart_cancel",
        CartCommand::SetFulfillmentSelection(_) => "cart_set_fulfillment",
        _ => "cart_unknown",
    }
}

fn checkout_status_label(result: &TransactionResult) -> &'static str {
    match result.status {
        TransactionStatus::Completed => "completed",
        TransactionStatus::Rejected => "rejected",
        TransactionStatus::AuthFailed => "auth_failed",
        TransactionStatus::CommitFailed => "commit_failed",
        TransactionStatus::TimedOut => "timed_out",
        _ => "unknown",
    }
}
