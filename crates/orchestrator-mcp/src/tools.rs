//! MCP tool definitions and dispatch for commerce operations.

use crate::jsonrpc::{JsonRpcRequest, JsonRpcResponse, INTERNAL_ERROR, METHOD_NOT_FOUND};
use orchestrator_api::{AuthContext, OrchestratorFacade};
use orchestrator_core::contract::*;
use std::time::Instant;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

pub fn list_tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "create_cart".to_string(),
            description: "Create a new shopping cart".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "merchant_id": { "type": "string" },
                    "currency": { "type": "string" }
                },
                "required": ["merchant_id", "currency"]
            }),
        },
        ToolDefinition {
            name: "add_item".to_string(),
            description: "Add an item to a cart".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "cart_id": { "type": "string" },
                    "item_id": { "type": "string" },
                    "quantity": { "type": "integer" }
                },
                "required": ["cart_id", "item_id", "quantity"]
            }),
        },
        ToolDefinition {
            name: "update_item_qty".to_string(),
            description: "Update item quantity in a cart".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "cart_id": { "type": "string" },
                    "line_id": { "type": "string" },
                    "quantity": { "type": "integer" }
                },
                "required": ["cart_id", "line_id", "quantity"]
            }),
        },
        ToolDefinition {
            name: "remove_item".to_string(),
            description: "Remove an item from a cart".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "cart_id": { "type": "string" },
                    "line_id": { "type": "string" }
                },
                "required": ["cart_id", "line_id"]
            }),
        },
        ToolDefinition {
            name: "apply_adjustment".to_string(),
            description: "Apply a discount or promo code".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "cart_id": { "type": "string" },
                    "code": { "type": "string" }
                },
                "required": ["cart_id", "code"]
            }),
        },
        ToolDefinition {
            name: "get_cart".to_string(),
            description: "Get current cart state".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "cart_id": { "type": "string" }
                },
                "required": ["cart_id"]
            }),
        },
        ToolDefinition {
            name: "start_checkout".to_string(),
            description: "Mark cart as checkout-ready".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "cart_id": { "type": "string" },
                    "cart_version": { "type": "integer" }
                },
                "required": ["cart_id", "cart_version"]
            }),
        },
        ToolDefinition {
            name: "execute_checkout".to_string(),
            description: "Execute checkout for a cart".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "tenant_id": { "type": "string" },
                    "merchant_id": { "type": "string" },
                    "cart_id": { "type": "string" },
                    "cart_version": { "type": "integer" },
                    "currency": { "type": "string" },
                    "payment_intent": { "type": "object" },
                    "idempotency_key": { "type": "string" }
                },
                "required": ["tenant_id", "merchant_id", "cart_id", "cart_version", "currency", "payment_intent", "idempotency_key"]
            }),
        },
        ToolDefinition {
            name: "get_order".to_string(),
            description: "Retrieve an order by ID".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "order_id": { "type": "string" }
                },
                "required": ["order_id"]
            }),
        },
        ToolDefinition {
            name: "list_orders".to_string(),
            description: "List orders for a tenant".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "tenant_id": { "type": "string" }
                },
                "required": ["tenant_id"]
            }),
        },
        ToolDefinition {
            name: "capture_payment".to_string(),
            description: "Capture an authorized payment".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "tenant_id": { "type": "string" },
                    "merchant_id": { "type": "string" },
                    "transaction_id": { "type": "string" },
                    "amount_minor": { "type": "integer" },
                    "idempotency_key": { "type": "string" }
                },
                "required": ["tenant_id", "merchant_id", "transaction_id", "amount_minor", "idempotency_key"]
            }),
        },
        ToolDefinition {
            name: "void_payment".to_string(),
            description: "Void an authorized payment".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "tenant_id": { "type": "string" },
                    "merchant_id": { "type": "string" },
                    "transaction_id": { "type": "string" },
                    "amount_minor": { "type": "integer" },
                    "idempotency_key": { "type": "string" }
                },
                "required": ["tenant_id", "merchant_id", "transaction_id", "amount_minor", "idempotency_key"]
            }),
        },
        ToolDefinition {
            name: "refund_payment".to_string(),
            description: "Refund a captured payment".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "tenant_id": { "type": "string" },
                    "merchant_id": { "type": "string" },
                    "transaction_id": { "type": "string" },
                    "amount_minor": { "type": "integer" },
                    "idempotency_key": { "type": "string" }
                },
                "required": ["tenant_id", "merchant_id", "transaction_id", "amount_minor", "idempotency_key"]
            }),
        },
        ToolDefinition {
            name: "lookup_catalog_item".to_string(),
            description: "Look up a catalog item by ID".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "item_id": { "type": "string" }
                },
                "required": ["item_id"]
            }),
        },
    ]
}

fn parse_cart_id(s: &str) -> Result<CartId, String> {
    uuid::Uuid::parse_str(s)
        .map(CartId)
        .map_err(|e| e.to_string())
}

fn get_str<'a>(params: &'a serde_json::Value, key: &str) -> Result<&'a str, String> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("missing required field: {}", key))
}

fn get_u64(params: &serde_json::Value, key: &str) -> Result<u64, String> {
    params
        .get(key)
        .and_then(|v| v.as_u64())
        .ok_or_else(|| format!("missing required field: {}", key))
}

fn get_i64(params: &serde_json::Value, key: &str) -> Result<i64, String> {
    params
        .get(key)
        .and_then(|v| v.as_i64())
        .ok_or_else(|| format!("missing required field: {}", key))
}

/// Handle a JSON-RPC request and return a response. Dispatches MCP methods to facade operations.
pub async fn handle_mcp_request(
    request: &JsonRpcRequest,
    facade: &OrchestratorFacade,
    auth: &AuthContext,
) -> JsonRpcResponse {
    let started = Instant::now();
    let result = match request.method.as_str() {
        "tools/list" => {
            let tools = list_tools();
            Ok(serde_json::json!({ "tools": tools }))
        }
        "tools/call" => {
            let params = request.params.as_ref().cloned().unwrap_or_default();
            let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let arguments = params.get("arguments").cloned().unwrap_or_default();
            dispatch_tool(tool_name, &arguments, facade, auth).await
        }
        "resources/list" => Ok(serde_json::json!({
            "resources": [
                {
                    "uri": "order://{id}",
                    "name": "Order",
                    "description": "Retrieve an order by ID",
                    "mimeType": "application/json"
                },
                {
                    "uri": "cart://{id}",
                    "name": "Cart",
                    "description": "Retrieve a cart by ID",
                    "mimeType": "application/json"
                }
            ]
        })),
        "resources/read" => {
            let params = request.params.as_ref().cloned().unwrap_or_default();
            let uri = params.get("uri").and_then(|v| v.as_str()).unwrap_or("");
            read_resource(uri, facade).await
        }
        "initialize" => Ok(serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": { "listChanged": false },
                "resources": { "subscribe": false, "listChanged": false }
            },
            "serverInfo": {
                "name": "commerce-orchestrator",
                "version": env!("CARGO_PKG_VERSION")
            }
        })),
        _ => {
            return JsonRpcResponse::error(
                request.id.clone(),
                METHOD_NOT_FOUND,
                format!("method not found: {}", request.method),
            );
        }
    };

    let tool_name = request
        .params
        .as_ref()
        .and_then(|p| p.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or(&request.method);

    match result {
        Ok(value) => {
            orchestrator_observability::observe_operation(
                "mcp_tool_call",
                "success",
                started.elapsed().as_secs_f64(),
            );
            orchestrator_observability::incr(&format!("mcp_tool_{}_total", tool_name));
            JsonRpcResponse::success(request.id.clone(), value)
        }
        Err(err) => {
            orchestrator_observability::observe_operation(
                "mcp_tool_call",
                "error",
                started.elapsed().as_secs_f64(),
            );
            JsonRpcResponse::error(request.id.clone(), INTERNAL_ERROR, err)
        }
    }
}

async fn dispatch_tool(
    name: &str,
    args: &serde_json::Value,
    facade: &OrchestratorFacade,
    auth: &AuthContext,
) -> Result<serde_json::Value, String> {
    match name {
        "create_cart" => {
            let cmd = CartCommand::CreateCart(CreateCartPayload {
                merchant_id: get_str(args, "merchant_id")?.to_string(),
                currency: get_str(args, "currency")?.to_string(),
            });
            let proj = facade
                .dispatch_cart_command(cmd, None)
                .await
                .map_err(|e| e.to_string())?;
            serde_json::to_value(proj).map_err(|e| e.to_string())
        }
        "add_item" => {
            let cart_id = parse_cart_id(get_str(args, "cart_id")?)?;
            let cmd = CartCommand::AddItem(AddItemPayload {
                item_id: get_str(args, "item_id")?.to_string(),
                quantity: get_u64(args, "quantity")? as u32,
            });
            let proj = facade
                .dispatch_cart_command(cmd, Some(cart_id))
                .await
                .map_err(|e| e.to_string())?;
            serde_json::to_value(proj).map_err(|e| e.to_string())
        }
        "update_item_qty" => {
            let cart_id = parse_cart_id(get_str(args, "cart_id")?)?;
            let cmd = CartCommand::UpdateItemQty(UpdateItemQtyPayload {
                line_id: get_str(args, "line_id")?.to_string(),
                quantity: get_u64(args, "quantity")? as u32,
            });
            let proj = facade
                .dispatch_cart_command(cmd, Some(cart_id))
                .await
                .map_err(|e| e.to_string())?;
            serde_json::to_value(proj).map_err(|e| e.to_string())
        }
        "remove_item" => {
            let cart_id = parse_cart_id(get_str(args, "cart_id")?)?;
            let cmd = CartCommand::RemoveItem(RemoveItemPayload {
                line_id: get_str(args, "line_id")?.to_string(),
            });
            let proj = facade
                .dispatch_cart_command(cmd, Some(cart_id))
                .await
                .map_err(|e| e.to_string())?;
            serde_json::to_value(proj).map_err(|e| e.to_string())
        }
        "apply_adjustment" => {
            let cart_id = parse_cart_id(get_str(args, "cart_id")?)?;
            let cmd = CartCommand::ApplyAdjustment(ApplyAdjustmentPayload {
                code: get_str(args, "code")?.to_string(),
            });
            let proj = facade
                .dispatch_cart_command(cmd, Some(cart_id))
                .await
                .map_err(|e| e.to_string())?;
            serde_json::to_value(proj).map_err(|e| e.to_string())
        }
        "get_cart" => {
            let cart_id = parse_cart_id(get_str(args, "cart_id")?)?;
            let cmd = CartCommand::GetCart(GetCartPayload { cart_id });
            let proj = facade
                .dispatch_cart_command(cmd, Some(cart_id))
                .await
                .map_err(|e| e.to_string())?;
            serde_json::to_value(proj).map_err(|e| e.to_string())
        }
        "start_checkout" => {
            let cart_id = parse_cart_id(get_str(args, "cart_id")?)?;
            let cmd = CartCommand::StartCheckout(StartCheckoutPayload {
                cart_id,
                cart_version: get_u64(args, "cart_version")?,
            });
            let proj = facade
                .dispatch_cart_command(cmd, Some(cart_id))
                .await
                .map_err(|e| e.to_string())?;
            serde_json::to_value(proj).map_err(|e| e.to_string())
        }
        "execute_checkout" => {
            let request = CheckoutRequest {
                tenant_id: get_str(args, "tenant_id")?.to_string(),
                merchant_id: get_str(args, "merchant_id")?.to_string(),
                cart_id: parse_cart_id(get_str(args, "cart_id")?)?,
                cart_version: get_u64(args, "cart_version")?,
                currency: get_str(args, "currency")?.to_string(),
                customer: None,
                location: None,
                payment_intent: serde_json::from_value(
                    args.get("payment_intent")
                        .cloned()
                        .ok_or("missing payment_intent")?,
                )
                .map_err(|e| e.to_string())?,
                idempotency_key: get_str(args, "idempotency_key")?.to_string(),
            };
            let result = facade
                .execute_checkout_authorized(auth, request)
                .await
                .map_err(|e| e.to_string())?;
            serde_json::to_value(result).map_err(|e| e.to_string())
        }
        "get_order" => {
            let order_id = get_str(args, "order_id")?;
            let order = facade
                .get_order(order_id)
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "order not found".to_string())?;
            serde_json::to_value(order).map_err(|e| e.to_string())
        }
        "list_orders" => {
            let tenant_id = get_str(args, "tenant_id")?;
            let orders = facade
                .list_orders(tenant_id)
                .await
                .map_err(|e| e.to_string())?;
            serde_json::to_value(orders).map_err(|e| e.to_string())
        }
        "capture_payment" | "void_payment" | "refund_payment" => {
            let request = PaymentLifecycleRequest {
                tenant_id: get_str(args, "tenant_id")?.to_string(),
                merchant_id: get_str(args, "merchant_id")?.to_string(),
                transaction_id: get_str(args, "transaction_id")?.to_string(),
                amount_minor: get_i64(args, "amount_minor")?,
                idempotency_key: get_str(args, "idempotency_key")?.to_string(),
            };
            let result = match name {
                "capture_payment" => facade.capture_payment(&request).await,
                "void_payment" => facade.void_payment(&request).await,
                "refund_payment" => facade.refund_payment(&request).await,
                _ => unreachable!(),
            };
            let op_result = result.map_err(|e| e.to_string())?;
            Ok(serde_json::json!({
                "success": op_result.success,
                "reference": op_result.reference,
            }))
        }
        "lookup_catalog_item" => {
            let item_id = get_str(args, "item_id")?;
            let item = facade
                .lookup_catalog_item(item_id)
                .await
                .map_err(|e| e.to_string())?;
            Ok(serde_json::json!({
                "id": item.id,
                "title": item.title,
                "price_minor": item.price_minor,
            }))
        }
        _ => Err(format!("unknown tool: {}", name)),
    }
}

async fn read_resource(
    uri: &str,
    facade: &OrchestratorFacade,
) -> Result<serde_json::Value, String> {
    if let Some(order_id) = uri.strip_prefix("order://") {
        let order = facade
            .get_order(order_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "order not found".to_string())?;
        let content = serde_json::to_string(&order).map_err(|e| e.to_string())?;
        Ok(serde_json::json!({
            "contents": [{
                "uri": uri,
                "mimeType": "application/json",
                "text": content
            }]
        }))
    } else if let Some(cart_id_str) = uri.strip_prefix("cart://") {
        let cart_id = parse_cart_id(cart_id_str)?;
        let cmd = CartCommand::GetCart(GetCartPayload { cart_id });
        let proj = facade
            .dispatch_cart_command(cmd, Some(cart_id))
            .await
            .map_err(|e| e.to_string())?;
        let content = serde_json::to_string(&proj).map_err(|e| e.to_string())?;
        Ok(serde_json::json!({
            "contents": [{
                "uri": uri,
                "mimeType": "application/json",
                "text": content
            }]
        }))
    } else {
        Err(format!("unsupported resource URI: {}", uri))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orchestrator_core::policy::PolicyEngine;
    use provider_contracts::CatalogItem;
    use provider_mocks::*;
    use std::sync::Arc;

    fn build_facade() -> OrchestratorFacade {
        let catalog = MockCatalogProvider::new();
        catalog.add_item(CatalogItem {
            id: "item_1".to_string(),
            title: "Test Item".to_string(),
            price_minor: 500,
        });
        OrchestratorFacade::new(
            Arc::new(catalog),
            Arc::new(MockPricingProvider),
            Arc::new(MockTaxProvider),
            Arc::new(MockGeoProvider),
            Arc::new(MockPaymentProvider),
            Arc::new(MockReceiptProvider),
            PolicyEngine::default(),
        )
    }

    fn dev_auth() -> AuthContext {
        AuthContext {
            caller_id: "dev".to_string(),
            tenant_id: "dev".to_string(),
            scopes: vec!["checkout:execute".to_string()],
        }
    }

    #[tokio::test]
    async fn initialize_returns_server_info() {
        let facade = build_facade();
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: serde_json::json!(1),
            method: "initialize".to_string(),
            params: None,
        };
        let resp = handle_mcp_request(&req, &facade, &dev_auth()).await;
        let result = resp.result.unwrap();
        assert_eq!(result["serverInfo"]["name"], "commerce-orchestrator");
    }

    #[tokio::test]
    async fn tools_list_returns_all_tools() {
        let facade = build_facade();
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: serde_json::json!(1),
            method: "tools/list".to_string(),
            params: None,
        };
        let resp = handle_mcp_request(&req, &facade, &dev_auth()).await;
        let result = resp.result.unwrap();
        let tools = result["tools"].as_array().unwrap();
        assert!(tools.len() >= 14);
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"create_cart"));
        assert!(names.contains(&"execute_checkout"));
        assert!(names.contains(&"lookup_catalog_item"));
    }

    #[tokio::test]
    async fn tool_call_create_cart() {
        let facade = build_facade();
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: serde_json::json!(1),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({
                "name": "create_cart",
                "arguments": {
                    "merchant_id": "m1",
                    "currency": "USD"
                }
            })),
        };
        let resp = handle_mcp_request(&req, &facade, &dev_auth()).await;
        assert!(
            resp.error.is_none(),
            "expected no error, got {:?}",
            resp.error
        );
        let result = resp.result.unwrap();
        assert!(result.get("cart_id").is_some());
        assert_eq!(result["currency"], "USD");
    }

    #[tokio::test]
    async fn tool_call_lookup_catalog_item() {
        let facade = build_facade();
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: serde_json::json!(1),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({
                "name": "lookup_catalog_item",
                "arguments": { "item_id": "item_1" }
            })),
        };
        let resp = handle_mcp_request(&req, &facade, &dev_auth()).await;
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        assert_eq!(result["id"], "item_1");
        assert_eq!(result["price_minor"], 500);
    }

    #[tokio::test]
    async fn unknown_method_returns_error() {
        let facade = build_facade();
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: serde_json::json!(1),
            method: "foo/bar".to_string(),
            params: None,
        };
        let resp = handle_mcp_request(&req, &facade, &dev_auth()).await;
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, crate::jsonrpc::METHOD_NOT_FOUND);
    }
}
