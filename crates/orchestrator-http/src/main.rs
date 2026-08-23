//! Binary entrypoint for the orchestrator HTTP server.
//!
//! Config: file-first (CONFIG_FILE or config.yaml) with env overrides.
//! Production: ENV=production and all required vars (DATABASE_URL, AUTH_BEARER_TOKEN, and all six component base URLs).
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

    if config::RunMode::from_env() == config::RunMode::MigrateOnly {
        let database_url = server_config
            .persistence
            .database_url
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or("DATABASE_URL (or persistence.database_url) required to run migrations")?;
        tracing::info!("applying pending database migrations");
        orchestrator_runtime::persistence::migrate(database_url)
            .await
            .map_err(|e| format!("migrations: {}", e))?;
        tracing::info!("migrations applied");
        return Ok(());
    }

    let (facade, authn, allow_dev_auth, discovery_base_url, signing) = if profile.is_production() {
        let prod = server_config
            .require_production()
            .map_err(|e| format!("production config: {}", e))?;
        let catalog = Arc::new(
            integration_adapters::CatalogHttpAdapter::new(
                &prod.components.catalog_base_url,
                prod.client_config_for(config::Provider::Catalog)?,
            )
            .map_err(|e| format!("catalog adapter: {}", e))?,
        );
        let pricing = Arc::new(
            integration_adapters::PricingHttpAdapter::new(
                &prod.components.pricing_base_url,
                prod.client_config_for(config::Provider::Pricing)?,
            )
            .map_err(|e| format!("pricing adapter: {}", e))?,
        );
        let tax = Arc::new(
            integration_adapters::TaxHttpAdapter::new(
                &prod.components.tax_base_url,
                prod.client_config_for(config::Provider::Tax)?,
            )
            .map_err(|e| format!("tax adapter: {}", e))?,
        );
        let geo = Arc::new(
            integration_adapters::GeoHttpAdapter::new(
                &prod.components.geo_base_url,
                prod.client_config_for(config::Provider::Geo)?,
            )
            .map_err(|e| format!("geo adapter: {}", e))?,
        );
        let payment = Arc::new(
            integration_adapters::PaymentHttpAdapter::new(
                &prod.components.payment_base_url,
                prod.client_config_for(config::Provider::Payment)?,
            )
            .map_err(|e| format!("payment adapter: {}", e))?,
        );
        let receipt = Arc::new(
            integration_adapters::ReceiptHttpAdapter::new(
                &prod.components.receipt_base_url,
                prod.client_config_for(config::Provider::Receipt)?,
            )
            .map_err(|e| format!("receipt adapter: {}", e))?,
        );
        let policy = orchestrator_core::policy::PolicyEngine::default();
        let ap2_strict = matches!(
            std::env::var("AP2_STRICT").as_deref(),
            Ok("1") | Ok("true") | Ok("yes")
        );
        let facade = orchestrator_api::OrchestratorFacade::new_postgres(
            catalog,
            pricing,
            tax,
            geo,
            payment,
            receipt,
            policy,
            &prod.database_url,
        )
        .await
        .map_err(|e| format!("postgres facade: {}", e))?
        .with_ap2_strict(ap2_strict)
        .with_webhook_delivery();
        let facade = match prod.components.fulfillment_base_url.as_deref() {
            Some(base_url) => facade.with_fulfillment_provider(Arc::new(
                integration_adapters::FulfillmentHttpAdapter::new(
                    base_url,
                    prod.client_config_for(config::Provider::Fulfillment)?,
                )
                .map_err(|e| format!("fulfillment adapter: {}", e))?,
            )),
            None => {
                tracing::warn!(
                    "FULFILLMENT_BASE_URL is not set; using built-in static shipping rates"
                );
                facade
            }
        };
        let facade = match prod.components.payment_delegation_base_url.as_deref() {
            Some(base_url) => facade.with_payment_delegation(Arc::new(
                integration_adapters::PaymentDelegationHttpAdapter::new(
                    base_url,
                    prod.client_config_for(config::Provider::PaymentDelegation)?,
                )
                .map_err(|e| format!("payment delegation adapter: {}", e))?,
            )),
            None => {
                tracing::warn!(
                    "PAYMENT_DELEGATION_BASE_URL is not set; ACP delegate_payment returns 501 \
                     and the capability is not advertised"
                );
                facade
            }
        };
        let facade = match prod.components.identity_link_base_url.as_deref() {
            Some(base_url) => facade.with_identity_link_provider(Arc::new(
                integration_adapters::IdentityLinkHttpAdapter::new(
                    base_url,
                    prod.client_config_for(config::Provider::IdentityLink)?,
                )
                .map_err(|e| format!("identity link adapter: {}", e))?,
            )),
            None => {
                tracing::warn!(
                    "IDENTITY_LINK_BASE_URL is not set; identity linking returns 501 and the \
                     capability is not advertised"
                );
                facade
            }
        };
        let authn: Arc<dyn orchestrator_api::AuthnResolver> = if prod.auth_mode == "jwt" {
            let trusted_issuers = prod
                .trusted_issuers
                .split(',')
                .filter_map(|issuer| {
                    let trimmed = issuer.trim();
                    (!trimmed.is_empty()).then(|| trimmed.to_string())
                })
                .collect::<Vec<_>>();
            Arc::new(orchestrator_api::JwtAuthnResolver::new_hs256(
                &prod.auth_token,
                trusted_issuers,
            ))
        } else {
            Arc::new(StaticTokenAuthnResolver::new(
                prod.auth_token,
                prod.auth_tenant_id,
                prod.auth_caller_id,
            ))
        };
        (
            facade,
            Some(authn),
            false,
            prod.public_base_url,
            Some(prod.signing),
        )
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
        .with_ap2_strict(ap2_strict)
        .with_webhook_delivery();
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
        // Development signs only if the operator supplied a key; there is no default.
        let signing = server_config
            .signing
            .to_keyring()
            .map_err(|e| format!("signing config: {}", e))?;
        if signing.is_none() {
            tracing::warn!(
                "UCP_SIGNING_KEY is not set; discovery will advertise no signing keys and \
                 UCP signatures will be neither produced nor verified"
            );
        }
        (facade, None, true, discovery_base_url, signing)
    };
    let (outbox_shutdown_tx, outbox_shutdown_rx) = tokio::sync::watch::channel(false);
    let outbox_processor = facade.spawn_outbox_processor(
        server_config.outbox.to_processor_config(),
        outbox_shutdown_rx,
    );

    let state = AppState::new(facade)
        .production_mode(profile.is_production())
        .with_discovery_base_url(discovery_base_url);
    let state = if let Some(a) = authn {
        state.with_authn(a)
    } else {
        state
    };
    let state = if let Some(keyring) = signing {
        tracing::info!(kid = keyring.active_kid(), "UCP response signing enabled");
        state.with_signing(keyring)
    } else {
        state
    };
    let inbound_keys = server_config
        .signing
        .to_verifying_keyring()
        .map_err(|e| format!("agent signing keys: {}", e))?;
    let state = match inbound_keys {
        Some(keys) => {
            tracing::info!(kids = ?keys.kids(), "inbound UCP signature verification enabled");
            state.with_inbound_verification(keys)
        }
        None => state,
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
            let _ = outbox_shutdown_tx.send(true);
            let _ = outbox_processor.await;
            match res {
                Ok(Ok(())) => Ok(()),
                Ok(Err(e)) => Err(e.to_string().into()),
                Err(e) => Err(format!("server task: {}", e).into()),
            }
        }
        _ = shutdown_signal() => {
            tracing::info!("shutdown signal received, draining");
            shutdown_flag.store(true, Ordering::SeqCst);
            // Stop taking new work, then let the processor flush what is queued
            // before the process goes away.
            let _ = outbox_shutdown_tx.send(true);
            let drain = tokio::time::timeout(
                server_config.outbox.drain_timeout(),
                outbox_processor,
            )
            .await;
            if drain.is_err() {
                tracing::warn!("outbox drain timed out; some messages remain queued");
            }
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
