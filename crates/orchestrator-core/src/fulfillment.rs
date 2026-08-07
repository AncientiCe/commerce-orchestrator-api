//! UCP fulfillment extension (`dev.ucp.shopping.fulfillment`) domain types: shipping/pickup
//! methods, destinations, groups, and options. Orchestrator-native; maps to UCP without a hard
//! dependency on UCP wire format. See `ucp.dev/2026-04-08/specification/fulfillment`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum FulfillmentMethodType {
    Shipping,
    Pickup,
}

/// Postal address fields shared by shipping destinations and retail locations.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostalAddress {
    #[serde(default)]
    pub extended_address: Option<String>,
    #[serde(default)]
    pub street_address: Option<String>,
    #[serde(default)]
    pub address_locality: Option<String>,
    #[serde(default)]
    pub address_region: Option<String>,
    #[serde(default)]
    pub address_country: Option<String>,
    #[serde(default)]
    pub postal_code: Option<String>,
    #[serde(default)]
    pub first_name: Option<String>,
    #[serde(default)]
    pub last_name: Option<String>,
    #[serde(default)]
    pub phone_number: Option<String>,
}

/// A destination is either a shipping address or a retail (pickup) location.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FulfillmentDestination {
    Shipping {
        id: String,
        address: PostalAddress,
    },
    Retail {
        id: String,
        name: String,
        #[serde(default)]
        address: Option<PostalAddress>,
    },
}

impl FulfillmentDestination {
    pub fn id(&self) -> &str {
        match self {
            Self::Shipping { id, .. } | Self::Retail { id, .. } => id,
        }
    }
}

/// A single quoted fulfillment option (e.g. standard/express shipping, in-store pickup).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FulfillmentOption {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub carrier: Option<String>,
    #[serde(default)]
    pub earliest_fulfillment_time: Option<String>,
    #[serde(default)]
    pub latest_fulfillment_time: Option<String>,
    pub amount_minor: i64,
}

/// A group of line items sharing the same set of fulfillment options and selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FulfillmentGroup {
    pub id: String,
    pub line_item_ids: Vec<String>,
    pub options: Vec<FulfillmentOption>,
    #[serde(default)]
    pub selected_option_id: Option<String>,
}

/// A fulfillment method (shipping or pickup) covering a set of line items, with candidate
/// destinations and per-group option quotes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FulfillmentMethod {
    pub id: String,
    pub method_type: FulfillmentMethodType,
    pub line_item_ids: Vec<String>,
    pub destinations: Vec<FulfillmentDestination>,
    #[serde(default)]
    pub selected_destination_id: Option<String>,
    pub groups: Vec<FulfillmentGroup>,
}

/// Fulfillment state attached to a cart/checkout projection.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FulfillmentState {
    pub methods: Vec<FulfillmentMethod>,
}

/// Built-in fulfillment rate quotes (standard/express shipping, free in-store pickup).
/// Orchestrator-native placeholder pricing until a dedicated shipping-rate provider is
/// introduced; mirrors the UCP fulfillment extension's option shape.
pub fn default_options_for_method(method_type: FulfillmentMethodType) -> Vec<FulfillmentOption> {
    match method_type {
        FulfillmentMethodType::Shipping => vec![
            FulfillmentOption {
                id: "standard".to_string(),
                title: "Standard Shipping".to_string(),
                description: Some("Arrives in 5-8 business days".to_string()),
                carrier: None,
                earliest_fulfillment_time: None,
                latest_fulfillment_time: None,
                amount_minor: 500,
            },
            FulfillmentOption {
                id: "express".to_string(),
                title: "Express Shipping".to_string(),
                description: Some("Arrives in 2-3 business days".to_string()),
                carrier: None,
                earliest_fulfillment_time: None,
                latest_fulfillment_time: None,
                amount_minor: 1_000,
            },
        ],
        FulfillmentMethodType::Pickup => vec![FulfillmentOption {
            id: "in_store".to_string(),
            title: "In-Store Pickup".to_string(),
            description: Some("Ready for pickup today".to_string()),
            carrier: None,
            earliest_fulfillment_time: None,
            latest_fulfillment_time: None,
            amount_minor: 0,
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipping_options_include_standard_and_express_with_positive_cost() {
        let options = default_options_for_method(FulfillmentMethodType::Shipping);
        assert_eq!(options.len(), 2);
        assert!(options
            .iter()
            .any(|o| o.id == "standard" && o.amount_minor == 500));
        assert!(options
            .iter()
            .any(|o| o.id == "express" && o.amount_minor == 1_000));
    }

    #[test]
    fn pickup_options_are_free() {
        let options = default_options_for_method(FulfillmentMethodType::Pickup);
        assert_eq!(options.len(), 1);
        assert_eq!(options[0].amount_minor, 0);
    }

    #[test]
    fn destination_id_reads_through_either_variant() {
        let shipping = FulfillmentDestination::Shipping {
            id: "dest_1".to_string(),
            address: PostalAddress::default(),
        };
        let retail = FulfillmentDestination::Retail {
            id: "dest_2".to_string(),
            name: "Downtown Store".to_string(),
            address: None,
        };
        assert_eq!(shipping.id(), "dest_1");
        assert_eq!(retail.id(), "dest_2");
    }
}
