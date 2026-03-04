# Runbook: Dead-letter queue handling

## When messages reach dead-letter

Messages are moved to the dead-letter queue when they exceed the configured `max_attempts` in `process_outbox_once(max_attempts)`.

## Diagnostics

- **List entries**: `facade.list_dead_letter().await` returns all dead-letter messages with `id`, `topic`, `payload`, `correlation_id`, and `attempts`.
- Use `correlation_id` to correlate with checkout/transaction logs and trace the original request.

## Replay

1. Identify the message `id` from `list_dead_letter()`.
2. Fix the root cause (e.g. downstream API availability, schema change, or configuration).
3. Replay: `facade.replay_from_dead_letter(&message_id).await`. Returns `true` if the message was found and re-enqueued.
4. The message is re-enqueued with `attempts` reset to 0 and will be processed again by the next `process_outbox_once` run.

## Do not

- Replay in a tight loop without fixing the cause; messages will fail again and return to dead-letter.
- Delete or clear dead-letter without logging or archiving if you need an audit trail.
