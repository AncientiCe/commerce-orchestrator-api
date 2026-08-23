//! Background outbox processor.
//!
//! Without this loop nothing ever drains the outbox in a running server:
//! messages accumulate and no webhook is ever delivered. The Postgres dequeue
//! uses `FOR UPDATE SKIP LOCKED`, so several replicas can run this safely.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::runner::{OutboxOutcome, Runner};

/// Loop tuning.
#[derive(Clone, Copy, Debug)]
pub struct OutboxProcessorConfig {
    /// Pause between polls when the outbox is empty.
    pub interval: Duration,
    /// Messages handled per wake-up before pausing again.
    pub batch_size: usize,
    /// Delivery attempts before a message is dead-lettered.
    pub max_attempts: u32,
    /// How long shutdown waits for the queue to drain.
    pub drain_timeout: Duration,
    /// Pause after the first failed delivery, doubled per attempt. Without it a
    /// down endpoint burns the whole attempt budget within milliseconds and every
    /// message lands in the dead-letter queue.
    pub retry_backoff: Duration,
    /// Ceiling for that pause.
    pub max_retry_backoff: Duration,
}

impl Default for OutboxProcessorConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_millis(500),
            batch_size: 32,
            max_attempts: 5,
            drain_timeout: Duration::from_secs(10),
            retry_backoff: Duration::from_millis(500),
            max_retry_backoff: Duration::from_secs(30),
        }
    }
}

/// Drains the outbox on an interval until asked to stop.
pub struct OutboxProcessor {
    runner: Runner,
    config: OutboxProcessorConfig,
}

impl OutboxProcessor {
    pub fn new(runner: Runner, config: OutboxProcessorConfig) -> Self {
        Self { runner, config }
    }

    /// Process up to `batch_size` messages. Returns how many were handled.
    ///
    /// A failed delivery pauses the batch for a backoff proportional to the
    /// attempts already spent, so a provider that is down is retried slowly
    /// instead of being hammered until the message is dead-lettered.
    pub async fn process_batch(&self) -> usize {
        let mut handled = 0;
        for _ in 0..self.config.batch_size {
            let pending_before = self.runner.outbox_len().await;
            if pending_before == 0 {
                break;
            }
            match self
                .runner
                .process_outbox_once_reporting(self.config.max_attempts)
                .await
            {
                Ok(OutboxOutcome::Idle) => break,
                Ok(OutboxOutcome::Delivered) | Ok(OutboxOutcome::DeadLettered) => handled += 1,
                Ok(OutboxOutcome::Retrying { attempts }) => {
                    handled += 1;
                    tokio::time::sleep(self.retry_delay(attempts)).await;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "outbox processing failed");
                    orchestrator_observability::incr("outbox_process_error_total");
                    break;
                }
            }
        }
        self.record_depth().await;
        handled
    }

    /// Exponential backoff on the attempts already spent, capped.
    fn retry_delay(&self, attempts: u32) -> Duration {
        retry_delay(&self.config, attempts)
    }

    /// Publish current queue depths so an operator can alert on a growing backlog.
    pub async fn record_depth(&self) {
        orchestrator_observability::set_queue_depth(
            "outbox",
            self.runner.outbox_len().await as i64,
        );
        orchestrator_observability::set_queue_depth(
            "dead_letter",
            self.runner.dead_letter_len().await as i64,
        );
    }

    /// Drain until the outbox is empty or the timeout expires. Returns how many
    /// messages were handled.
    pub async fn drain(&self) -> usize {
        let deadline = tokio::time::Instant::now() + self.config.drain_timeout;
        let mut handled = 0;
        while tokio::time::Instant::now() < deadline {
            let processed = self.process_batch().await;
            if processed == 0 {
                break;
            }
            handled += processed;
        }
        handled
    }

    /// Run until `shutdown` flips to true, then drain what remains.
    pub async fn run(self, mut shutdown: watch::Receiver<bool>) {
        tracing::info!(
            interval_ms = self.config.interval.as_millis() as u64,
            batch_size = self.config.batch_size,
            "outbox processor started"
        );
        loop {
            if *shutdown.borrow() {
                break;
            }
            let handled = self.process_batch().await;
            if handled > 0 {
                // More work is likely waiting; poll again without pausing.
                continue;
            }
            tokio::select! {
                _ = tokio::time::sleep(self.config.interval) => {}
                _ = shutdown.changed() => {}
            }
        }
        let drained = self.drain().await;
        tracing::info!(drained, "outbox processor stopped");
    }

    /// Spawn [`Self::run`] on the current runtime.
    pub fn spawn(self, shutdown: watch::Receiver<bool>) -> JoinHandle<()> {
        tokio::spawn(self.run(shutdown))
    }
}

/// Exponential backoff on the attempts already spent, capped.
fn retry_delay(config: &OutboxProcessorConfig, attempts: u32) -> Duration {
    let factor = 1u32 << attempts.saturating_sub(1).min(16);
    config
        .retry_backoff
        .saturating_mul(factor)
        .min(config.max_retry_backoff)
}

/// Build a webhook deliverer over a runner's own webhook registrations.
pub fn webhook_deliverer_for(runner: &Runner) -> Arc<crate::webhooks::WebhookDeliverer> {
    Arc::new(crate::webhooks::WebhookDeliverer::new(
        runner.webhook_store(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_backoff_grows_with_attempts_and_is_capped() {
        let config = OutboxProcessorConfig {
            retry_backoff: Duration::from_millis(100),
            max_retry_backoff: Duration::from_millis(400),
            ..OutboxProcessorConfig::default()
        };

        assert_eq!(retry_delay(&config, 1), Duration::from_millis(100));
        assert_eq!(retry_delay(&config, 2), Duration::from_millis(200));
        assert_eq!(retry_delay(&config, 3), Duration::from_millis(400));
        assert_eq!(
            retry_delay(&config, 30),
            Duration::from_millis(400),
            "backoff must stay bounded however many attempts have been spent"
        );
    }

    #[test]
    fn a_failing_delivery_is_never_retried_instantly() {
        assert!(
            retry_delay(&OutboxProcessorConfig::default(), 1) > Duration::ZERO,
            "an immediate retry burns the attempt budget without giving the endpoint time to recover"
        );
    }
}
