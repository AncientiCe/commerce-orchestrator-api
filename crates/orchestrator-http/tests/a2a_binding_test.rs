//! A2A 1.0: the Agent Card at `/.well-known/agent-card.json` and the HTTP+JSON
//! binding (`POST /message:send`, task operations) under `/api/v1/a2a`.

use axum_test::{TestRequest, TestServer};
use orchestrator_http::{app, AppState, StaticTokenAuthnResolver};
use provider_contracts::CatalogItem;
use provider_mocks::{
    MockCatalogProvider, MockGeoProvider, MockIdentityLinkProvider, MockPaymentProvider,
    MockPricingProvider, MockReceiptProvider, MockTaxProvider,
};
use std::sync::Arc;

const BASE: &str = "https://orchestrator.example.com";

fn facade() -> orchestrator_api::OrchestratorFacade {
    let catalog = MockCatalogProvider::default();
    catalog.add_item(CatalogItem {
        id: "SKU-1".to_string(),
        title: "Test Product".to_string(),
        price_minor: 1000,
    });
    orchestrator_api::OrchestratorFacade::new(
        Arc::new(catalog),
        Arc::new(MockPricingProvider),
        Arc::new(MockTaxProvider),
        Arc::new(MockGeoProvider),
        Arc::new(MockPaymentProvider),
        Arc::new(MockReceiptProvider),
        orchestrator_core::policy::PolicyEngine::default(),
    )
}

fn server_for(state: AppState) -> TestServer {
    TestServer::new(app::app().with_state(state.with_discovery_base_url(BASE.to_string()))).unwrap()
}

fn server() -> TestServer {
    server_for(AppState::new(facade()))
}

async fn agent_card(server: &TestServer) -> serde_json::Value {
    let response = server.get("/.well-known/agent-card.json").await;
    response.assert_status_ok();
    response.json()
}

#[tokio::test]
async fn the_agent_card_has_every_required_field() {
    let card = agent_card(&server()).await;
    for field in [
        "name",
        "description",
        "supportedInterfaces",
        "version",
        "capabilities",
        "defaultInputModes",
        "defaultOutputModes",
        "skills",
    ] {
        assert!(card.get(field).is_some(), "AgentCard.{field} is required");
    }
    assert_eq!(
        card["supportedInterfaces"],
        serde_json::json!([{
            "url": format!("{BASE}/api/v1/a2a"),
            "protocolBinding": "HTTP+JSON",
            "protocolVersion": "1.0",
        }])
    );
    assert_eq!(card["capabilities"]["streaming"], false);
    assert_eq!(card["capabilities"]["pushNotifications"], false);
    assert_eq!(
        card["defaultInputModes"],
        serde_json::json!(["application/json"])
    );
    for skill in card["skills"].as_array().unwrap() {
        for field in ["id", "name", "description", "tags"] {
            assert!(skill.get(field).is_some(), "AgentSkill.{field} is required");
        }
    }
}

#[tokio::test]
async fn the_agent_card_lists_only_skills_this_deployment_can_run() {
    let ids = |card: &serde_json::Value| -> Vec<String> {
        card["skills"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["id"].as_str().unwrap().to_string())
            .collect()
    };
    let bare = agent_card(&server()).await;
    assert_eq!(ids(&bare), vec!["cart", "checkout", "order-lookup"]);

    let with_identity = agent_card(&server_for(AppState::new(
        facade().with_identity_link_provider(Arc::new(MockIdentityLinkProvider)),
    )))
    .await;
    assert!(ids(&with_identity).contains(&"identity-linking".to_string()));
}

#[tokio::test]
async fn the_agent_card_declares_bearer_auth_when_the_deployment_requires_it() {
    let open = agent_card(&server()).await;
    assert!(open.get("securityRequirements").is_none());

    let resolver = Arc::new(StaticTokenAuthnResolver::new(
        "test-token".to_string(),
        "tenant-1".to_string(),
        "caller-1".to_string(),
    ));
    let secured = agent_card(&server_for(
        AppState::new(facade())
            .production_mode(true)
            .with_authn(resolver),
    ))
    .await;
    assert_eq!(
        secured["securitySchemes"]["bearer"]["httpAuthSecurityScheme"]["scheme"],
        "Bearer"
    );
    assert_eq!(
        secured["securityRequirements"],
        serde_json::json!([{ "schemes": { "bearer": { "list": [] } } }])
    );
}

#[tokio::test]
async fn ucp_discovery_points_at_the_agent_card() {
    let profile: serde_json::Value = server().get("/.well-known/ucp").await.json();
    assert_eq!(
        profile["orchestrator"]["a2a_agent_card"],
        format!("{BASE}/.well-known/agent-card.json")
    );
}

fn send(server: &TestServer, body: serde_json::Value) -> TestRequest {
    server
        .post("/api/v1/a2a/message:send")
        .add_header("a2a-version", "1.0")
        .add_header("content-type", "application/a2a+json")
        .bytes(serde_json::to_vec(&body).unwrap().into())
}

fn message(context_id: Option<&str>, data: serde_json::Value) -> serde_json::Value {
    let mut message = serde_json::json!({
        "messageId": "msg-1",
        "role": "ROLE_USER",
        "parts": [{ "data": data, "mediaType": "application/json" }],
    });
    if let Some(context_id) = context_id {
        message["contextId"] = context_id.into();
    }
    serde_json::json!({ "message": message })
}

fn create_cart_data() -> serde_json::Value {
    serde_json::json!({
        "capability": "dev.ucp.shopping.checkout",
        "payload": { "command": { "kind": "create_cart", "merchant_id": "m1", "currency": "USD" } },
    })
}

fn reason(body: &serde_json::Value) -> &str {
    body["error"]["details"][0]["reason"]
        .as_str()
        .unwrap_or_default()
}

#[tokio::test]
async fn send_message_runs_a_cart_command_and_replies_with_an_agent_message() {
    let response = send(&server(), message(Some("ctx-7"), create_cart_data())).await;
    response.assert_status_ok();
    assert_eq!(
        response.headers()["content-type"].to_str().unwrap(),
        "application/a2a+json"
    );
    let body: serde_json::Value = serde_json::from_slice(response.as_bytes()).unwrap();
    let reply = &body["message"];
    assert_eq!(reply["role"], "ROLE_AGENT");
    assert_eq!(reply["contextId"], "ctx-7");
    assert!(reply["messageId"].as_str().is_some_and(|id| !id.is_empty()));
    assert_eq!(reply["parts"][0]["mediaType"], "application/json");
    assert!(reply["parts"][0]["data"]["cart_id"].is_string());
}

#[tokio::test]
async fn send_message_threads_a_whole_cart_through_one_context() {
    let server = server();
    let created: serde_json::Value = serde_json::from_slice(
        send(&server, message(None, create_cart_data()))
            .await
            .as_bytes(),
    )
    .unwrap();
    let context_id = created["message"]["contextId"]
        .as_str()
        .unwrap()
        .to_string();
    let cart_id = created["message"]["parts"][0]["data"]["cart_id"]
        .as_str()
        .unwrap();

    let added = send(
        &server,
        message(
            Some(&context_id),
            serde_json::json!({
                "capability": "dev.ucp.shopping.checkout",
                "payload": {
                    "command": { "kind": "add_item", "item_id": "SKU-1", "quantity": 2 },
                    "cart_id": cart_id,
                },
            }),
        ),
    )
    .await;
    added.assert_status_ok();
    let added: serde_json::Value = serde_json::from_slice(added.as_bytes()).unwrap();
    assert_eq!(added["message"]["contextId"], context_id);
    assert_eq!(added["message"]["parts"][0]["data"]["subtotal_minor"], 2000);
}

#[tokio::test]
async fn send_message_requires_a2a_version_1_0() {
    let server = server();
    for version in [None, Some("0.3")] {
        let mut request = server
            .post("/api/v1/a2a/message:send")
            .json(&message(None, create_cart_data()));
        if let Some(version) = version {
            request = request.add_header("a2a-version", version);
        }
        let response = request.await;
        assert_eq!(response.status_code().as_u16(), 400, "{version:?}");
        let body: serde_json::Value = serde_json::from_slice(response.as_bytes()).unwrap();
        assert_eq!(reason(&body), "VERSION_NOT_SUPPORTED");
        assert_eq!(body["error"]["details"][0]["domain"], "a2a-protocol.org");
    }
}

#[tokio::test]
async fn send_message_without_a_data_part_is_invalid() {
    let response = send(
        &server(),
        serde_json::json!({
            "message": { "messageId": "m", "role": "ROLE_USER", "parts": [{ "text": "buy socks" }] }
        }),
    )
    .await;
    assert_eq!(response.status_code().as_u16(), 400);
    let body: serde_json::Value = serde_json::from_slice(response.as_bytes()).unwrap();
    assert_eq!(body["error"]["status"], "INVALID_ARGUMENT");
    assert_eq!(reason(&body), "INVALID_PARAMS");
}

#[tokio::test]
async fn orchestrator_errors_keep_their_http_status_in_a2a_error_shape() {
    let response = send(
        &server(),
        message(
            None,
            serde_json::json!({
                "capability": "dev.ucp.order.query",
                "payload": { "order_id": "ord_missing" },
            }),
        ),
    )
    .await;
    assert_eq!(response.status_code().as_u16(), 404);
    let body: serde_json::Value = serde_json::from_slice(response.as_bytes()).unwrap();
    assert_eq!(body["error"]["code"], 404);
    assert_eq!(body["error"]["status"], "NOT_FOUND");
}

#[tokio::test]
async fn tasks_are_never_created_so_task_operations_report_not_found() {
    let server = server();
    for response in [
        server
            .get("/api/v1/a2a/tasks/task-1")
            .add_header("a2a-version", "1.0")
            .await,
        server
            .post("/api/v1/a2a/tasks/task-1:cancel")
            .add_header("a2a-version", "1.0")
            .await,
    ] {
        assert_eq!(response.status_code().as_u16(), 404);
        let body: serde_json::Value = serde_json::from_slice(response.as_bytes()).unwrap();
        assert_eq!(reason(&body), "TASK_NOT_FOUND");
    }
}

#[tokio::test]
async fn the_legacy_envelope_routes_still_work() {
    server()
        .post("/api/v1/a2a/cart")
        .json(&create_cart_data())
        .await
        .assert_status_ok();
}
