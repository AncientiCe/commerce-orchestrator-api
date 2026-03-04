# Runbook: Outbox retries and processing

## Overview

The orchestrator writes external side effects (e.g. order.created) to an outbox. A processor dequeues messages, attempts delivery, and re-enqueues or moves to dead-letter based on a configurable max attempt count.

## Configurable retry

- Call `process_outbox_once(max_attempts)` (e.g. from a timer or worker).
- Each time a message is processed, its `attempts` counter is incremented.
- If `attempts > max_attempts`, the message is moved to the dead-letter queue; otherwise it is re-enqueued for a later retry.
- Recommended: set `max_attempts` to 3–5 and run `process_outbox_once` on a schedule (e.g. every 30s).

## Operational steps

1. **Monitor outbox size** (if exposed): if it grows unbounded, scale processors or fix downstream availability.
2. **Run the processor** regularly: `facade.process_outbox_once(3).await` (or your chosen max).
3. **Inspect dead-letter** via `facade.list_dead_letter().await`; each entry includes `id`, `topic`, `correlation_id`, and `attempts` for diagnostics.
4. **Replay after fixing cause**: use `facade.replay_from_dead_letter(&message_id).await` to put the message back on the outbox (attempts reset to 0). Ensure the downstream cause of failure is resolved before replaying.
