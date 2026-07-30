//! MCP tool definitions and dispatch for commerce operations.

use crate::jsonrpc::{
    JsonRpcRequest, JsonRpcResponse, INTERNAL_ERROR, METHOD_NOT_FOUND, UNSUPPORTED_PROTOCOL_VERSION,
};
use orchestrator_api::{AuthContext, OrchestratorFacade};
use orchestrator_core::contract::*;
use std::time::Instant;

pub const MCP_MODERN_VERSION: &str = "2026-07-28";
pub const MCP_LEGACY_VERSION: &str = "2024-11-05";
pub const MCP_LEGACY_2025_VERSION: &str = "2025-11-25";
pub const MCP_SUPPORTED_VERSIONS: &[&str] = &[
    MCP_MODERN_VERSION,
    MCP_LEGACY_2025_VERSION,
    MCP_LEGACY_VERSION,
];

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
        ToolDefinition {
            name: "lookup_catalog_items".to_string(),
            description: "Look up multiple catalog items by ID".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "item_ids": {
                        "type": "array",
                        "items": { "type": "string" }
                    }
                },
                "required": ["item_ids"]
            }),
        },
        ToolDefinition {
            name: "search_catalog_items".to_string(),
            description: "Search catalog items by query".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" }
                }
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
///
/// Dual-era support:
/// - Legacy (`2024-11-05` / `2025-11-25`): `initialize` handshake.
/// - Modern (`2026-07-28`): per-request protocol version via header and/or `_meta`.
pub async fn handle_mcp_request(
    request: &JsonRpcRequest,
    facade: &OrchestratorFacade,
    auth: &AuthContext,
) -> JsonRpcResponse {
    handle_mcp_request_with_version(request, facade, auth, None).await
}

pub async fn handle_mcp_request_with_version(
    request: &JsonRpcRequest,
    facade: &OrchestratorFacade,
    auth: &AuthContext,
    header_version: Option<&str>,
) -> JsonRpcResponse {
    let started = Instant::now();
    let meta_version = extract_meta_protocol_version(request);
    let negotiated = match negotiate_mcp_version(header_version, meta_version.as_deref(), request) {
        Ok(v) => v,
        Err(requested) => {
            orchestrator_observability::incr("mcp_version_unsupported_total");
            return JsonRpcResponse::error_with_data(
                request.id.clone(),
                UNSUPPORTED_PROTOCOL_VERSION,
                "Unsupported protocol version",
                serde_json::json!({
                    "supported": MCP_SUPPORTED_VERSIONS,
                    "requested": requested,
                }),
            );
        }
    };
    orchestrator_observability::incr(&format!(
        "mcp_version_selected_{}_total",
        negotiated.replace('-', "_")
    ));

    if header_version.is_some()
        && meta_version.is_some()
        && header_version.map(str::trim) != meta_version.as_deref().map(str::trim)
    {
        return JsonRpcResponse::error(
            request.id.clone(),
            -32600,
            "MCP-Protocol-Version header does not match params._meta protocolVersion",
        );
    }

    let result = match request.method.as_str() {
        "server/discover" => Ok(server_discover_payload()),
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
        "initialize" => {
            if negotiated == MCP_MODERN_VERSION {
                return JsonRpcResponse::error(
                    request.id.clone(),
                    METHOD_NOT_FOUND,
                    "initialize is not used in MCP 2026-07-28; call server/discover instead",
                );
            }
            Ok(serde_json::json!({
                "protocolVersion": negotiated,
                "capabilities": {
                    "tools": { "listChanged": false },
                    "resources": { "subscribe": false, "listChanged": false }
                },
                "serverInfo": {
                    "name": "commerce-orchestrator",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }))
        }
        "notifications/initialized" => Ok(serde_json::json!({})),
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
        Ok(mut value) => {
            if negotiated == MCP_MODERN_VERSION {
                if let Some(obj) = value.as_object_mut() {
                    let meta = obj.entry("_meta").or_insert_with(|| serde_json::json!({}));
                    if let Some(meta_obj) = meta.as_object_mut() {
                        meta_obj.insert(
                            "io.modelcontextprotocol/serverInfo".to_string(),
                            serde_json::json!({
                                "name": "commerce-orchestrator",
                                "version": env!("CARGO_PKG_VERSION")
                            }),
                        );
                        meta_obj.insert(
                            "io.modelcontextprotocol/protocolVersion".to_string(),
                            serde_json::json!(MCP_MODERN_VERSION),
                        );
                    }
                }
            }
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

fn server_discover_payload() -> serde_json::Value {
    serde_json::json!({
        "protocolVersions": MCP_SUPPORTED_VERSIONS,
        "capabilities": {
            "tools": { "listChanged": false },
            "resources": { "subscribe": false, "listChanged": false }
        },
        "serverInfo": {
            "name": "commerce-orchestrator",
            "version": env!("CARGO_PKG_VERSION")
        }
    })
}

fn extract_meta_protocol_version(request: &JsonRpcRequest) -> Option<String> {
    let params = request.params.as_ref()?;
    let meta = params.get("_meta")?;
    meta.get("io.modelcontextprotocol/protocolVersion")
        .or_else(|| meta.get("protocolVersion"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Negotiate MCP version. Returns Ok(version) or Err(requested_string).
fn negotiate_mcp_version(
    header_version: Option<&str>,
    meta_version: Option<&str>,
    request: &JsonRpcRequest,
) -> Result<&'static str, String> {
    let requested = header_version
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .or(meta_version.map(str::trim).filter(|v| !v.is_empty()));

    if let Some(v) = requested {
        return match_supported(v).ok_or_else(|| v.to_string());
    }

    // Legacy initialize without version defaults to 2024-11-05.
    if request.method == "initialize" || request.method == "notifications/initialized" {
        return Ok(MCP_LEGACY_VERSION);
    }

    // Modern clients must declare a version for non-discover calls; allow discover without.
    if request.method == "server/discover" {
        return Ok(MCP_MODERN_VERSION);
    }

    // Backward compat: no header → treat as legacy for tools/resources.
    Ok(MCP_LEGACY_VERSION)
}

fn match_supported(version: &str) -> Option<&'static str> {
    MCP_SUPPORTED_VERSIONS
        .iter()
        .find(|v| **v == version)
        .copied()
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
        "lookup_catalog_items" => {
            let item_ids = args
                .get("item_ids")
                .and_then(|value| value.as_array())
                .ok_or_else(|| "missing required field: item_ids".to_string())?
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .map(|item_id| item_id.to_string())
                        .ok_or_else(|| "item_ids must contain only strings".to_string())
                })
                .collect::<Result<Vec<_>, _>>()?;
            let items = facade
                .lookup_catalog_items(&item_ids)
                .await
                .map_err(|e| e.to_string())?;
            serde_json::to_value(items).map_err(|e| e.to_string())
        }
        "search_catalog_items" => {
            let query = args.get("query").and_then(|value| value.as_str());
            let items = facade
                .search_catalog_items(query)
                .await
                .map_err(|e| e.to_string())?;
            serde_json::to_value(items).map_err(|e| e.to_string())
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

    #[tokio::test]
    async fn server_discover_returns_supported_versions() {
        let facade = build_facade();
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: serde_json::json!(1),
            method: "server/discover".to_string(),
            params: None,
        };
        let resp =
            handle_mcp_request_with_version(&req, &facade, &dev_auth(), Some(MCP_MODERN_VERSION))
                .await;
        let result = resp.result.expect("discover result");
        let versions = result["protocolVersions"].as_array().unwrap();
        assert!(versions
            .iter()
            .any(|v| v.as_str() == Some(MCP_MODERN_VERSION)));
        assert!(versions
            .iter()
            .any(|v| v.as_str() == Some(MCP_LEGACY_VERSION)));
    }

    #[tokio::test]
    async fn modern_unsupported_version_returns_32022() {
        let facade = build_facade();
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: serde_json::json!(1),
            method: "tools/list".to_string(),
            params: None,
        };
        let resp =
            handle_mcp_request_with_version(&req, &facade, &dev_auth(), Some("1900-01-01")).await;
        let err = resp.error.expect("error");
        assert_eq!(err.code, crate::jsonrpc::UNSUPPORTED_PROTOCOL_VERSION);
        let data = err.data.unwrap();
        let supported = data["supported"].as_array().unwrap();
        assert!(supported
            .iter()
            .any(|v| v.as_str() == Some(MCP_MODERN_VERSION)));
    }

    #[tokio::test]
    async fn modern_tools_list_embeds_server_meta() {
        let facade = build_facade();
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: serde_json::json!(1),
            method: "tools/list".to_string(),
            params: Some(serde_json::json!({
                "_meta": {
                    "io.modelcontextprotocol/protocolVersion": MCP_MODERN_VERSION,
                    "io.modelcontextprotocol/clientInfo": { "name": "test", "version": "0" },
                    "io.modelcontextprotocol/clientCapabilities": {}
                }
            })),
        };
        let resp =
            handle_mcp_request_with_version(&req, &facade, &dev_auth(), Some(MCP_MODERN_VERSION))
                .await;
        let result = resp.result.unwrap();
        assert_eq!(
            result["_meta"]["io.modelcontextprotocol/protocolVersion"],
            MCP_MODERN_VERSION
        );
    }
}
