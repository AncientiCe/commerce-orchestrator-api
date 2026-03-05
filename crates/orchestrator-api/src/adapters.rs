//! Phase 2 adapter scaffolding for UCP/A2A/AP2 interop.
//! Normalizes A2A-style envelopes into domain types so the same facade and authz apply.

use orchestrator_core::contract::{CartCommand, CartId, CheckoutRequest};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UcpCheckoutEnvelope {
    pub capability: String,
    pub payload: CheckoutRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct A2AHandoffProfile {
    pub protocol: String,
    pub version: String,
    pub delegated_capability: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ap2PaymentMetadata {
    pub handler_id: Option<String>,
    pub consent_proof: Option<String>,
}

pub fn extract_ap2_metadata(request: &CheckoutRequest) -> Ap2PaymentMetadata {
    Ap2PaymentMetadata {
        handler_id: request.payment_intent.payment_handler_id.clone(),
        consent_proof: request.payment_intent.ap2_consent_proof.clone(),
    }
}

/// Checkout-related capability IDs that this adapter accepts for checkout execute.
const CHECKOUT_CAPABILITIES: &[&str] = &[
    "dev.ucp.shopping.checkout",
    "checkout",
    "ucp.shopping.checkout",
];

/// Cart/shopping capability IDs accepted for cart commands.
const CART_CAPABILITIES: &[&str] = &[
    "dev.ucp.shopping.checkout",
    "dev.ucp.shopping.discount",
    "ucp.shopping.cart",
    "cart",
];

/// Normalize an A2A-style checkout envelope (JSON) into a CheckoutRequest.
/// Expects `{ "capability": "<id>", "payload": { ... CheckoutRequest shape ... } }`.
/// Returns an error if capability is not checkout-related or payload fails to parse.
pub fn normalize_a2a_checkout_envelope(value: &serde_json::Value) -> Result<CheckoutRequest, String> {
    let obj = value
        .as_object()
        .ok_or_else(|| "A2A envelope must be a JSON object".to_string())?;
    let capability = obj
        .get("capability")
        .and_then(|c| c.as_str())
        .ok_or_else(|| "missing or invalid capability".to_string())?;
    if !CHECKOUT_CAPABILITIES
        .iter()
        .any(|c| *c == capability || capability.ends_with(".checkout"))
    {
        return Err(format!("unsupported checkout capability: {}", capability));
    }
    let payload = obj
        .get("payload")
        .ok_or_else(|| "missing payload".to_string())?;
    serde_json::from_value(payload.clone()).map_err(|e| format!("invalid checkout payload: {}", e))
}

/// A2A cart envelope payload: command + optional cart_id (same shape as REST cart/commands body).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct A2ACartPayload {
    pub command: serde_json::Value,
    #[serde(default)]
    pub cart_id: Option<String>,
}

/// Normalize an A2A-style cart envelope into (CartCommand, Option<CartId>).
/// Expects `{ "capability": "<id>", "payload": { "command": { "kind": "...", ... }, "cart_id": "..."? } }`.
pub fn normalize_a2a_cart_envelope(
    value: &serde_json::Value,
) -> Result<(CartCommand, Option<CartId>), String> {
    let obj = value
        .as_object()
        .ok_or_else(|| "A2A envelope must be a JSON object".to_string())?;
    let capability = obj
        .get("capability")
        .and_then(|c| c.as_str())
        .ok_or_else(|| "missing or invalid capability".to_string())?;
    if !CART_CAPABILITIES
        .iter()
        .any(|c| *c == capability)
        && !capability.contains("cart")
        && !capability.contains("checkout")
        && !capability.contains("discount")
    {
        return Err(format!("unsupported cart capability: {}", capability));
    }
    let payload = obj
        .get("payload")
        .ok_or_else(|| "missing payload".to_string())?;
    let cart_id = payload
        .get("cart_id")
        .and_then(|c| c.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| Uuid::from_str(s).map(CartId))
        .transpose()
        .map_err(|e| format!("invalid cart_id: {}", e))?;
    let cmd_value = payload.get("command").ok_or_else(|| "missing command".to_string())?;
    let cmd = cart_value_to_command(cmd_value)?;
    Ok((cmd, cart_id))
}

fn cart_value_to_command(v: &serde_json::Value) -> Result<CartCommand, String> {
    use orchestrator_core::contract::*;
    let kind = v
        .get("kind")
        .and_then(|k| k.as_str())
        .ok_or_else(|| "command must have kind".to_string())?;
    let parse_cart_id = |s: &str| Uuid::from_str(s).map(CartId).map_err(|e| e.to_string());
    match kind {
        "create_cart" => {
            let merchant_id = v.get("merchant_id").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let currency = v.get("currency").and_then(|x| x.as_str()).unwrap_or("").to_string();
            Ok(CartCommand::CreateCart(CreateCartPayload {
                merchant_id,
                currency,
            }))
        }
        "add_item" => {
            let item_id = v.get("item_id").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let quantity = v.get("quantity").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
            Ok(CartCommand::AddItem(AddItemPayload { item_id, quantity }))
        }
        "update_item_qty" => {
            let line_id = v.get("line_id").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let quantity = v.get("quantity").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
            Ok(CartCommand::UpdateItemQty(UpdateItemQtyPayload {
                line_id,
                quantity,
            }))
        }
        "remove_item" => {
            let line_id = v.get("line_id").and_then(|x| x.as_str()).unwrap_or("").to_string();
            Ok(CartCommand::RemoveItem(RemoveItemPayload { line_id }))
        }
        "apply_adjustment" => {
            let code = v.get("code").and_then(|x| x.as_str()).unwrap_or("").to_string();
            Ok(CartCommand::ApplyAdjustment(ApplyAdjustmentPayload { code }))
        }
        "get_cart" => {
            let cart_id = v.get("cart_id").and_then(|x| x.as_str()).ok_or("missing cart_id")?;
            Ok(CartCommand::GetCart(GetCartPayload {
                cart_id: parse_cart_id(cart_id)?,
            }))
        }
        "start_checkout" => {
            let cart_id = v.get("cart_id").and_then(|x| x.as_str()).ok_or("missing cart_id")?;
            let cart_version = v.get("cart_version").and_then(|x| x.as_u64()).unwrap_or(1);
            Ok(CartCommand::StartCheckout(StartCheckoutPayload {
                cart_id: parse_cart_id(cart_id)?,
                cart_version,
            }))
        }
        _ => Err(format!("unknown command kind: {}", kind)),
    }
}
