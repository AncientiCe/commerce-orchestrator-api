//! Cart stream event types for event sourcing.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CartStreamEvent {
    Created {
        merchant_id: String,
        currency: String,
    },
    ItemAdded {
        line_id: String,
        item_id: String,
        quantity: u32,
    },
    ItemQtyUpdated {
        line_id: String,
        quantity: u32,
    },
    ItemRemoved {
        line_id: String,
    },
    AdjustmentApplied {
        code: String,
    },
    Repriced,
    Retaxed {
        tax_minor: i64,
    },
    GeoChecked {
        allowed: bool,
    },
    CheckoutReady,
}
