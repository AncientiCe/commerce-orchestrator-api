//! HTTP-based adapters for external component APIs.

pub mod catalog;
pub mod client;
pub mod error;
pub mod geo;
pub mod payment;
pub mod pricing;
pub mod receipt;
pub mod tax;

pub use catalog::CatalogHttpAdapter;
pub use client::{build_client, get_with_retry, post_json_with_retry, ClientConfig};
pub use error::AdapterError;
pub use geo::GeoHttpAdapter;
pub use payment::PaymentHttpAdapter;
pub use pricing::PricingHttpAdapter;
pub use receipt::ReceiptHttpAdapter;
pub use tax::TaxHttpAdapter;
