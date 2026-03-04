//! Policy enforcement hooks: max totals, blocked geos, payment constraints.

use crate::contract::CheckoutRequest;

#[derive(Debug, Clone)]
pub struct PolicyEngine {
    pub max_total_minor: Option<i64>,
    pub blocked_country_codes: Vec<String>,
    pub require_payment_reference: bool,
}

impl Default for PolicyEngine {
    fn default() -> Self {
        Self {
            max_total_minor: None,
            blocked_country_codes: Vec::new(),
            require_payment_reference: true,
        }
    }
}

impl PolicyEngine {
    pub fn check_checkout(&self, req: &CheckoutRequest, total_minor: i64) -> PolicyCheckResult {
        let mut errors = Vec::new();
        if let Some(max) = self.max_total_minor {
            if total_minor > max {
                errors.push(format!("total {} exceeds max allowed {}", total_minor, max));
            }
        }
        if let Some(ref loc) = req.location {
            if let Some(ref cc) = loc.country_code {
                if self
                    .blocked_country_codes
                    .iter()
                    .any(|b| b.eq_ignore_ascii_case(cc))
                {
                    errors.push(format!("country {} is not allowed", cc));
                }
            }
        }
        if self.require_payment_reference && req.payment_intent.token_or_reference.is_empty() {
            errors.push("payment token or reference required".to_string());
        }
        if errors.is_empty() {
            PolicyCheckResult::Allowed
        } else {
            PolicyCheckResult::Rejected(errors)
        }
    }
}

#[derive(Debug, Clone)]
pub enum PolicyCheckResult {
    Allowed,
    Rejected(Vec<String>),
}
