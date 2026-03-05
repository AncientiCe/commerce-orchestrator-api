//! Application state for the HTTP server.

use orchestrator_api::{AuthnResolver, OrchestratorFacade};
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
}

impl AppState {
    pub fn new(facade: OrchestratorFacade) -> Self {
        Self {
            facade,
            authn: None,
            allow_dev_auth: true,
            shutdown_flag: Arc::new(AtomicBool::new(false)),
        }
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
