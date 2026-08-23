//! HTTP-based adapters for external component APIs.

pub mod auth;
pub mod catalog;
pub mod circuit;
pub mod client;
pub mod delegation;
pub mod error;
pub mod fulfillment;
pub mod geo;
pub mod payment;
pub mod pricing;
pub mod receipt;
pub mod tax;

pub use auth::{
    OAuth2ClientAuth, OAuth2ClientCredentials, OAuth2TokenSource, OutboundAuth, TlsConfig,
};
pub use catalog::CatalogHttpAdapter;
pub use circuit::{CircuitBreaker, CircuitBreakerConfig, CircuitState};
pub use client::{
    build_client, get_with_retry, post_json, post_json_with_retry, ClientConfig, RequestOptions,
};
pub use delegation::{IdentityLinkHttpAdapter, PaymentDelegationHttpAdapter};
pub use error::AdapterError;
pub use fulfillment::FulfillmentHttpAdapter;
pub use geo::GeoHttpAdapter;
pub use payment::PaymentHttpAdapter;
pub use pricing::PricingHttpAdapter;
pub use receipt::ReceiptHttpAdapter;
pub use tax::TaxHttpAdapter;
