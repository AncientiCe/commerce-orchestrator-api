//! Prometheus metrics registry and helpers.

use once_cell::sync::Lazy;
use prometheus_client::encoding::text::encode;
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::family::Family;
use prometheus_client::metrics::histogram::{exponential_buckets, Histogram};
use prometheus_client::registry::Registry;

#[derive(Clone, Debug, Hash, PartialEq, Eq, prometheus_client::encoding::EncodeLabelSet)]
struct EventLabels {
    name: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, prometheus_client::encoding::EncodeLabelSet)]
struct ProviderCallLabels {
    method: String,
    provider: String,
    status: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, prometheus_client::encoding::EncodeLabelSet)]
struct ProviderLatencyLabels {
    method: String,
    provider: String,
}

struct MetricsRegistry {
    registry: Registry,
    event_counts: Family<EventLabels, Counter>,
    provider_calls: Family<ProviderCallLabels, Counter>,
    provider_latency: Family<ProviderLatencyLabels, Histogram>,
}

impl MetricsRegistry {
    fn new() -> Self {
        let mut registry = Registry::default();
        let event_counts: Family<EventLabels, Counter> = Family::default();
        let provider_calls: Family<ProviderCallLabels, Counter> = Family::default();
        let provider_latency =
            Family::<ProviderLatencyLabels, Histogram>::new_with_constructor(|| {
                Histogram::new(exponential_buckets(0.005, 2.0, 16))
            });
        registry.register(
            "orchestrator_events_total",
            "Counter for orchestrator events",
            event_counts.clone(),
        );
        registry.register(
            "orchestrator_provider_http_calls_total",
            "Total downstream provider HTTP calls",
            provider_calls.clone(),
        );
        registry.register(
            "orchestrator_provider_http_latency_seconds",
            "Downstream provider HTTP latency in seconds",
            provider_latency.clone(),
        );
        Self {
            registry,
            event_counts,
            provider_calls,
            provider_latency,
        }
    }
}

static METRICS: Lazy<std::sync::Mutex<MetricsRegistry>> =
    Lazy::new(|| std::sync::Mutex::new(MetricsRegistry::new()));

pub fn incr(name: &str) {
    let guard = METRICS.lock().expect("metrics lock");
    guard
        .event_counts
        .get_or_create(&EventLabels {
            name: name.to_string(),
        })
        .inc();
}

pub fn get_count(name: &str) -> u64 {
    let guard = METRICS.lock().expect("metrics lock");
    let counter = guard.event_counts.get_or_create(&EventLabels {
        name: name.to_string(),
    });
    counter.get()
}

pub fn observe_provider_http_call(method: &str, provider: &str, status: &str, seconds: f64) {
    let guard = METRICS.lock().expect("metrics lock");
    guard
        .provider_calls
        .get_or_create(&ProviderCallLabels {
            method: method.to_string(),
            provider: provider.to_string(),
            status: status.to_string(),
        })
        .inc();
    guard
        .provider_latency
        .get_or_create(&ProviderLatencyLabels {
            method: method.to_string(),
            provider: provider.to_string(),
        })
        .observe(seconds);
}

pub fn render_prometheus() -> Result<String, String> {
    let guard = METRICS.lock().expect("metrics lock");
    let mut output = String::new();
    encode(&mut output, &guard.registry).map_err(|error| error.to_string())?;
    Ok(output)
}
