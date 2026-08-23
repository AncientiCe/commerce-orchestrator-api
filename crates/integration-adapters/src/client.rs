//! Resilient HTTP client for outbound calls to component APIs.
//!
//! Retry safety is the important property here. A request is only retried when
//! repeating it cannot change the downstream outcome: safe methods, or a POST
//! carrying an idempotency key that the provider is contracted to honour. A POST
//! without an idempotency key is retried only when the connection never
//! established, because any response — including a timeout after the bytes were
//! sent — means the provider may already have applied the effect.

use crate::auth::{OutboundAuth, TlsConfig};
use crate::circuit::{status_counts_as_failure, transport_failure_counts, CircuitBreaker};
use crate::error::AdapterError;
use reqwest::{Client, RequestBuilder, Response};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Upper bound on a server-provided `Retry-After` delay, so a hostile or
/// mistaken header cannot stall a checkout.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(30);

/// Configuration for the shared HTTP client used by adapters.
#[derive(Clone, Debug)]
pub struct ClientConfig {
    pub connect_timeout: Duration,
    pub timeout: Duration,
    pub max_retries: u32,
    pub retry_backoff_ms: u64,
    /// Credentials presented to the downstream provider.
    pub auth: OutboundAuth,
    /// Client certificate and trust roots for the downstream connection.
    pub tls: TlsConfig,
    /// Shared breaker for this provider. `None` disables circuit breaking.
    pub circuit_breaker: Option<Arc<CircuitBreaker>>,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(5),
            timeout: Duration::from_secs(30),
            max_retries: 3,
            retry_backoff_ms: 100,
            auth: OutboundAuth::None,
            tls: TlsConfig::default(),
            circuit_breaker: None,
        }
    }
}

/// Per-request metadata: correlation, idempotency, and provider-specific headers.
#[derive(Clone, Copy, Default)]
pub struct RequestOptions<'a> {
    pub correlation_id: Option<&'a str>,
    /// Present only when the provider is contracted to deduplicate on this key.
    /// Its presence is what makes a POST safe to retry.
    pub idempotency_key: Option<&'a str>,
    pub extra_headers: &'a [(String, String)],
}

impl<'a> RequestOptions<'a> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_correlation_id(mut self, correlation_id: Option<&'a str>) -> Self {
        self.correlation_id = correlation_id;
        self
    }

    pub fn with_idempotency_key(mut self, idempotency_key: &'a str) -> Self {
        self.idempotency_key = Some(idempotency_key);
        self
    }

    pub fn with_headers(mut self, extra_headers: &'a [(String, String)]) -> Self {
        self.extra_headers = extra_headers;
        self
    }
}

/// Build a reqwest client with timeouts and any configured TLS material.
pub fn build_client(config: &ClientConfig) -> Result<Client, AdapterError> {
    let mut builder = Client::builder()
        .connect_timeout(config.connect_timeout)
        .timeout(config.timeout);

    if let Some(pem) = config.tls.client_identity_pem.as_deref() {
        let identity = reqwest::Identity::from_pem(pem).map_err(|e| {
            AdapterError::Config(format!(
                "invalid TLS client identity (expected concatenated certificate and private key PEM): {}",
                e
            ))
        })?;
        builder = builder.identity(identity);
    }

    if let Some(pem) = config.tls.ca_bundle_pem.as_deref() {
        // Parsed eagerly so a misconfigured bundle fails at startup, not at first request.
        let certificates = reqwest::Certificate::from_pem_bundle(pem).map_err(|e| {
            AdapterError::Config(format!("invalid TLS CA bundle (expected PEM): {}", e))
        })?;
        if certificates.is_empty() {
            return Err(AdapterError::Config(
                "invalid TLS CA bundle (expected PEM): no certificates found".to_string(),
            ));
        }
        for certificate in certificates {
            builder = builder.add_root_certificate(certificate);
        }
    }

    builder.build().map_err(AdapterError::Http)
}

/// Statuses where repeating the identical request can plausibly succeed.
fn status_is_retryable(status: u16) -> bool {
    matches!(status, 408 | 425 | 429 | 500 | 502 | 503 | 504)
}

/// `Retry-After` in delta-seconds form. HTTP-date form is ignored in favour of
/// normal backoff rather than pulling in a date parser.
fn retry_after_delay(response: &Response) -> Option<Duration> {
    let raw = response.headers().get(reqwest::header::RETRY_AFTER)?;
    let seconds: u64 = raw.to_str().ok()?.trim().parse().ok()?;
    Some(Duration::from_secs(seconds).min(MAX_RETRY_AFTER))
}

/// Exponential backoff with equal jitter, so concurrent callers that fail
/// together do not retry in lockstep.
fn backoff_delay(config: &ClientConfig, attempt: u32) -> Duration {
    let base = config
        .retry_backoff_ms
        .saturating_mul(2u64.saturating_pow(attempt));
    let half = base / 2;
    let jitter = if half == 0 {
        0
    } else {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as u64)
            .unwrap_or(0);
        nanos % (half + 1)
    };
    Duration::from_millis(half + jitter)
}

/// Whether a failed attempt may be repeated.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RetryPolicy {
    /// Safe methods and idempotency-keyed requests: repeating cannot double-apply.
    Idempotent,
    /// The request may already have been applied; only a failure to connect is safe to repeat.
    NonIdempotent,
}

impl RetryPolicy {
    fn allows_status_retry(self, status: u16) -> bool {
        // A status means the provider received and processed the request. For a
        // non-idempotent call, repeating it risks a duplicate effect.
        self == Self::Idempotent && status_is_retryable(status)
    }

    fn allows_transport_retry(self, error: &reqwest::Error) -> bool {
        match self {
            Self::Idempotent => true,
            // The request never reached the provider, so nothing can have been applied.
            Self::NonIdempotent => error.is_connect(),
        }
    }
}

/// Execute a GET request with retries. GET is a safe method, so retries are unrestricted.
pub async fn get_with_retry(
    client: &Client,
    url: &str,
    correlation_id: Option<&str>,
    config: &ClientConfig,
) -> Result<Response, AdapterError> {
    let options = RequestOptions::new().with_correlation_id(correlation_id);
    execute_with_retry(
        client,
        config,
        "GET",
        url,
        RetryPolicy::Idempotent,
        &options,
        || client.get(url),
    )
    .await
}

/// Execute a POST request with a JSON body. Without an idempotency key this is
/// treated as non-idempotent and is not retried once the provider has responded.
pub async fn post_json_with_retry<T: serde::Serialize + Send + Sync>(
    client: &Client,
    url: &str,
    body: &T,
    correlation_id: Option<&str>,
    config: &ClientConfig,
) -> Result<Response, AdapterError> {
    let options = RequestOptions::new().with_correlation_id(correlation_id);
    post_json(client, url, body, config, &options).await
}

/// Execute a POST request with a JSON body and provider-specific headers.
pub async fn post_json_with_retry_with_headers<T: serde::Serialize + Send + Sync>(
    client: &Client,
    url: &str,
    body: &T,
    correlation_id: Option<&str>,
    config: &ClientConfig,
    extra_headers: &[(String, String)],
) -> Result<Response, AdapterError> {
    let options = RequestOptions::new()
        .with_correlation_id(correlation_id)
        .with_headers(extra_headers);
    post_json(client, url, body, config, &options).await
}

/// Execute a POST request with full control over correlation, idempotency, and headers.
pub async fn post_json<T: serde::Serialize + Send + Sync>(
    client: &Client,
    url: &str,
    body: &T,
    config: &ClientConfig,
    options: &RequestOptions<'_>,
) -> Result<Response, AdapterError> {
    let policy = if options.idempotency_key.is_some() {
        RetryPolicy::Idempotent
    } else {
        RetryPolicy::NonIdempotent
    };
    execute_with_retry(client, config, "POST", url, policy, options, || {
        client.post(url).json(body)
    })
    .await
}

async fn execute_with_retry<F>(
    client: &Client,
    config: &ClientConfig,
    method: &str,
    url: &str,
    policy: RetryPolicy,
    options: &RequestOptions<'_>,
    build_request: F,
) -> Result<Response, AdapterError>
where
    F: Fn() -> RequestBuilder,
{
    let effective_correlation_id = options
        .correlation_id
        .map(str::to_string)
        .or_else(orchestrator_observability::current_correlation_id);

    let mut last_err = None;
    for attempt in 0..=config.max_retries {
        if let Some(breaker) = config.circuit_breaker.as_ref() {
            // Surface the breaker rejection rather than a stale earlier failure.
            breaker.acquire()?;
        }
        let started = std::time::Instant::now();
        let mut request = build_request();
        if let Some(cid) = effective_correlation_id.as_deref() {
            request = request.header("X-Correlation-ID", cid);
        }
        if let Some(key) = options.idempotency_key {
            request = request.header("Idempotency-Key", key);
        }
        for (name, value) in options.extra_headers {
            request = request.header(name, value);
        }
        // Attaching credentials can fail on its own (an OAuth token endpoint that
        // is down, for instance). The breaker has already admitted this call, so
        // the failure has to be reported: returning here without settling a
        // half-open probe would leave the circuit closed to every later caller.
        request = match config.auth.apply(request, client).await {
            Ok(request) => request,
            Err(e) => {
                if let Some(breaker) = config.circuit_breaker.as_ref() {
                    breaker.on_failure();
                }
                orchestrator_observability::incr("provider_outbound_auth_error_total");
                return Err(e);
            }
        };

        let mut retry_after = None;
        let retryable = match request.send().await {
            Ok(response) => {
                let status = response.status();
                observe_call(
                    method,
                    url,
                    status.as_u16(),
                    started.elapsed().as_secs_f64(),
                );
                if let Some(breaker) = config.circuit_breaker.as_ref() {
                    if status.is_success() || !status_counts_as_failure(status.as_u16()) {
                        breaker.on_success();
                    } else {
                        breaker.on_failure();
                    }
                }
                if status.is_success() {
                    return Ok(response);
                }
                let retryable = policy.allows_status_retry(status.as_u16());
                if retryable {
                    retry_after = retry_after_delay(&response);
                }
                let body = response.text().await.unwrap_or_default();
                last_err = Some(AdapterError::Status(status.as_u16(), body));
                retryable
            }
            Err(e) => {
                observe_call(
                    method,
                    url,
                    if e.is_timeout() { 408 } else { 599 },
                    started.elapsed().as_secs_f64(),
                );
                if let Some(breaker) = config.circuit_breaker.as_ref() {
                    if transport_failure_counts(&e) {
                        breaker.on_failure();
                    }
                }
                let retryable = policy.allows_transport_retry(&e);
                last_err = Some(if e.is_timeout() {
                    AdapterError::Timeout(config.timeout)
                } else {
                    AdapterError::Http(e)
                });
                retryable
            }
        };

        if !retryable || attempt >= config.max_retries {
            break;
        }
        tokio::time::sleep(retry_after.unwrap_or_else(|| backoff_delay(config, attempt))).await;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retryable_statuses_are_transient_only() {
        for status in [408, 425, 429, 500, 502, 503, 504] {
            assert!(
                status_is_retryable(status),
                "{} should be retryable",
                status
            );
        }
        for status in [400, 401, 402, 403, 404, 409, 422, 501] {
            assert!(
                !status_is_retryable(status),
                "{} should not be retryable",
                status
            );
        }
    }

    #[test]
    fn non_idempotent_requests_never_retry_on_a_status() {
        let policy = RetryPolicy::NonIdempotent;
        for status in [408, 429, 500, 503] {
            assert!(
                !policy.allows_status_retry(status),
                "a keyless POST must not repeat after status {}",
                status
            );
        }
    }

    #[test]
    fn idempotent_requests_retry_on_transient_statuses() {
        let policy = RetryPolicy::Idempotent;
        assert!(policy.allows_status_retry(503));
        assert!(!policy.allows_status_retry(400));
    }

    #[test]
    fn backoff_grows_and_stays_within_bounds() {
        let config = ClientConfig {
            retry_backoff_ms: 100,
            ..ClientConfig::default()
        };
        for attempt in 0..4 {
            let base = 100u64 * 2u64.pow(attempt);
            let delay = backoff_delay(&config, attempt).as_millis() as u64;
            assert!(
                delay >= base / 2 && delay <= base,
                "attempt {} delay {}ms outside [{}, {}]",
                attempt,
                delay,
                base / 2,
                base
            );
        }
    }
}
