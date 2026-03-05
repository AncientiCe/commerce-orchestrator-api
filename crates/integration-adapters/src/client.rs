//! Resilient HTTP client for outbound calls to component APIs.

use crate::error::AdapterError;
use reqwest::Client;
use std::time::Duration;

/// Configuration for the shared HTTP client used by adapters.
#[derive(Clone, Debug)]
pub struct ClientConfig {
    pub connect_timeout: Duration,
    pub timeout: Duration,
    pub max_retries: u32,
    pub retry_backoff_ms: u64,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(5),
            timeout: Duration::from_secs(30),
            max_retries: 3,
            retry_backoff_ms: 100,
        }
    }
}

/// Build a reqwest client with timeouts.
pub fn build_client(config: &ClientConfig) -> Result<Client, AdapterError> {
    Client::builder()
        .connect_timeout(config.connect_timeout)
        .timeout(config.timeout)
        .build()
        .map_err(AdapterError::Http)
}

/// Execute a GET request with retries and optional correlation ID header.
pub async fn get_with_retry(
    client: &Client,
    url: &str,
    correlation_id: Option<&str>,
    config: &ClientConfig,
) -> Result<reqwest::Response, AdapterError> {
    let mut last_err = None;
    for attempt in 0..=config.max_retries {
        let mut req = client.get(url);
        if let Some(cid) = correlation_id {
            req = req.header("X-Correlation-ID", cid);
        }
        match req.send().await {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    return Ok(resp);
                }
                let body = resp.text().await.unwrap_or_default();
                last_err = Some(AdapterError::Status(status.as_u16(), body));
            }
            Err(e) => {
                if e.is_timeout() {
                    last_err = Some(AdapterError::Timeout(config.timeout));
                } else {
                    last_err = Some(AdapterError::Http(e));
                }
            }
        }
        if attempt < config.max_retries {
            let backoff =
                Duration::from_millis(config.retry_backoff_ms * 2u64.saturating_pow(attempt));
            tokio::time::sleep(backoff).await;
        }
    }
    Err(last_err.unwrap_or_else(|| AdapterError::Config("no response".into())))
}

/// Execute a POST request with JSON body, retries, and optional correlation ID.
pub async fn post_json_with_retry<T: serde::Serialize + Send>(
    client: &Client,
    url: &str,
    body: &T,
    correlation_id: Option<&str>,
    config: &ClientConfig,
) -> Result<reqwest::Response, AdapterError> {
    let mut last_err = None;
    for attempt in 0..=config.max_retries {
        let mut req = client.post(url).json(body);
        if let Some(cid) = correlation_id {
            req = req.header("X-Correlation-ID", cid);
        }
        match req.send().await {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    return Ok(resp);
                }
                let body = resp.text().await.unwrap_or_default();
                last_err = Some(AdapterError::Status(status.as_u16(), body));
            }
            Err(e) => {
                if e.is_timeout() {
                    last_err = Some(AdapterError::Timeout(config.timeout));
                } else {
                    last_err = Some(AdapterError::Http(e));
                }
            }
        }
        if attempt < config.max_retries {
            let backoff =
                Duration::from_millis(config.retry_backoff_ms * 2u64.saturating_pow(attempt));
            tokio::time::sleep(backoff).await;
        }
    }
    Err(last_err.unwrap_or_else(|| AdapterError::Config("no response".into())))
}
