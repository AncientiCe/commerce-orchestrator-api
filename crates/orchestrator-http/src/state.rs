//! Application state for the HTTP server.

use orchestrator_api::{AuthnResolver, OrchestratorFacade, SigningKeyring, VerifyingKeyring};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Shared state for all request handlers.
#[derive(Clone)]
pub struct AppState {
    pub facade: OrchestratorFacade,
    pub authn: Option<Arc<dyn AuthnResolver>>,
    /// When true, allow unauthenticated dev context when authn is None. When false (production), require authn and reject missing/invalid tokens.
    pub allow_dev_auth: bool,
    /// Set to true when shutdown signal received; readiness returns 503 when true.
    pub shutdown_flag: Arc<AtomicBool>,
    /// Base URL for this service (used in /.well-known/ucp discovery manifest). Defaults to http://127.0.0.1:port from bind.
    pub discovery_base_url: String,
    /// UCP signing material. `None` means this deployment does not sign or verify
    /// UCP messages, and discovery says so instead of advertising a key it lacks.
    pub signing: Option<SigningKeyring>,
    /// Public keys of agents whose signatures we require and verify. `None` means
    /// inbound signatures are not required.
    pub inbound_keys: Option<VerifyingKeyring>,
}

impl AppState {
    pub fn new(facade: OrchestratorFacade) -> Self {
        Self {
            facade,
            authn: None,
            allow_dev_auth: true,
            shutdown_flag: Arc::new(AtomicBool::new(false)),
            discovery_base_url: "http://127.0.0.1:8080".to_string(),
            signing: None,
            inbound_keys: None,
        }
    }

    /// Attach UCP signing material so responses are signed with the active key.
    pub fn with_signing(mut self, signing: SigningKeyring) -> Self {
        self.signing = Some(signing);
        self
    }

    /// Require and verify inbound agent signatures against these public keys.
    pub fn with_inbound_verification(mut self, keys: VerifyingKeyring) -> Self {
        self.inbound_keys = Some(keys);
        self
    }

    /// Set the base URL used in the discovery manifest (/.well-known/ucp). Call when deploying behind a known public URL.
    pub fn with_discovery_base_url(mut self, url: String) -> Self {
        self.discovery_base_url = url;
        self
    }

    /// Returns true if the server is shutting down (readiness should fail).
    pub fn is_shutting_down(&self) -> bool {
        self.shutdown_flag.load(Ordering::SeqCst)
    }

    pub fn with_authn(mut self, authn: Arc<dyn AuthnResolver>) -> Self {
        self.authn = Some(authn);
        self
    }

    /// Set production mode: require auth resolver; no dev fallback.
    pub fn production_mode(mut self, production: bool) -> Self {
        self.allow_dev_auth = !production;
        self
    }
}
