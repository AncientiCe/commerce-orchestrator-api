//! Per-provider circuit breaker.
//!
//! Only failures that suggest the provider is unhealthy count: transport
//! failures and transient server statuses. A provider that answers `404` or
//! `422` is working correctly and must never be cut off.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::error::AdapterError;

/// Externally observable breaker state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

impl CircuitState {
    /// Numeric encoding for the state gauge.
    fn as_metric(self) -> i64 {
        match self {
            Self::Closed => 0,
            Self::HalfOpen => 1,
            Self::Open => 2,
        }
    }
}

/// Breaker tuning.
#[derive(Clone, Copy, Debug)]
pub struct CircuitBreakerConfig {
    pub enabled: bool,
    /// Consecutive qualifying failures before the circuit opens.
    pub failure_threshold: u32,
    /// How long the circuit stays open before a probe is allowed through.
    pub open_duration: Duration,
    /// How long a probe may stay in flight before it is treated as abandoned and
    /// another one is admitted. Without this a probe that never reports back —
    /// a request that fails before it is sent, for instance — leaves the breaker
    /// half-open forever and every later call is rejected.
    pub probe_timeout: Duration,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            failure_threshold: 5,
            open_duration: Duration::from_secs(30),
            probe_timeout: Duration::from_secs(60),
        }
    }
}

#[derive(Debug)]
enum Internal {
    Closed {
        consecutive_failures: u32,
    },
    Open {
        until: Instant,
    },
    /// A single probe is in flight; further calls are rejected until it settles
    /// or `until` passes, whichever comes first.
    HalfOpen {
        until: Instant,
    },
}

/// Shared breaker for one provider. Clone the `Arc`, not the breaker.
#[derive(Debug)]
pub struct CircuitBreaker {
    provider: String,
    config: CircuitBreakerConfig,
    state: Mutex<Internal>,
}

impl CircuitBreaker {
    pub fn new(provider: impl Into<String>, config: CircuitBreakerConfig) -> Self {
        Self {
            provider: provider.into(),
            config,
            state: Mutex::new(Internal::Closed {
                consecutive_failures: 0,
            }),
        }
    }

    pub fn provider(&self) -> &str {
        &self.provider
    }

    /// Current state, collapsing an elapsed open window into `HalfOpen`.
    pub fn state(&self) -> CircuitState {
        let guard = self.state.lock().expect("circuit state lock");
        match &*guard {
            Internal::Closed { .. } => CircuitState::Closed,
            Internal::HalfOpen { .. } => CircuitState::HalfOpen,
            Internal::Open { until } => {
                if Instant::now() >= *until {
                    CircuitState::HalfOpen
                } else {
                    CircuitState::Open
                }
            }
        }
    }

    /// Ask permission to call the provider.
    pub fn acquire(&self) -> Result<(), AdapterError> {
        if !self.config.enabled {
            return Ok(());
        }
        let mut guard = self.state.lock().expect("circuit state lock");
        let now = Instant::now();
        match &*guard {
            Internal::Closed { .. } => Ok(()),
            Internal::HalfOpen { until } => {
                if now >= *until {
                    // The previous probe never reported an outcome. Rather than stay
                    // wedged, admit a fresh one.
                    orchestrator_observability::incr("provider_circuit_probe_expired_total");
                    tracing::warn!(
                        provider = %self.provider,
                        "half-open probe never settled; admitting another"
                    );
                    *guard = Internal::HalfOpen {
                        until: now + self.config.probe_timeout,
                    };
                    Ok(())
                } else {
                    Err(self.rejected())
                }
            }
            Internal::Open { until } => {
                if now >= *until {
                    *guard = Internal::HalfOpen {
                        until: now + self.config.probe_timeout,
                    };
                    self.record_state(CircuitState::HalfOpen);
                    Ok(())
                } else {
                    Err(self.rejected())
                }
            }
        }
    }

    /// Record a healthy response, closing the circuit.
    pub fn on_success(&self) {
        if !self.config.enabled {
            return;
        }
        let mut guard = self.state.lock().expect("circuit state lock");
        let was_recovering = !matches!(
            &*guard,
            Internal::Closed {
                consecutive_failures: 0
            }
        );
        *guard = Internal::Closed {
            consecutive_failures: 0,
        };
        if was_recovering {
            self.record_state(CircuitState::Closed);
            orchestrator_observability::incr("provider_circuit_close_total");
        }
    }

    /// Record a failure that suggests the provider is unhealthy.
    pub fn on_failure(&self) {
        if !self.config.enabled {
            return;
        }
        let mut guard = self.state.lock().expect("circuit state lock");
        let should_open = match &*guard {
            // A failed probe sends us straight back to open.
            Internal::HalfOpen { .. } | Internal::Open { .. } => true,
            Internal::Closed {
                consecutive_failures,
            } => {
                let failures = consecutive_failures.saturating_add(1);
                if failures >= self.config.failure_threshold {
                    true
                } else {
                    *guard = Internal::Closed {
                        consecutive_failures: failures,
                    };
                    false
                }
            }
        };
        if should_open {
            *guard = Internal::Open {
                until: Instant::now() + self.config.open_duration,
            };
            drop(guard);
            self.record_state(CircuitState::Open);
            orchestrator_observability::incr("provider_circuit_open_total");
            tracing::warn!(
                provider = %self.provider,
                open_for_secs = self.config.open_duration.as_secs(),
                "provider circuit opened"
            );
        }
    }

    fn rejected(&self) -> AdapterError {
        orchestrator_observability::incr("provider_circuit_rejected_total");
        AdapterError::CircuitOpen(self.provider.clone())
    }

    fn record_state(&self, state: CircuitState) {
        orchestrator_observability::set_provider_circuit_state(&self.provider, state.as_metric());
    }
}

/// Whether a transport failure should count against the circuit.
pub fn transport_failure_counts(error: &reqwest::Error) -> bool {
    error.is_connect() || error.is_timeout() || error.is_request()
}

/// Whether a response status should count against the circuit. Client errors
/// mean the provider is healthy and rejecting our input, so they do not.
pub fn status_counts_as_failure(status: u16) -> bool {
    matches!(status, 408 | 429) || (500..=599).contains(&status)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn breaker(threshold: u32) -> CircuitBreaker {
        CircuitBreaker::new(
            "test",
            CircuitBreakerConfig {
                enabled: true,
                failure_threshold: threshold,
                open_duration: Duration::from_millis(20),
                probe_timeout: Duration::from_secs(60),
            },
        )
    }

    #[test]
    fn opens_only_at_the_threshold() {
        let breaker = breaker(3);
        breaker.on_failure();
        breaker.on_failure();
        assert_eq!(breaker.state(), CircuitState::Closed);
        breaker.on_failure();
        assert_eq!(breaker.state(), CircuitState::Open);
        assert!(breaker.acquire().is_err());
    }

    #[test]
    fn half_open_admits_a_single_probe() {
        let breaker = breaker(1);
        breaker.on_failure();
        std::thread::sleep(Duration::from_millis(30));
        assert!(breaker.acquire().is_ok(), "first probe admitted");
        assert!(breaker.acquire().is_err(), "second probe rejected");
    }

    #[test]
    fn a_probe_that_never_settles_does_not_wedge_the_breaker() {
        let breaker = CircuitBreaker::new(
            "test",
            CircuitBreakerConfig {
                enabled: true,
                failure_threshold: 1,
                open_duration: Duration::from_millis(20),
                probe_timeout: Duration::from_millis(20),
            },
        );
        breaker.on_failure();
        std::thread::sleep(Duration::from_millis(30));
        breaker.acquire().expect("first probe admitted");
        assert!(
            breaker.acquire().is_err(),
            "a probe is in flight, so the next call waits"
        );

        // The probe never reported success or failure (e.g. the call failed before
        // it was sent). Once its lease expires another attempt must get through.
        std::thread::sleep(Duration::from_millis(30));
        assert!(
            breaker.acquire().is_ok(),
            "an abandoned probe must not lock the provider out permanently"
        );
    }

    #[test]
    fn success_after_probe_closes_the_circuit() {
        let breaker = breaker(1);
        breaker.on_failure();
        std::thread::sleep(Duration::from_millis(30));
        breaker.acquire().unwrap();
        breaker.on_success();
        assert_eq!(breaker.state(), CircuitState::Closed);
        assert!(breaker.acquire().is_ok());
    }

    #[test]
    fn client_errors_are_not_circuit_failures() {
        for status in [400, 401, 402, 404, 409, 422] {
            assert!(!status_counts_as_failure(status), "{}", status);
        }
        for status in [408, 429, 500, 502, 503, 504] {
            assert!(status_counts_as_failure(status), "{}", status);
        }
    }

    #[test]
    fn disabled_breaker_always_admits() {
        let breaker = CircuitBreaker::new(
            "test",
            CircuitBreakerConfig {
                enabled: false,
                failure_threshold: 1,
                open_duration: Duration::from_secs(60),
                probe_timeout: Duration::from_secs(60),
            },
        );
        breaker.on_failure();
        breaker.on_failure();
        assert_eq!(breaker.state(), CircuitState::Closed);
        assert!(breaker.acquire().is_ok());
    }
}
