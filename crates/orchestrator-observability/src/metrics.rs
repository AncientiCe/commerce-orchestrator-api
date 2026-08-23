//! Prometheus metrics registry and helpers.

use once_cell::sync::Lazy;
use prometheus_client::encoding::text::encode;
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::family::Family;
use prometheus_client::metrics::gauge::Gauge;
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

#[derive(Clone, Debug, Hash, PartialEq, Eq, prometheus_client::encoding::EncodeLabelSet)]
struct OperationLabels {
    operation: String,
    status: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, prometheus_client::encoding::EncodeLabelSet)]
struct ProviderLabels {
    provider: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, prometheus_client::encoding::EncodeLabelSet)]
struct QueueLabels {
    queue: String,
}

struct MetricsRegistry {
    registry: Registry,
    event_counts: Family<EventLabels, Counter>,
    provider_calls: Family<ProviderCallLabels, Counter>,
    provider_latency: Family<ProviderLatencyLabels, Histogram>,
    operation_calls: Family<OperationLabels, Counter>,
    operation_latency: Family<OperationLabels, Histogram>,
    provider_circuit_state: Family<ProviderLabels, Gauge>,
    queue_depth: Family<QueueLabels, Gauge>,
    delivery_attempts: Family<OperationLabels, Histogram>,
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
        let operation_calls: Family<OperationLabels, Counter> = Family::default();
        let operation_latency = Family::<OperationLabels, Histogram>::new_with_constructor(|| {
            Histogram::new(exponential_buckets(0.005, 2.0, 16))
        });
        // Counter families are registered without `_total`; the encoder appends it.
        registry.register(
            "orchestrator_events",
            "Counter for orchestrator events",
            event_counts.clone(),
        );
        registry.register(
            "orchestrator_provider_http_calls",
            "Total downstream provider HTTP calls",
            provider_calls.clone(),
        );
        registry.register(
            "orchestrator_provider_http_latency_seconds",
            "Downstream provider HTTP latency in seconds",
            provider_latency.clone(),
        );
        registry.register(
            "orchestrator_operation_calls",
            "Operation calls by operation and status",
            operation_calls.clone(),
        );
        registry.register(
            "orchestrator_operation_latency_seconds",
            "Operation latency in seconds by operation and status",
            operation_latency.clone(),
        );
        let provider_circuit_state: Family<ProviderLabels, Gauge> = Family::default();
        registry.register(
            "orchestrator_provider_circuit_state",
            "Provider circuit breaker state (0 closed, 1 half-open, 2 open)",
            provider_circuit_state.clone(),
        );
        let queue_depth: Family<QueueLabels, Gauge> = Family::default();
        registry.register(
            "orchestrator_queue_depth",
            "Pending entries by queue (outbox, dead_letter)",
            queue_depth.clone(),
        );
        let delivery_attempts = Family::<OperationLabels, Histogram>::new_with_constructor(|| {
            Histogram::new(exponential_buckets(1.0, 2.0, 8))
        });
        registry.register(
            "orchestrator_delivery_attempts",
            "Attempts made before a delivery settled, by operation and status",
            delivery_attempts.clone(),
        );
        Self {
            registry,
            event_counts,
            provider_calls,
            provider_latency,
            operation_calls,
            operation_latency,
            provider_circuit_state,
            queue_depth,
            delivery_attempts,
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

pub fn observe_operation(operation: &str, status: &str, seconds: f64) {
    let guard = METRICS.lock().expect("metrics lock");
    guard
        .operation_calls
        .get_or_create(&OperationLabels {
            operation: operation.to_string(),
            status: status.to_string(),
        })
        .inc();
    guard
        .operation_latency
        .get_or_create(&OperationLabels {
            operation: operation.to_string(),
            status: status.to_string(),
        })
        .observe(seconds);
}

/// Record a provider circuit breaker state: 0 closed, 1 half-open, 2 open.
pub fn set_provider_circuit_state(provider: &str, state: i64) {
    let guard = METRICS.lock().expect("metrics lock");
    guard
        .provider_circuit_state
        .get_or_create(&ProviderLabels {
            provider: provider.to_string(),
        })
        .set(state);
}

/// Record the pending depth of a queue such as the outbox or dead-letter table.
pub fn set_queue_depth(queue: &str, depth: i64) {
    let guard = METRICS.lock().expect("metrics lock");
    guard
        .queue_depth
        .get_or_create(&QueueLabels {
            queue: queue.to_string(),
        })
        .set(depth);
}

/// Read back a queue depth gauge. Primarily for tests.
pub fn get_queue_depth(queue: &str) -> i64 {
    let guard = METRICS.lock().expect("metrics lock");
    let depth = guard
        .queue_depth
        .get_or_create(&QueueLabels {
            queue: queue.to_string(),
        })
        .get();
    depth
}

/// Record how many attempts a delivery took before settling.
pub fn observe_delivery_attempts(operation: &str, status: &str, attempts: u32) {
    let guard = METRICS.lock().expect("metrics lock");
    guard
        .delivery_attempts
        .get_or_create(&OperationLabels {
            operation: operation.to_string(),
            status: status.to_string(),
        })
        .observe(attempts as f64);
}

pub fn render_prometheus() -> Result<String, String> {
    let guard = METRICS.lock().expect("metrics lock");
    let mut output = String::new();
    encode(&mut output, &guard.registry).map_err(|error| error.to_string())?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The OpenMetrics encoder appends `_total` to counter samples, so a family
    /// registered as `..._total` is exposed as `..._total_total`. Dashboards and
    /// alerts have to reference the exposed name, so keep it clean.
    #[test]
    fn counters_are_exposed_with_exactly_one_total_suffix() {
        incr("naming_check");
        observe_operation("naming_check", "ok", 0.01);
        observe_provider_http_call("GET", "naming_check", "200", 0.01);

        let exposition = render_prometheus().expect("render");
        for name in [
            "orchestrator_events_total",
            "orchestrator_operation_calls_total",
            "orchestrator_provider_http_calls_total",
        ] {
            assert!(
                exposition.contains(&format!("\n{name}{{")),
                "{name} is not exposed under that exact name"
            );
            assert!(
                !exposition.contains(&format!("{name}_total")),
                "{name} is double-suffixed in the exposition"
            );
        }
    }
}
