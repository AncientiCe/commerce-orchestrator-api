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
    let effective_correlation_id = correlation_id
        .map(str::to_string)
        .or_else(orchestrator_observability::current_correlation_id);
    let mut last_err = None;
    for attempt in 0..=config.max_retries {
        let started = std::time::Instant::now();
        let mut req = client.get(url);
        if let Some(cid) = effective_correlation_id.as_deref() {
            req = req.header("X-Correlation-ID", cid);
        }
        match req.send().await {
            Ok(resp) => {
                let status = resp.status();
                observe_call("GET", url, status.as_u16(), started.elapsed().as_secs_f64());
                if status.is_success() {
                    return Ok(resp);
                }
                let body = resp.text().await.unwrap_or_default();
                last_err = Some(AdapterError::Status(status.as_u16(), body));
            }
            Err(e) => {
                observe_call(
                    "GET",
                    url,
                    if e.is_timeout() { 408 } else { 599 },
                    started.elapsed().as_secs_f64(),
                );
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
    let effective_correlation_id = correlation_id
        .map(str::to_string)
        .or_else(orchestrator_observability::current_correlation_id);
    let mut last_err = None;
    for attempt in 0..=config.max_retries {
        let started = std::time::Instant::now();
        let mut req = client.post(url).json(body);
        if let Some(cid) = effective_correlation_id.as_deref() {
            req = req.header("X-Correlation-ID", cid);
        }
        match req.send().await {
            Ok(resp) => {
                let status = resp.status();
                observe_call(
                    "POST",
                    url,
                    status.as_u16(),
                    started.elapsed().as_secs_f64(),
                );
                if status.is_success() {
                    return Ok(resp);
                }
                let body = resp.text().await.unwrap_or_default();
                last_err = Some(AdapterError::Status(status.as_u16(), body));
            }
            Err(e) => {
                observe_call(
                    "POST",
                    url,
                    if e.is_timeout() { 408 } else { 599 },
                    started.elapsed().as_secs_f64(),
                );
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

fn provider_from_url(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .unwrap_or_else(|| "unknown".to_string())
}

fn observe_call(method: &str, url: &str, status: u16, latency_seconds: f64) {
    let provider = provider_from_url(url);
    orchestrator_observability::observe_provider_http_call(
        method,
        &provider,
        &status.to_string(),
        latency_seconds,
    );
}
