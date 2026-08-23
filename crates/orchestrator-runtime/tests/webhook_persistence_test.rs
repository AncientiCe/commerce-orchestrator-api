//! Webhook registrations must outlive the process. Before v0.9.0 they were held
//! in memory even in persistent mode, so every restart silently dropped every
//! subscriber.

use orchestrator_runtime::persistence::open_persistent_stores;
use orchestrator_runtime::webhooks::WebhookRegistration;

fn registration(id: &str, topic: Option<&str>) -> WebhookRegistration {
    WebhookRegistration {
        id: id.to_string(),
        tenant_id: "t1".to_string(),
        url: format!("https://example.com/{}", id),
        secret: "shhh".to_string(),
        event_filter: topic.map(|t| vec![t.to_string()]),
        active: true,
    }
}

async fn temp_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("orch-webhooks-{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&dir).await.unwrap();
    dir
}

#[tokio::test]
async fn registrations_survive_a_restart() {
    let dir = temp_dir().await;

    let stores = open_persistent_stores(&dir).await.unwrap();
    stores
        .webhook_store()
        .register(registration("wh_1", None))
        .await
        .unwrap();
    drop(stores);

    let reopened = open_persistent_stores(&dir).await.unwrap();
    let hooks = reopened.webhook_store().list_by_tenant("t1").await.unwrap();
    assert_eq!(hooks.len(), 1, "registration should have been reloaded");
    assert_eq!(hooks[0].url, "https://example.com/wh_1");

    tokio::fs::remove_dir_all(&dir).await.ok();
}

#[tokio::test]
async fn unregister_survives_a_restart() {
    let dir = temp_dir().await;

    let stores = open_persistent_stores(&dir).await.unwrap();
    stores
        .webhook_store()
        .register(registration("wh_2", None))
        .await
        .unwrap();
    assert!(stores.webhook_store().unregister("wh_2").await.unwrap());
    drop(stores);

    let reopened = open_persistent_stores(&dir).await.unwrap();
    assert!(reopened
        .webhook_store()
        .list_by_tenant("t1")
        .await
        .unwrap()
        .is_empty());

    tokio::fs::remove_dir_all(&dir).await.ok();
}

#[tokio::test]
async fn topic_filter_is_preserved_across_a_restart() {
    let dir = temp_dir().await;

    let stores = open_persistent_stores(&dir).await.unwrap();
    stores
        .webhook_store()
        .register(registration("wh_3", Some("payment.captured")))
        .await
        .unwrap();
    drop(stores);

    let reopened = open_persistent_stores(&dir).await.unwrap();
    let store = reopened.webhook_store();
    assert!(store
        .list_active_for_topic("t1", "order.created")
        .await
        .unwrap()
        .is_empty());
    assert_eq!(
        store
            .list_active_for_topic("t1", "payment.captured")
            .await
            .unwrap()
            .len(),
        1
    );

    tokio::fs::remove_dir_all(&dir).await.ok();
}

#[tokio::test]
async fn a_reloaded_registration_is_still_scoped_to_its_tenant() {
    let dir = temp_dir().await;

    let stores = open_persistent_stores(&dir).await.unwrap();
    stores
        .webhook_store()
        .register(registration("wh_4", None))
        .await
        .unwrap();
    drop(stores);

    let reopened = open_persistent_stores(&dir).await.unwrap();
    assert!(
        reopened
            .webhook_store()
            .list_active_for_topic("other_tenant", "order.created")
            .await
            .unwrap()
            .is_empty(),
        "another tenant must not receive t1's events after a restart"
    );

    tokio::fs::remove_dir_all(&dir).await.ok();
}
