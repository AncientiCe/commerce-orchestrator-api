//! Discounts applied to a cart, as returned by the pricing provider.
//!
//! Maps to the UCP shopping discount extension (`dev.ucp.shopping.discount`).

use serde::{Deserialize, Serialize};

/// A discount the pricing provider decided to grant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppliedDiscount {
    /// The adjustment code the buyer supplied.
    pub code: String,
    /// Human-readable reason, suitable for showing to the buyer.
    #[serde(default)]
    pub description: Option<String>,
    /// Amount deducted, in minor units. Always positive.
    pub amount_minor: i64,
    /// Line this applies to; `None` means the whole order.
    #[serde(default)]
    pub line_id: Option<String>,
}

/// Total of a set of discounts, clamped to `owed` so a total can never go negative.
pub fn total_discount_minor(discounts: &[AppliedDiscount], owed: i64) -> i64 {
    let raw: i64 = discounts
        .iter()
        .map(|d| d.amount_minor.max(0))
        .fold(0i64, |acc, amount| acc.saturating_add(amount));
    raw.min(owed.max(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn discount(amount_minor: i64) -> AppliedDiscount {
        AppliedDiscount {
            code: "C".to_string(),
            description: None,
            amount_minor,
            line_id: None,
        }
    }

    #[test]
    fn sums_multiple_discounts() {
        assert_eq!(
            total_discount_minor(&[discount(100), discount(50)], 1000),
            150
        );
    }

    #[test]
    fn clamps_to_the_amount_owed() {
        assert_eq!(total_discount_minor(&[discount(5000)], 1000), 1000);
    }

    #[test]
    fn ignores_negative_amounts() {
        assert_eq!(
            total_discount_minor(&[discount(-500), discount(100)], 1000),
            100
        );
    }

    #[test]
    fn nothing_owed_means_nothing_discounted() {
        assert_eq!(total_discount_minor(&[discount(100)], 0), 0);
    }
}
