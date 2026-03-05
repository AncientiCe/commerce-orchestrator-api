//! Server configuration: one source of truth for server, auth, persistence, and all downstream component APIs.
//! Load from config file (file-first) with env overrides; fail-fast validation in production.

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
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct ServerSection {
    #[serde(default = "default_bind_addr")]
    pub bind_addr: String,
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

fn default_bind_addr() -> String {
    "0.0.0.0:8080".to_string()
}
fn default_log_level() -> String {
    "info".to_string()
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct AuthSection {
    pub bearer_token: Option<String>,
    pub tenant_id: Option<String>,
    pub caller_id: Option<String>,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct PersistenceSection {
    pub path: Option<String>,
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
        if let Ok(v) = std::env::var("AUTH_BEARER_TOKEN") {
            let t = v.trim().to_string();
            if !t.is_empty() {
                self.auth.bearer_token = Some(t);
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
    }

    /// Validate for production: persistence path, auth token, and all six component URLs required.
    pub fn require_production(&self) -> Result<ProductionConfig, String> {
        let persistence_path = self
            .persistence
            .path
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or("PERSISTENCE_PATH or DATA_DIR (or persistence.path) required in production")?
            .to_string();
        let auth_token = self
            .auth
            .bearer_token
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or("AUTH_BEARER_TOKEN (or auth.bearer_token) required in production")?
            .to_string();
        let components = self.components.require_all()?;
        Ok(ProductionConfig {
            persistence_path,
            auth_token,
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
        })
    }
}

/// Validated production config: all required fields present.
#[derive(Clone, Debug)]
pub struct ProductionConfig {
    pub persistence_path: String,
    pub auth_token: String,
    pub auth_tenant_id: String,
    pub auth_caller_id: String,
    pub components: ResolvedComponents,
    pub http_client: HttpClientConfig,
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
        config.persistence.path = Some("/data".into());
        config.auth.bearer_token = Some("token".into());
        config.components.catalog_base_url = Some("http://catalog:8080".into());
        config.components.pricing_base_url = Some("http://pricing:8080".into());
        config.components.tax_base_url = Some("http://tax:8080".into());
        config.components.geo_base_url = Some("http://geo:8080".into());
        config.components.payment_base_url = Some("http://payment:8080".into());
        config.components.receipt_base_url = Some("http://receipt:8080".into());
        config
    }

    #[test]
    fn require_production_succeeds_when_all_component_urls_set() {
        let config = production_ready_config();
        let prod = config.require_production().unwrap();
        assert_eq!(prod.persistence_path, "/data");
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
        config.persistence.path = None;
        let err = config.require_production().unwrap_err();
        assert!(err.contains("PERSISTENCE_PATH") || err.contains("persistence"));
    }

    #[test]
    fn require_production_fails_when_auth_token_missing() {
        let mut config = production_ready_config();
        config.auth.bearer_token = None;
        let err = config.require_production().unwrap_err();
        assert!(err.contains("AUTH_BEARER_TOKEN") || err.contains("bearer_token"));
    }
}
