//! Binary entrypoint for the orchestrator HTTP server.
//!
//! Config: file-first (CONFIG_FILE or config.yaml) with env overrides.
//! Production: ENV=production and all required vars (PERSISTENCE_PATH, AUTH_BEARER_TOKEN, and all six component base URLs).
//! Development: default; uses mocks and allows unauthenticated dev context.

use std::net::SocketAddr;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use orchestrator_http::{app, auth::StaticTokenAuthnResolver, config, AppState};
use provider_mocks::{
    MockCatalogProvider, MockGeoProvider, MockPaymentProvider, MockPricingProvider,
    MockReceiptProvider, MockTaxProvider,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt::init();

    let profile = config::EnvProfile::from_env();
    let server_config = config::ServerConfig::load(config::default_config_path().as_deref())
        .map_err(|e| format!("config: {}", e))?;

    let (facade, authn, allow_dev_auth, discovery_base_url) = if profile.is_production() {
        let prod = server_config
            .require_production()
            .map_err(|e| format!("production config: {}", e))?;
        let client_config = prod.http_client.to_client_config();
        let catalog = Arc::new(
            integration_adapters::CatalogHttpAdapter::new(
                &prod.components.catalog_base_url,
                client_config.clone(),
            )
            .map_err(|e| format!("catalog adapter: {}", e))?,
        );
        let pricing = Arc::new(
            integration_adapters::PricingHttpAdapter::new(
                &prod.components.pricing_base_url,
                client_config.clone(),
            )
            .map_err(|e| format!("pricing adapter: {}", e))?,
        );
        let tax = Arc::new(
            integration_adapters::TaxHttpAdapter::new(
                &prod.components.tax_base_url,
                client_config.clone(),
            )
            .map_err(|e| format!("tax adapter: {}", e))?,
        );
        let geo = Arc::new(
            integration_adapters::GeoHttpAdapter::new(
                &prod.components.geo_base_url,
                client_config.clone(),
            )
            .map_err(|e| format!("geo adapter: {}", e))?,
        );
        let payment = Arc::new(
            integration_adapters::PaymentHttpAdapter::new(
                &prod.components.payment_base_url,
                client_config.clone(),
            )
            .map_err(|e| format!("payment adapter: {}", e))?,
        );
        let receipt = Arc::new(
            integration_adapters::ReceiptHttpAdapter::new(
                &prod.components.receipt_base_url,
                client_config.clone(),
            )
            .map_err(|e| format!("receipt adapter: {}", e))?,
        );
        let policy = orchestrator_core::policy::PolicyEngine::default();
        let ap2_strict = matches!(
            std::env::var("AP2_STRICT").as_deref(),
            Ok("1") | Ok("true") | Ok("yes")
        );
        let facade = orchestrator_api::OrchestratorFacade::new_persistent(
            catalog,
            pricing,
            tax,
            geo,
            payment,
            receipt,
            policy,
            &prod.persistence_path,
        )
        .await
        .map_err(|e| format!("persistent facade: {}", e))?
        .with_ap2_strict(ap2_strict);
        let authn = Arc::new(StaticTokenAuthnResolver::new(
            prod.auth_token,
            prod.auth_tenant_id,
            prod.auth_caller_id,
        ));
        (facade, Some(authn), false, prod.public_base_url)
    } else {
        let catalog = Arc::new(MockCatalogProvider::default());
        let pricing = Arc::new(MockPricingProvider);
        let tax = Arc::new(MockTaxProvider);
        let geo = Arc::new(MockGeoProvider);
        let payment = Arc::new(MockPaymentProvider);
        let receipt = Arc::new(MockReceiptProvider);
        let policy = orchestrator_core::policy::PolicyEngine::default();
        let ap2_strict = matches!(
            std::env::var("AP2_STRICT").as_deref(),
            Ok("1") | Ok("true") | Ok("yes")
        );
        let facade = orchestrator_api::OrchestratorFacade::new(
            catalog, pricing, tax, geo, payment, receipt, policy,
        )
        .with_ap2_strict(ap2_strict);
        let discovery_base_url = server_config
            .server
            .public_base_url
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                let port = server_config
                    .server
                    .bind_addr
                    .rsplit(':')
                    .next()
                    .unwrap_or("8080");
                format!("http://127.0.0.1:{}", port)
            });
        (facade, None, true, discovery_base_url)
    };
    let state = AppState::new(facade)
        .production_mode(profile.is_production())
        .with_discovery_base_url(discovery_base_url);
    let state = if let Some(a) = authn {
        state.with_authn(a)
    } else {
        state
    };

    let router = app::app();
    let addr: SocketAddr = server_config.server.bind_addr.parse()?;
    tracing::info!(
        profile = ?profile,
        allow_dev_auth = allow_dev_auth,
        "orchestrator API listening on {}",
        addr
    );

    let shutdown_flag = state.shutdown_flag.clone();
    let server = tokio::spawn(async move { app::serve(router, state, addr).await });
    tokio::select! {
        res = server => {
            match res {
                Ok(Ok(())) => Ok(()),
                Ok(Err(e)) => Err(e.to_string().into()),
                Err(e) => Err(format!("server task: {}", e).into()),
            }
        }
        _ = shutdown_signal() => {
            tracing::info!("shutdown signal received, draining");
            shutdown_flag.store(true, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_secs(5)).await;
            std::process::exit(0);
        }
    }
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("register SIGTERM");
        let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
            .expect("register SIGINT");
        tokio::select! {
            _ = sigterm.recv() => {}
            _ = sigint.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await.expect("register ctrl_c");
    }
}
