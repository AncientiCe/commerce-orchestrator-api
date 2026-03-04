//! Schema validation and policy prechecks for cart commands and checkout.

use crate::contract::{CartCommand, CheckoutRequest, PaymentIntent};

#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<String>,
}

impl ValidationResult {
    pub fn ok() -> Self {
        Self {
            valid: true,
            errors: Vec::new(),
        }
    }

    pub fn invalid(errors: Vec<String>) -> Self {
        Self {
            valid: false,
            errors,
        }
    }
}

pub fn validate_cart_command(cmd: &CartCommand) -> ValidationResult {
    let mut errors = Vec::new();
    match cmd {
        CartCommand::CreateCart(p) => {
            if p.merchant_id.is_empty() {
                errors.push("merchant_id required".to_string());
            }
            if p.currency.is_empty() || p.currency.len() != 3 {
                errors.push("currency must be 3-letter code".to_string());
            }
        }
        CartCommand::AddItem(p) => {
            if p.item_id.is_empty() {
                errors.push("item_id required".to_string());
            }
            if p.quantity == 0 {
                errors.push("quantity must be > 0".to_string());
            }
        }
        CartCommand::UpdateItemQty(p) => {
            if p.line_id.is_empty() {
                errors.push("line_id required".to_string());
            }
        }
        CartCommand::RemoveItem(p) => {
            if p.line_id.is_empty() {
                errors.push("line_id required".to_string());
            }
        }
        CartCommand::GetCart(p) => {
            let _ = p;
        }
        CartCommand::StartCheckout(p) => {
            if p.cart_version == 0 {
                errors.push("cart_version must be > 0".to_string());
            }
        }
        CartCommand::ApplyAdjustment(p) => {
            if p.code.is_empty() {
                errors.push("adjustment code required".to_string());
            }
        }
    }
    if errors.is_empty() {
        ValidationResult::ok()
    } else {
        ValidationResult::invalid(errors)
    }
}

pub fn validate_checkout_request(req: &CheckoutRequest) -> ValidationResult {
    let mut errors = Vec::new();
    if req.tenant_id.is_empty() {
        errors.push("tenant_id required".to_string());
    }
    if req.merchant_id.is_empty() {
        errors.push("merchant_id required".to_string());
    }
    if req.currency.is_empty() || req.currency.len() != 3 {
        errors.push("currency must be 3-letter code".to_string());
    }
    if req.cart_version == 0 {
        errors.push("cart_version must be > 0".to_string());
    }
    if req.idempotency_key.is_empty() {
        errors.push("idempotency_key required".to_string());
    }
    validate_payment_intent(&req.payment_intent, &mut errors);
    if errors.is_empty() {
        ValidationResult::ok()
    } else {
        ValidationResult::invalid(errors)
    }
}

fn validate_payment_intent(intent: &PaymentIntent, errors: &mut Vec<String>) {
    if intent.amount_minor < 0 {
        errors.push("payment amount must be >= 0".to_string());
    }
    if intent.token_or_reference.is_empty() {
        errors.push("payment token_or_reference required".to_string());
    }
    if intent
        .payment_handler_id
        .as_ref()
        .is_some_and(|handler| handler.is_empty())
    {
        errors.push("payment_handler_id cannot be empty".to_string());
    }
}
