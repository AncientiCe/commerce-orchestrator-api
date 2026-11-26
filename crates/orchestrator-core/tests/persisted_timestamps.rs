//! Orders and order events written by earlier releases must keep loading.

use orchestrator_core::contract::OrderRecord;

fn stored_order(created_at: &str, occurred_at: &str) -> String {
    format!(
        r#"{{"order_id":"ord_1","tenant_id":"t1","transaction_id":"tx_1","checkout_id":"00000000-0000-0000-0000-000000000001",
        "status":"Created","events":[{{"id":"ev_1","event_type":"created","description":"placed","occurred_at":"{occurred_at}"}}],
        "adjustments":[],"created_at":"{created_at}"}}"#
    )
}

#[test]
fn an_order_stored_with_utc_z_timestamps_still_loads() {
    let order: OrderRecord = serde_json::from_str(&stored_order(
        "2026-11-04T10:07:14.505577Z",
        "2026-11-04T10:07:15Z",
    ))
    .expect("stored order loads");
    assert_eq!(order.created_at.to_string(), "2026-11-04T10:07:14.505577Z");
    assert!(order.events[0].occurred_at > order.created_at);
}

#[test]
fn an_order_stored_with_numeric_utc_offsets_still_loads() {
    let order: OrderRecord = serde_json::from_str(&stored_order(
        "2026-11-04T10:07:14.505577+00:00",
        "2026-11-04T10:07:15+00:00",
    ))
    .expect("stored order loads");
    assert_eq!(order.created_at.to_string(), "2026-11-04T10:07:14.505577Z");
}

#[test]
fn an_order_round_trips_through_its_stored_form() {
    let order: OrderRecord = serde_json::from_str(&stored_order(
        "2026-11-04T10:07:14.505577Z",
        "2026-11-04T10:07:15Z",
    ))
    .expect("stored order loads");
    let again: OrderRecord =
        serde_json::from_str(&serde_json::to_string(&order).expect("serialises")).expect("reloads");
    assert_eq!(again.created_at, order.created_at);
    assert_eq!(again.events[0].occurred_at, order.events[0].occurred_at);
}
