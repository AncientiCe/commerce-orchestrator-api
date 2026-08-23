//! Server configuration: one source of truth for server, auth, persistence, and all downstream component APIs.
//! Load from config file (file-first) with env overrides; fail-fast validation in production.

use orchestrator_api::{SigningKeyring, VerifyingKeyring};
use std::path::Path;
use std::time::Duration;

/// Runtime profile: production enforces auth, persistence, and real adapters; development allows mocks and dev auth.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EnvProfile {
    Development,
    Production,
}

impl EnvProfile {
    pub fn from_env() -> Self {
        match std::env::var("ENV").as_deref() {
            Ok("production") | Ok("prod") => Self::Production,
            _ => Self::Development,
        }
    }

    pub fn is_production(self) -> bool {
        matches!(self, Self::Production)
    }
}

/// What the process was asked to do: serve traffic, or apply pending database
/// migrations and exit. The migration Job uses the latter so a rollout migrates
/// once, before the new replicas start.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunMode {
    Serve,
    MigrateOnly,
}

impl RunMode {
    pub fn from_env() -> Self {
        Self::resolve(
            std::env::args().skip(1),
            std::env::var("MIGRATE_ONLY").ok().as_deref(),
        )
    }

    pub fn resolve<I, S>(args: I, migrate_only_env: Option<&str>) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let flagged = args.into_iter().any(|arg| arg.as_ref() == "--migrate");
        let env_set = matches!(
            migrate_only_env.map(str::trim),
            Some("1") | Some("true") | Some("yes")
        );
        if flagged || env_set {
            Self::MigrateOnly
        } else {
            Self::Serve
        }
    }
}

/// Shared HTTP client policy for outbound calls to component APIs.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct HttpClientConfig {
    #[serde(default = "default_connect_timeout_secs")]
    pub connect_timeout_secs: u64,
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    #[serde(default = "default_retry_backoff_ms")]
    pub retry_backoff_ms: u64,
    #[serde(default)]
    pub circuit_breaker: CircuitBreakerSection,
}

/// Circuit breaker policy applied to every downstream provider.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CircuitBreakerSection {
    #[serde(default = "default_circuit_enabled")]
    pub enabled: bool,
    #[serde(default = "default_circuit_failure_threshold")]
    pub failure_threshold: u32,
    #[serde(default = "default_circuit_open_secs")]
    pub open_secs: u64,
    /// How long a half-open probe may stay in flight before another is admitted.
    #[serde(default = "default_circuit_probe_timeout_secs")]
    pub probe_timeout_secs: u64,
}

fn default_circuit_enabled() -> bool {
    true
}
fn default_circuit_failure_threshold() -> u32 {
    5
}
fn default_circuit_open_secs() -> u64 {
    30
}
fn default_circuit_probe_timeout_secs() -> u64 {
    60
}

impl Default for CircuitBreakerSection {
    fn default() -> Self {
        Self {
            enabled: default_circuit_enabled(),
            failure_threshold: default_circuit_failure_threshold(),
            open_secs: default_circuit_open_secs(),
            probe_timeout_secs: default_circuit_probe_timeout_secs(),
        }
    }
}

impl CircuitBreakerSection {
    fn to_config(&self) -> integration_adapters::CircuitBreakerConfig {
        integration_adapters::CircuitBreakerConfig {
            enabled: self.enabled,
            failure_threshold: self.failure_threshold.max(1),
            open_duration: Duration::from_secs(self.open_secs),
            probe_timeout: Duration::from_secs(self.probe_timeout_secs.max(1)),
        }
    }

    fn apply_env_overrides(&mut self) {
        if let Ok(v) = std::env::var("PROVIDER_CIRCUIT_ENABLED") {
            self.enabled = matches!(v.trim(), "1" | "true" | "yes");
        }
        if let Ok(v) = std::env::var("PROVIDER_CIRCUIT_FAILURE_THRESHOLD") {
            if let Ok(parsed) = v.trim().parse::<u32>() {
                self.failure_threshold = parsed;
            }
        }
        if let Ok(v) = std::env::var("PROVIDER_CIRCUIT_OPEN_SECS") {
            if let Ok(parsed) = v.trim().parse::<u64>() {
                self.open_secs = parsed;
            }
        }
        if let Ok(v) = std::env::var("PROVIDER_CIRCUIT_PROBE_TIMEOUT_SECS") {
            if let Ok(parsed) = v.trim().parse::<u64>() {
                self.probe_timeout_secs = parsed;
            }
        }
    }
}

fn default_connect_timeout_secs() -> u64 {
    5
}
fn default_timeout_secs() -> u64 {
    30
}
fn default_max_retries() -> u32 {
    3
}
fn default_retry_backoff_ms() -> u64 {
    100
}

impl Default for HttpClientConfig {
    fn default() -> Self {
        Self {
            connect_timeout_secs: default_connect_timeout_secs(),
            timeout_secs: default_timeout_secs(),
            max_retries: default_max_retries(),
            retry_backoff_ms: default_retry_backoff_ms(),
            circuit_breaker: CircuitBreakerSection::default(),
        }
    }
}

impl HttpClientConfig {
    pub fn to_client_config(&self) -> integration_adapters::ClientConfig {
        integration_adapters::ClientConfig {
            connect_timeout: Duration::from_secs(self.connect_timeout_secs),
            timeout: Duration::from_secs(self.timeout_secs),
            max_retries: self.max_retries,
            retry_backoff_ms: self.retry_backoff_ms,
            ..Default::default()
        }
    }

    /// Client policy for one provider, carrying that provider's credentials, TLS
    /// material, and its own circuit breaker.
    pub fn to_client_config_with_auth(
        &self,
        provider: Provider,
        auth: &ProviderAuthConfig,
    ) -> Result<integration_adapters::ClientConfig, String> {
        Ok(integration_adapters::ClientConfig {
            connect_timeout: Duration::from_secs(self.connect_timeout_secs),
            timeout: Duration::from_secs(self.timeout_secs),
            max_retries: self.max_retries,
            retry_backoff_ms: self.retry_backoff_ms,
            auth: auth.to_outbound_auth()?,
            tls: auth.to_tls_config()?,
            circuit_breaker: Some(std::sync::Arc::new(
                integration_adapters::CircuitBreaker::new(
                    provider.label(),
                    self.circuit_breaker.to_config(),
                ),
            )),
        })
    }
}

/// Credentials and TLS material for one downstream provider.
///
/// A provider section that sets any field replaces the shared default outright, so
/// credentials are never assembled from two places.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct ProviderAuthConfig {
    /// `none` (default), `bearer`, `api_key`, or `oauth2`.
    pub mode: Option<String>,
    pub token: Option<String>,
    pub header: Option<String>,
    pub value: Option<String>,
    pub oauth_token_url: Option<String>,
    pub oauth_client_id: Option<String>,
    pub oauth_client_secret: Option<String>,
    pub oauth_scope: Option<String>,
    /// `basic` (default) or `body`.
    pub oauth_client_auth: Option<String>,
    /// Path to a PEM file holding the client certificate and its private key.
    pub tls_client_cert_path: Option<String>,
    /// Path to a PEM file holding additional trust roots.
    pub tls_ca_path: Option<String>,
}

/// Header used for `api_key` mode when none is configured.
const DEFAULT_API_KEY_HEADER: &str = "X-API-Key";

impl ProviderAuthConfig {
    fn is_empty(&self) -> bool {
        self.mode.is_none()
            && self.token.is_none()
            && self.header.is_none()
            && self.value.is_none()
            && self.oauth_token_url.is_none()
            && self.oauth_client_id.is_none()
            && self.oauth_client_secret.is_none()
            && self.oauth_scope.is_none()
            && self.oauth_client_auth.is_none()
            && self.tls_client_cert_path.is_none()
            && self.tls_ca_path.is_none()
    }

    fn required<'a>(&self, field: Option<&'a String>, name: &str) -> Result<&'a str, String> {
        field
            .map(String::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                format!(
                    "{} required for provider auth mode '{}'",
                    name,
                    self.mode_str()
                )
            })
    }

    fn mode_str(&self) -> String {
        self.mode
            .clone()
            .unwrap_or_else(|| "none".to_string())
            .trim()
            .to_ascii_lowercase()
    }

    /// Build the outbound credential for this provider.
    pub fn to_outbound_auth(&self) -> Result<integration_adapters::OutboundAuth, String> {
        use integration_adapters::{OAuth2ClientAuth, OAuth2ClientCredentials, OutboundAuth};
        match self.mode_str().as_str() {
            "none" | "" => Ok(OutboundAuth::None),
            "bearer" => Ok(OutboundAuth::bearer(
                self.required(self.token.as_ref(), "token")?,
            )),
            "api_key" | "apikey" => {
                let header = self
                    .header
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(DEFAULT_API_KEY_HEADER);
                Ok(OutboundAuth::api_key(
                    header,
                    self.required(self.value.as_ref(), "value")?,
                ))
            }
            "oauth2" | "oauth" => Ok(OutboundAuth::oauth2(OAuth2ClientCredentials {
                token_url: self
                    .required(self.oauth_token_url.as_ref(), "oauth_token_url")?
                    .to_string(),
                client_id: self
                    .required(self.oauth_client_id.as_ref(), "oauth_client_id")?
                    .to_string(),
                client_secret: self
                    .required(self.oauth_client_secret.as_ref(), "oauth_client_secret")?
                    .to_string(),
                scope: self.oauth_scope.clone().filter(|s| !s.is_empty()),
                client_auth: match self.oauth_client_auth.as_deref() {
                    Some(v) if !v.is_empty() => OAuth2ClientAuth::parse(v)?,
                    _ => OAuth2ClientAuth::default(),
                },
            })),
            other => Err(format!(
                "unsupported provider auth mode '{}' (expected none, bearer, api_key, or oauth2)",
                other
            )),
        }
    }

    /// Read TLS material from disk. Paths are read eagerly so a bad path fails at startup.
    pub fn to_tls_config(&self) -> Result<integration_adapters::TlsConfig, String> {
        let read = |path: &str, label: &str| -> Result<Vec<u8>, String> {
            std::fs::read(path).map_err(|e| format!("read {} from {}: {}", label, path, e))
        };
        Ok(integration_adapters::TlsConfig {
            client_identity_pem: self
                .tls_client_cert_path
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(|p| read(p, "TLS client certificate"))
                .transpose()?,
            ca_bundle_pem: self
                .tls_ca_path
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(|p| read(p, "TLS CA bundle"))
                .transpose()?,
        })
    }

    /// Apply `<PREFIX>_AUTH_*`, `<PREFIX>_OAUTH_*`, and `<PREFIX>_TLS_*` environment overrides.
    pub fn apply_env_overrides(&mut self, prefix: &str) {
        for (suffix, field) in [
            ("AUTH_MODE", &mut self.mode),
            ("AUTH_TOKEN", &mut self.token),
            ("AUTH_HEADER", &mut self.header),
            ("AUTH_VALUE", &mut self.value),
            ("OAUTH_TOKEN_URL", &mut self.oauth_token_url),
            ("OAUTH_CLIENT_ID", &mut self.oauth_client_id),
            ("OAUTH_CLIENT_SECRET", &mut self.oauth_client_secret),
            ("OAUTH_SCOPE", &mut self.oauth_scope),
            ("OAUTH_CLIENT_AUTH", &mut self.oauth_client_auth),
            ("TLS_CLIENT_CERT_PATH", &mut self.tls_client_cert_path),
            ("TLS_CA_PATH", &mut self.tls_ca_path),
        ] {
            if let Ok(v) = std::env::var(format!("{}_{}", prefix, suffix)) {
                let t = v.trim().to_string();
                if !t.is_empty() {
                    *field = Some(t);
                }
            }
        }
    }
}

/// Per-provider credentials, with a shared default for the common case of one
/// internal credential covering every provider.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct ProviderAuthSection {
    #[serde(default)]
    pub default: ProviderAuthConfig,
    #[serde(default)]
    pub catalog: ProviderAuthConfig,
    #[serde(default)]
    pub pricing: ProviderAuthConfig,
    #[serde(default)]
    pub tax: ProviderAuthConfig,
    #[serde(default)]
    pub geo: ProviderAuthConfig,
    #[serde(default)]
    pub payment: ProviderAuthConfig,
    #[serde(default)]
    pub receipt: ProviderAuthConfig,
    #[serde(default)]
    pub fulfillment: ProviderAuthConfig,
    #[serde(default)]
    pub payment_delegation: ProviderAuthConfig,
    #[serde(default)]
    pub identity_link: ProviderAuthConfig,
}

impl ProviderAuthSection {
    /// Credentials for one provider: its own section when set, otherwise the shared default.
    pub fn for_provider(&self, provider: Provider) -> &ProviderAuthConfig {
        let specific = match provider {
            Provider::Catalog => &self.catalog,
            Provider::Pricing => &self.pricing,
            Provider::Tax => &self.tax,
            Provider::Geo => &self.geo,
            Provider::Payment => &self.payment,
            Provider::Receipt => &self.receipt,
            Provider::Fulfillment => &self.fulfillment,
            Provider::PaymentDelegation => &self.payment_delegation,
            Provider::IdentityLink => &self.identity_link,
        };
        if specific.is_empty() {
            &self.default
        } else {
            specific
        }
    }

    pub fn apply_env_overrides(&mut self) {
        self.default.apply_env_overrides("PROVIDER");
        for provider in Provider::ALL {
            let section = match provider {
                Provider::Catalog => &mut self.catalog,
                Provider::Pricing => &mut self.pricing,
                Provider::Tax => &mut self.tax,
                Provider::Geo => &mut self.geo,
                Provider::Payment => &mut self.payment,
                Provider::Receipt => &mut self.receipt,
                Provider::Fulfillment => &mut self.fulfillment,
                Provider::PaymentDelegation => &mut self.payment_delegation,
                Provider::IdentityLink => &mut self.identity_link,
            };
            section.apply_env_overrides(provider.env_prefix());
        }
    }
}

/// Downstream provider identity, used for config lookup and metric labels.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Provider {
    Catalog,
    Pricing,
    Tax,
    Geo,
    Payment,
    Receipt,
    Fulfillment,
    PaymentDelegation,
    IdentityLink,
}

impl Provider {
    pub const ALL: [Provider; 9] = [
        Provider::Catalog,
        Provider::Pricing,
        Provider::Tax,
        Provider::Geo,
        Provider::Payment,
        Provider::Receipt,
        Provider::Fulfillment,
        Provider::PaymentDelegation,
        Provider::IdentityLink,
    ];

    pub fn env_prefix(self) -> &'static str {
        match self {
            Provider::Catalog => "CATALOG",
            Provider::Pricing => "PRICING",
            Provider::Tax => "TAX",
            Provider::Geo => "GEO",
            Provider::Payment => "PAYMENT",
            Provider::Receipt => "RECEIPT",
            Provider::Fulfillment => "FULFILLMENT",
            Provider::PaymentDelegation => "PAYMENT_DELEGATION",
            Provider::IdentityLink => "IDENTITY_LINK",
        }
    }

    /// Lower-case name used as a metric label.
    pub fn label(self) -> &'static str {
        match self {
            Provider::Catalog => "catalog",
            Provider::Pricing => "pricing",
            Provider::Tax => "tax",
            Provider::Geo => "geo",
            Provider::Payment => "payment",
            Provider::Receipt => "receipt",
            Provider::Fulfillment => "fulfillment",
            Provider::PaymentDelegation => "payment_delegation",
            Provider::IdentityLink => "identity_link",
        }
    }
}

/// Downstream component API base URLs. Each is the root URL for that service (e.g. https://catalog.example.com).
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct ComponentsConfig {
    pub catalog_base_url: Option<String>,
    pub pricing_base_url: Option<String>,
    pub tax_base_url: Option<String>,
    pub geo_base_url: Option<String>,
    pub payment_base_url: Option<String>,
    pub receipt_base_url: Option<String>,
    /// Optional shipping/pickup rating service. Without it the built-in static
    /// rate table is used, which is only suitable for development.
    pub fulfillment_base_url: Option<String>,
    /// Optional PSP delegation service. Without it `POST /acp/delegate_payment`
    /// returns `501` and the capability is not advertised.
    pub payment_delegation_base_url: Option<String>,
    /// Optional identity system. Without it identity linking returns `501` and
    /// the capability is not advertised.
    pub identity_link_base_url: Option<String>,
}

impl ComponentsConfig {
    fn trim_opt(s: Option<String>) -> Option<String> {
        s.and_then(|s| {
            let t = s.trim().to_string();
            if t.is_empty() {
                None
            } else {
                Some(t)
            }
        })
    }

    /// Apply env overrides. Env vars: CATALOG_BASE_URL, PRICING_BASE_URL, TAX_BASE_URL, GEO_BASE_URL, PAYMENT_BASE_URL, RECEIPT_BASE_URL.
    pub fn apply_env_overrides(&mut self) {
        for (key, opt) in [
            ("CATALOG_BASE_URL", &mut self.catalog_base_url),
            ("PRICING_BASE_URL", &mut self.pricing_base_url),
            ("TAX_BASE_URL", &mut self.tax_base_url),
            ("GEO_BASE_URL", &mut self.geo_base_url),
            ("PAYMENT_BASE_URL", &mut self.payment_base_url),
            ("RECEIPT_BASE_URL", &mut self.receipt_base_url),
            ("FULFILLMENT_BASE_URL", &mut self.fulfillment_base_url),
            (
                "PAYMENT_DELEGATION_BASE_URL",
                &mut self.payment_delegation_base_url,
            ),
            ("IDENTITY_LINK_BASE_URL", &mut self.identity_link_base_url),
        ] {
            if let Ok(v) = std::env::var(key) {
                *opt = Self::trim_opt(Some(v));
            }
        }
    }

    /// Require all six component URLs to be set; return error if any missing.
    pub fn require_all(&self) -> Result<ResolvedComponents, String> {
        let catalog = self
            .catalog_base_url
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or("CATALOG_BASE_URL (or components.catalog_base_url) required")?
            .to_string();
        let pricing = self
            .pricing_base_url
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or("PRICING_BASE_URL (or components.pricing_base_url) required")?
            .to_string();
        let tax = self
            .tax_base_url
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or("TAX_BASE_URL (or components.tax_base_url) required")?
            .to_string();
        let geo = self
            .geo_base_url
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or("GEO_BASE_URL (or components.geo_base_url) required")?
            .to_string();
        let payment = self
            .payment_base_url
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or("PAYMENT_BASE_URL (or components.payment_base_url) required")?
            .to_string();
        let receipt = self
            .receipt_base_url
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or("RECEIPT_BASE_URL (or components.receipt_base_url) required")?
            .to_string();
        Ok(ResolvedComponents {
            catalog_base_url: catalog,
            pricing_base_url: pricing,
            tax_base_url: tax,
            geo_base_url: geo,
            payment_base_url: payment,
            receipt_base_url: receipt,
            fulfillment_base_url: self.fulfillment_base_url.clone().filter(|s| !s.is_empty()),
            payment_delegation_base_url: self
                .payment_delegation_base_url
                .clone()
                .filter(|s| !s.is_empty()),
            identity_link_base_url: self
                .identity_link_base_url
                .clone()
                .filter(|s| !s.is_empty()),
        })
    }
}

/// Resolved component URLs (all present).
#[derive(Clone, Debug)]
pub struct ResolvedComponents {
    pub catalog_base_url: String,
    pub pricing_base_url: String,
    pub tax_base_url: String,
    pub geo_base_url: String,
    pub payment_base_url: String,
    pub receipt_base_url: String,
    pub fulfillment_base_url: Option<String>,
    pub payment_delegation_base_url: Option<String>,
    pub identity_link_base_url: Option<String>,
}

/// Full server configuration. Can be loaded from YAML and overridden by env.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct ServerConfig {
    #[serde(default)]
    pub server: ServerSection,
    #[serde(default)]
    pub auth: AuthSection,
    #[serde(default)]
    pub persistence: PersistenceSection,
    #[serde(default)]
    pub components: ComponentsConfig,
    #[serde(default)]
    pub http_client: HttpClientConfig,
    #[serde(default)]
    pub provider_auth: ProviderAuthSection,
    #[serde(default)]
    pub outbox: OutboxSection,
    #[serde(default)]
    pub signing: SigningSection,
}

/// UCP request/response signing material.
///
/// There is no default key: a shared, published default would let anyone forge
/// our signatures, so an unconfigured deployment simply does not sign, and
/// production refuses to start until real material is supplied.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct SigningSection {
    /// Key id advertised in discovery and sent in the `Signature` header.
    #[serde(default)]
    pub key_id: Option<String>,
    /// Base64-encoded 32-byte Ed25519 seed for the active key.
    #[serde(default)]
    pub key: Option<String>,
    /// Superseded keys still trusted for verification, as `kid:seed` pairs.
    #[serde(default)]
    pub previous_keys: Vec<String>,
    /// Public keys of agents whose requests we require to be signed, as
    /// `kid:base64-public-key` pairs. Empty means inbound signatures are optional.
    #[serde(default)]
    pub agent_keys: Vec<String>,
}

impl SigningSection {
    pub fn apply_env_overrides(&mut self) {
        if let Ok(v) = std::env::var("UCP_SIGNING_KEY_ID") {
            let t = v.trim().to_string();
            if !t.is_empty() {
                self.key_id = Some(t);
            }
        }
        if let Ok(v) = std::env::var("UCP_SIGNING_KEY") {
            let t = v.trim().to_string();
            if !t.is_empty() {
                self.key = Some(t);
            }
        }
        if let Ok(v) = std::env::var("UCP_SIGNING_PREVIOUS_KEYS") {
            let t = v.trim().to_string();
            if !t.is_empty() {
                self.previous_keys = split_pairs(&t);
            }
        }
        if let Ok(v) = std::env::var("UCP_AGENT_KEYS") {
            let t = v.trim().to_string();
            if !t.is_empty() {
                self.agent_keys = split_pairs(&t);
            }
        }
    }

    /// Build the keyring, or `None` when this deployment does not sign.
    pub fn to_keyring(&self) -> Result<Option<SigningKeyring>, String> {
        let (key_id, key) = match (
            self.key_id.as_deref().filter(|s| !s.is_empty()),
            self.key.as_deref().filter(|s| !s.is_empty()),
        ) {
            (Some(id), Some(key)) => (id, key),
            (None, None) => return Ok(None),
            (Some(_), None) => {
                return Err("UCP_SIGNING_KEY is required when UCP_SIGNING_KEY_ID is set".to_string())
            }
            (None, Some(_)) => {
                return Err("UCP_SIGNING_KEY_ID is required when UCP_SIGNING_KEY is set".to_string())
            }
        };
        let previous = self
            .previous_keys
            .iter()
            .map(|pair| {
                pair.split_once(':')
                    .map(|(kid, seed)| (kid.trim(), seed.trim()))
                    .ok_or_else(|| {
                        format!("previous signing key '{pair}' must be in 'kid:seed' form")
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        SigningKeyring::from_seeds(key_id, key, &previous)
            .map(Some)
            .map_err(|e| e.to_string())
    }

    /// Build the inbound verification keyring, or `None` when agent signatures
    /// are not required by this deployment.
    pub fn to_verifying_keyring(&self) -> Result<Option<VerifyingKeyring>, String> {
        if self.agent_keys.is_empty() {
            return Ok(None);
        }
        let parsed = self
            .agent_keys
            .iter()
            .map(|pair| {
                pair.split_once(':')
                    .map(|(kid, key)| (kid.trim(), key.trim()))
                    .ok_or_else(|| {
                        format!("agent signing key '{pair}' must be in 'kid:public_key' form")
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        VerifyingKeyring::from_public_keys(&parsed)
            .map(Some)
            .map_err(|e| e.to_string())
    }
}

fn split_pairs(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|pair| pair.trim().to_string())
        .filter(|pair| !pair.is_empty())
        .collect()
}

/// Background outbox processor tuning.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct OutboxSection {
    #[serde(default = "default_outbox_interval_ms")]
    pub interval_ms: u64,
    #[serde(default = "default_outbox_batch_size")]
    pub batch_size: usize,
    #[serde(default = "default_outbox_max_attempts")]
    pub max_attempts: u32,
    #[serde(default = "default_outbox_drain_secs")]
    pub drain_timeout_secs: u64,
    /// Pause after a failed delivery, doubled per attempt and capped by
    /// `max_retry_backoff_secs`. Retrying without a pause spends every attempt
    /// in milliseconds and dead-letters messages a healthy retry would deliver.
    #[serde(default = "default_outbox_retry_backoff_ms")]
    pub retry_backoff_ms: u64,
    #[serde(default = "default_outbox_max_retry_backoff_secs")]
    pub max_retry_backoff_secs: u64,
}

fn default_outbox_interval_ms() -> u64 {
    500
}
fn default_outbox_batch_size() -> usize {
    32
}
fn default_outbox_max_attempts() -> u32 {
    5
}
fn default_outbox_drain_secs() -> u64 {
    10
}
fn default_outbox_retry_backoff_ms() -> u64 {
    500
}
fn default_outbox_max_retry_backoff_secs() -> u64 {
    30
}

impl Default for OutboxSection {
    fn default() -> Self {
        Self {
            interval_ms: default_outbox_interval_ms(),
            batch_size: default_outbox_batch_size(),
            max_attempts: default_outbox_max_attempts(),
            drain_timeout_secs: default_outbox_drain_secs(),
            retry_backoff_ms: default_outbox_retry_backoff_ms(),
            max_retry_backoff_secs: default_outbox_max_retry_backoff_secs(),
        }
    }
}

impl OutboxSection {
    pub fn to_processor_config(&self) -> orchestrator_runtime::OutboxProcessorConfig {
        orchestrator_runtime::OutboxProcessorConfig {
            interval: Duration::from_millis(self.interval_ms),
            batch_size: self.batch_size.max(1),
            max_attempts: self.max_attempts.max(1),
            drain_timeout: self.drain_timeout(),
            retry_backoff: Duration::from_millis(self.retry_backoff_ms.max(1)),
            max_retry_backoff: Duration::from_secs(self.max_retry_backoff_secs.max(1)),
        }
    }

    pub fn drain_timeout(&self) -> Duration {
        Duration::from_secs(self.drain_timeout_secs)
    }

    fn apply_env_overrides(&mut self) {
        if let Ok(v) = std::env::var("OUTBOX_INTERVAL_MS") {
            if let Ok(parsed) = v.trim().parse() {
                self.interval_ms = parsed;
            }
        }
        if let Ok(v) = std::env::var("OUTBOX_BATCH_SIZE") {
            if let Ok(parsed) = v.trim().parse() {
                self.batch_size = parsed;
            }
        }
        if let Ok(v) = std::env::var("OUTBOX_MAX_ATTEMPTS") {
            if let Ok(parsed) = v.trim().parse() {
                self.max_attempts = parsed;
            }
        }
        if let Ok(v) = std::env::var("OUTBOX_DRAIN_TIMEOUT_SECS") {
            if let Ok(parsed) = v.trim().parse() {
                self.drain_timeout_secs = parsed;
            }
        }
        if let Ok(v) = std::env::var("OUTBOX_RETRY_BACKOFF_MS") {
            if let Ok(parsed) = v.trim().parse() {
                self.retry_backoff_ms = parsed;
            }
        }
        if let Ok(v) = std::env::var("OUTBOX_MAX_RETRY_BACKOFF_SECS") {
            if let Ok(parsed) = v.trim().parse() {
                self.max_retry_backoff_secs = parsed;
            }
        }
    }
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct ServerSection {
    #[serde(default = "default_bind_addr")]
    pub bind_addr: String,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    /// Public base URL for this orchestrator (e.g. https://orchestrator.example.com). Used for discovery manifest rest_endpoint.
    pub public_base_url: Option<String>,
}

fn default_bind_addr() -> String {
    "0.0.0.0:8080".to_string()
}
fn default_log_level() -> String {
    "info".to_string()
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct AuthSection {
    pub mode: Option<String>,
    pub bearer_token: Option<String>,
    pub jwt_hs256_secret: Option<String>,
    pub trusted_issuers: Option<String>,
    pub tenant_id: Option<String>,
    pub caller_id: Option<String>,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct PersistenceSection {
    pub path: Option<String>,
    pub database_url: Option<String>,
}

impl ServerConfig {
    /// Load config: file-first (if path exists) then apply env overrides.
    pub fn load(config_path: Option<&Path>) -> Result<Self, String> {
        let mut config = if let Some(p) = config_path {
            if p.exists() {
                let s =
                    std::fs::read_to_string(p).map_err(|e| format!("read config file: {}", e))?;
                serde_yaml::from_str(&s).map_err(|e| format!("parse config: {}", e))?
            } else {
                Self::default()
            }
        } else {
            Self::default()
        };
        config.apply_env_overrides();
        Ok(config)
    }

    /// Apply env overrides to all sections.
    pub fn apply_env_overrides(&mut self) {
        if let Ok(v) = std::env::var("BIND_ADDR") {
            let t = v.trim().to_string();
            if !t.is_empty() {
                self.server.bind_addr = t;
            }
        }
        if let Ok(v) = std::env::var("PUBLIC_BASE_URL") {
            let t = v.trim().to_string();
            if !t.is_empty() {
                self.server.public_base_url = Some(t);
            }
        }
        if let Ok(v) = std::env::var("RUST_LOG") {
            let t = v.trim().to_string();
            if !t.is_empty() {
                self.server.log_level = t;
            }
        }
        if let Ok(v) = std::env::var("PERSISTENCE_PATH").or_else(|_| std::env::var("DATA_DIR")) {
            let t = v.trim().to_string();
            if !t.is_empty() {
                self.persistence.path = Some(t);
            }
        }
        if let Ok(v) = std::env::var("DATABASE_URL") {
            let t = v.trim().to_string();
            if !t.is_empty() {
                self.persistence.database_url = Some(t);
            }
        }
        if let Ok(v) = std::env::var("AUTH_BEARER_TOKEN") {
            let t = v.trim().to_string();
            if !t.is_empty() {
                self.auth.bearer_token = Some(t);
            }
        }
        if let Ok(v) = std::env::var("AUTH_MODE") {
            let t = v.trim().to_string();
            if !t.is_empty() {
                self.auth.mode = Some(t);
            }
        }
        if let Ok(v) = std::env::var("AUTH_JWT_HS256_SECRET") {
            let t = v.trim().to_string();
            if !t.is_empty() {
                self.auth.jwt_hs256_secret = Some(t);
            }
        }
        if let Ok(v) = std::env::var("AP2_TRUSTED_ISSUERS") {
            let t = v.trim().to_string();
            if !t.is_empty() {
                self.auth.trusted_issuers = Some(t);
            }
        }
        if let Ok(v) = std::env::var("AUTH_TENANT_ID") {
            let t = v.trim().to_string();
            if !t.is_empty() {
                self.auth.tenant_id = Some(t);
            }
        }
        if let Ok(v) = std::env::var("AUTH_CALLER_ID") {
            let t = v.trim().to_string();
            if !t.is_empty() {
                self.auth.caller_id = Some(t);
            }
        }
        self.components.apply_env_overrides();
        self.provider_auth.apply_env_overrides();
        self.http_client.circuit_breaker.apply_env_overrides();
        self.outbox.apply_env_overrides();
        self.signing.apply_env_overrides();
    }

    /// Validate for production: public base URL, database URL, auth token, and all six
    /// component URLs required.
    pub fn require_production(&self) -> Result<ProductionConfig, String> {
        let public_base_url = self
            .server
            .public_base_url
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or("PUBLIC_BASE_URL (or server.public_base_url) required in production")?
            .to_string();
        let database_url = self
            .persistence
            .database_url
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or("DATABASE_URL (or persistence.database_url) required in production")?
            .to_string();
        let auth_mode = self
            .auth
            .mode
            .clone()
            .unwrap_or_else(|| "static".to_string())
            .to_lowercase();
        let auth_token = if auth_mode == "jwt" {
            self.auth
                .jwt_hs256_secret
                .as_deref()
                .filter(|s| !s.is_empty())
                .ok_or("AUTH_JWT_HS256_SECRET (or auth.jwt_hs256_secret) required in production when AUTH_MODE=jwt")?
                .to_string()
        } else {
            self.auth
                .bearer_token
                .as_deref()
                .filter(|s| !s.is_empty())
                .ok_or("AUTH_BEARER_TOKEN (or auth.bearer_token) required in production")?
                .to_string()
        };
        let components = self.components.require_all()?;
        // Surface credential and TLS misconfiguration at startup rather than on first call.
        for provider in Provider::ALL {
            let auth = self.provider_auth.for_provider(provider);
            auth.to_outbound_auth()
                .and_then(|_| auth.to_tls_config().map(|_| ()))
                .map_err(|e| format!("{} provider auth: {}", provider.env_prefix(), e))?;
        }
        let signing = self.signing.to_keyring()?.ok_or(
            "UCP_SIGNING_KEY_ID and UCP_SIGNING_KEY (or signing.key_id/signing.key) required in production",
        )?;
        Ok(ProductionConfig {
            public_base_url,
            database_url,
            signing,
            auth_mode,
            auth_token,
            trusted_issuers: self.auth.trusted_issuers.clone().unwrap_or_default(),
            auth_tenant_id: self
                .auth
                .tenant_id
                .clone()
                .unwrap_or_else(|| "prod".to_string()),
            auth_caller_id: self
                .auth
                .caller_id
                .clone()
                .unwrap_or_else(|| "prod".to_string()),
            components,
            http_client: self.http_client.clone(),
            provider_auth: self.provider_auth.clone(),
        })
    }
}

/// Validated production config: all required fields present.
#[derive(Clone, Debug)]
pub struct ProductionConfig {
    pub public_base_url: String,
    pub database_url: String,
    /// Production always signs; the server refuses to start without real key material.
    pub signing: SigningKeyring,
    pub auth_mode: String,
    pub auth_token: String,
    pub trusted_issuers: String,
    pub auth_tenant_id: String,
    pub auth_caller_id: String,
    pub components: ResolvedComponents,
    pub http_client: HttpClientConfig,
    pub provider_auth: ProviderAuthSection,
}

impl ProductionConfig {
    /// Client policy for one provider, including its credentials and TLS material.
    pub fn client_config_for(
        &self,
        provider: Provider,
    ) -> Result<integration_adapters::ClientConfig, String> {
        self.http_client
            .to_client_config_with_auth(provider, self.provider_auth.for_provider(provider))
            .map_err(|e| format!("{} provider auth: {}", provider.env_prefix(), e))
    }
}

/// Resolve config path from CONFIG_FILE env or default "config.yaml" in current dir.
pub fn default_config_path() -> Option<std::path::PathBuf> {
    std::env::var("CONFIG_FILE")
        .ok()
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .map(|cwd| cwd.join("config.yaml"))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn production_ready_config() -> ServerConfig {
        let mut config = ServerConfig::default();
        config.server.public_base_url = Some("https://orchestrator.example.com".into());
        config.persistence.database_url =
            Some("postgres://orchestrator:secret@localhost:5432/orchestrator".into());
        config.auth.bearer_token = Some("token".into());
        config.components.catalog_base_url = Some("http://catalog:8080".into());
        config.components.pricing_base_url = Some("http://pricing:8080".into());
        config.components.tax_base_url = Some("http://tax:8080".into());
        config.components.geo_base_url = Some("http://geo:8080".into());
        config.components.payment_base_url = Some("http://payment:8080".into());
        config.components.receipt_base_url = Some("http://receipt:8080".into());
        config.signing.key_id = Some("orch-2026-a".into());
        config.signing.key = Some(TEST_SEED.into());
        config
    }

    /// Deterministic 32-byte seed, base64url, for tests only.
    const TEST_SEED: &str = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8";
    const OTHER_TEST_SEED: &str = "IB8eHRwbGhkYFxYVFBMSERAPDg0MCwoJCAcGBQQDAgE";

    #[test]
    fn default_invocation_serves_traffic() {
        assert_eq!(RunMode::resolve(Vec::<String>::new(), None), RunMode::Serve);
    }

    #[test]
    fn the_migrate_flag_selects_migrate_only_mode() {
        assert_eq!(
            RunMode::resolve(vec!["--migrate".to_string()], None),
            RunMode::MigrateOnly
        );
    }

    #[test]
    fn migrate_only_can_also_be_selected_by_environment() {
        for truthy in ["1", "true", "yes"] {
            assert_eq!(
                RunMode::resolve(Vec::<String>::new(), Some(truthy)),
                RunMode::MigrateOnly,
                "MIGRATE_ONLY={truthy} should run migrations only"
            );
        }
        assert_eq!(
            RunMode::resolve(Vec::<String>::new(), Some("false")),
            RunMode::Serve
        );
    }

    #[test]
    fn require_production_fails_when_signing_key_missing() {
        let mut config = production_ready_config();
        config.signing.key = None;
        config.signing.key_id = None;
        let err = config.require_production().unwrap_err();
        assert!(
            err.contains("UCP_SIGNING_KEY"),
            "production must fail closed without signing material, got: {err}"
        );
    }

    #[test]
    fn require_production_rejects_unusable_signing_key() {
        let mut config = production_ready_config();
        config.signing.key = Some("not-a-key".into());
        let err = config.require_production().unwrap_err();
        assert!(err.contains("orch-2026-a"), "got: {err}");
    }

    #[test]
    fn a_key_id_without_a_key_is_a_configuration_error() {
        let config = SigningSection {
            key_id: Some("orch-2026-a".into()),
            ..Default::default()
        };
        let err = config.to_keyring().unwrap_err();
        assert!(err.contains("UCP_SIGNING_KEY"), "got: {err}");
    }

    #[test]
    fn no_signing_configuration_means_no_keyring() {
        assert!(SigningSection::default().to_keyring().unwrap().is_none());
    }

    #[test]
    fn previous_keys_are_parsed_as_kid_seed_pairs() {
        let section = SigningSection {
            key_id: Some("orch-2026-b".into()),
            key: Some(OTHER_TEST_SEED.into()),
            previous_keys: vec![format!("orch-2026-a:{TEST_SEED}")],
            ..Default::default()
        };
        let keyring = section.to_keyring().unwrap().expect("keyring");
        assert_eq!(keyring.active_kid(), "orch-2026-b");
        assert_eq!(keyring.public_jwks().len(), 2);
    }

    #[test]
    fn agent_keys_build_an_inbound_verifying_keyring() {
        let agent = SigningKeyring::from_seeds("agent-1", TEST_SEED, &[]).expect("agent keyring");
        let x = agent.public_jwks()[0].x.clone().expect("x");
        let section = SigningSection {
            agent_keys: vec![format!("agent-1:{x}")],
            ..Default::default()
        };
        let keyring = section
            .to_verifying_keyring()
            .expect("valid")
            .expect("keyring");
        assert_eq!(keyring.kids(), vec!["agent-1"]);
    }

    #[test]
    fn no_agent_keys_means_inbound_signatures_are_not_required() {
        assert!(SigningSection::default()
            .to_verifying_keyring()
            .unwrap()
            .is_none());
    }

    #[test]
    fn a_malformed_agent_key_is_rejected() {
        let section = SigningSection {
            agent_keys: vec!["missing-separator".into()],
            ..Default::default()
        };
        let err = section.to_verifying_keyring().unwrap_err();
        assert!(err.contains("kid:public_key"), "got: {err}");
    }

    #[test]
    fn a_malformed_previous_key_is_rejected() {
        let section = SigningSection {
            key_id: Some("orch-2026-b".into()),
            key: Some(OTHER_TEST_SEED.into()),
            previous_keys: vec!["missing-separator".into()],
            ..Default::default()
        };
        let err = section.to_keyring().unwrap_err();
        assert!(err.contains("kid:seed"), "got: {err}");
    }

    #[test]
    fn require_production_succeeds_when_all_component_urls_set() {
        let config = production_ready_config();
        let prod = config.require_production().unwrap();
        assert_eq!(prod.public_base_url, "https://orchestrator.example.com");
        assert_eq!(
            prod.database_url,
            "postgres://orchestrator:secret@localhost:5432/orchestrator"
        );
        assert_eq!(prod.auth_mode, "static");
        assert_eq!(prod.auth_token, "token");
        assert_eq!(prod.components.catalog_base_url, "http://catalog:8080");
        assert_eq!(prod.components.receipt_base_url, "http://receipt:8080");
    }

    #[test]
    fn require_production_fails_when_component_url_missing() {
        let mut config = production_ready_config();
        config.components.pricing_base_url = None;
        let err = config.require_production().unwrap_err();
        assert!(err.contains("PRICING_BASE_URL") || err.contains("pricing_base_url"));
    }

    #[test]
    fn require_production_fails_when_persistence_missing() {
        let mut config = production_ready_config();
        config.persistence.database_url = None;
        let err = config.require_production().unwrap_err();
        assert!(err.contains("DATABASE_URL") || err.contains("database_url"));
    }

    #[test]
    fn require_production_fails_when_auth_token_missing() {
        let mut config = production_ready_config();
        config.auth.bearer_token = None;
        let err = config.require_production().unwrap_err();
        assert!(err.contains("AUTH_BEARER_TOKEN") || err.contains("bearer_token"));
    }

    #[test]
    fn require_production_uses_jwt_secret_when_mode_is_jwt() {
        let mut config = production_ready_config();
        config.auth.mode = Some("jwt".into());
        config.auth.jwt_hs256_secret = Some("secret".into());
        config.auth.bearer_token = None;
        let prod = config.require_production().unwrap();
        assert_eq!(prod.auth_mode, "jwt");
        assert_eq!(prod.auth_token, "secret");
    }

    #[test]
    fn require_production_fails_when_public_base_url_missing() {
        let mut config = production_ready_config();
        config.server.public_base_url = None;
        let err = config.require_production().unwrap_err();
        assert!(err.contains("PUBLIC_BASE_URL") || err.contains("public_base_url"));
    }

    #[test]
    fn provider_auth_defaults_to_none() {
        let auth = ProviderAuthConfig::default();
        assert!(auth.to_outbound_auth().unwrap().is_none());
    }

    #[test]
    fn provider_auth_builds_bearer() {
        let auth = ProviderAuthConfig {
            mode: Some("bearer".into()),
            token: Some("t".into()),
            ..Default::default()
        };
        assert!(matches!(
            auth.to_outbound_auth().unwrap(),
            integration_adapters::OutboundAuth::Bearer { .. }
        ));
    }

    #[test]
    fn provider_auth_bearer_requires_token() {
        let auth = ProviderAuthConfig {
            mode: Some("bearer".into()),
            ..Default::default()
        };
        let err = auth.to_outbound_auth().unwrap_err();
        assert!(err.contains("token"), "unexpected error: {}", err);
    }

    #[test]
    fn provider_auth_api_key_defaults_header() {
        let auth = ProviderAuthConfig {
            mode: Some("api_key".into()),
            value: Some("k".into()),
            ..Default::default()
        };
        match auth.to_outbound_auth().unwrap() {
            integration_adapters::OutboundAuth::ApiKey { header, .. } => {
                assert_eq!(header, DEFAULT_API_KEY_HEADER);
            }
            other => panic!("expected api key auth, got {:?}", other),
        }
    }

    #[test]
    fn provider_auth_oauth2_requires_token_url() {
        let auth = ProviderAuthConfig {
            mode: Some("oauth2".into()),
            oauth_client_id: Some("cid".into()),
            oauth_client_secret: Some("secret".into()),
            ..Default::default()
        };
        let err = auth.to_outbound_auth().unwrap_err();
        assert!(err.contains("oauth_token_url"), "unexpected error: {}", err);
    }

    #[test]
    fn provider_auth_rejects_unknown_mode() {
        let auth = ProviderAuthConfig {
            mode: Some("kerberos".into()),
            ..Default::default()
        };
        let err = auth.to_outbound_auth().unwrap_err();
        assert!(err.contains("kerberos"), "unexpected error: {}", err);
    }

    #[test]
    fn provider_specific_auth_overrides_default() {
        let section = ProviderAuthSection {
            default: ProviderAuthConfig {
                mode: Some("bearer".into()),
                token: Some("shared".into()),
                ..Default::default()
            },
            payment: ProviderAuthConfig {
                mode: Some("api_key".into()),
                value: Some("payment-key".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(matches!(
            section
                .for_provider(Provider::Payment)
                .to_outbound_auth()
                .unwrap(),
            integration_adapters::OutboundAuth::ApiKey { .. }
        ));
        assert!(matches!(
            section
                .for_provider(Provider::Catalog)
                .to_outbound_auth()
                .unwrap(),
            integration_adapters::OutboundAuth::Bearer { .. }
        ));
    }

    #[test]
    fn require_production_rejects_invalid_provider_auth() {
        let mut config = production_ready_config();
        config.provider_auth.default = ProviderAuthConfig {
            mode: Some("bearer".into()),
            ..Default::default()
        };
        let err = config.require_production().unwrap_err();
        assert!(err.contains("provider auth"), "unexpected error: {}", err);
    }

    #[test]
    fn require_production_rejects_unreadable_tls_material() {
        let mut config = production_ready_config();
        config.provider_auth.default = ProviderAuthConfig {
            tls_ca_path: Some("./does-not-exist-ca.pem".into()),
            ..Default::default()
        };
        let err = config.require_production().unwrap_err();
        assert!(err.contains("TLS CA bundle"), "unexpected error: {}", err);
    }
}
