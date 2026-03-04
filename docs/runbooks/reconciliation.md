# Runbook: Payment reconciliation

## Overview

Reconciliation compares the orchestrator’s view of payment state (per transaction) with the payment provider’s view (when the provider implements `get_payment_state`). Use it to detect drift (e.g. captured in our store but voided at the provider).

## Running reconciliation

1. Collect transaction IDs to reconcile (e.g. from your orders or time window).
2. Call `facade.run_reconciliation(&transaction_ids).await`.
3. Inspect `report.mismatches`: each entry has `transaction_id`, `our_state`, and `provider_state` (if the provider returned a state).

## Handling mismatches

- **Our state differs from provider**: Investigate whether the provider was updated outside the orchestrator (e.g. manual void/refund) or a previous call failed after we updated our state. Align data or correct the provider state as per your policy.
- **Provider does not support get_payment_state**: Default implementation returns `None`; reconciliation then never reports a mismatch for that provider. Implement `get_payment_state` on your payment provider adapter to enable drift detection.

## Scheduling

Run reconciliation periodically (e.g. daily) over recent transaction IDs and alert on non-empty `mismatches`.
