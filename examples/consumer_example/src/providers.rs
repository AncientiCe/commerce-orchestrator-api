//! Example "my app" provider adapters.
//!
//! In a real consumer app you would implement each trait against your own backend
//! (catalog service, payment gateway, etc.). Here we wrap the in-repo mocks so
//! the example runs without external services; the wiring pattern is identical.

use async_trait::async_trait;
use orchestrator_core::contract::{
    CartProjection, CheckoutRequest, PaymentLifecycleRequest, TransactionResult,
};
use provider_contracts::{
    AuthResult, CatalogError, CatalogItem, CatalogProvider, GeoCheckResult, GeoError, GeoProvider,
    LinePrice, PaymentError, PaymentOperationResult, PaymentProvider, PricingError,
    PricingProvider, ReceiptError, ReceiptPayload, ReceiptProvider, TaxError, TaxProvider,
    TaxResult,
};
use provider_mocks::{
    MockCatalogProvider, MockGeoProvider, MockPaymentProvider, MockPricingProvider,
    MockReceiptProvider, MockTaxProvider,
};
use std::sync::Arc;

/// Your catalog adapter (here: wraps mock; replace with your catalog service).
pub struct MyCatalogProvider {
    inner: MockCatalogProvider,
}

impl Default for MyCatalogProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl MyCatalogProvider {
    pub fn new() -> Self {
        Self {
            inner: MockCatalogProvider::new(),
        }
    }

    pub fn add_item(&self, item: CatalogItem) {
        self.inner.add_item(item);
    }
}

#[async_trait]
impl CatalogProvider for MyCatalogProvider {
    async fn get_item(&self, item_id: &str) -> Result<CatalogItem, CatalogError> {
        self.inner.get_item(item_id).await
    }
}

/// Your pricing adapter (here: wraps mock; replace with your pricing service).
pub struct MyPricingProvider(pub Arc<MockPricingProvider>);

#[async_trait]
impl PricingProvider for MyPricingProvider {
    async fn resolve_prices(&self, cart: &CartProjection) -> Result<Vec<LinePrice>, PricingError> {
        self.0.resolve_prices(cart).await
    }
}

/// Your tax adapter (here: wraps mock; replace with your tax service).
pub struct MyTaxProvider(pub Arc<MockTaxProvider>);

#[async_trait]
impl TaxProvider for MyTaxProvider {
    async fn resolve_tax(&self, cart: &CartProjection) -> Result<TaxResult, TaxError> {
        self.0.resolve_tax(cart).await
    }
}

/// Your geo adapter (here: wraps mock; replace with your geo/shipping service).
pub struct MyGeoProvider(pub Arc<MockGeoProvider>);

#[async_trait]
impl GeoProvider for MyGeoProvider {
    async fn check(
        &self,
        cart: &CartProjection,
        request: &CheckoutRequest,
    ) -> Result<GeoCheckResult, GeoError> {
        self.0.check(cart, request).await
    }
}

/// Your payment adapter (here: wraps mock; replace with your payment gateway).
pub struct MyPaymentProvider(pub Arc<MockPaymentProvider>);

#[async_trait]
impl PaymentProvider for MyPaymentProvider {
    async fn authorize(&self, request: &CheckoutRequest) -> Result<AuthResult, PaymentError> {
        self.0.authorize(request).await
    }

    async fn capture(
        &self,
        request: &PaymentLifecycleRequest,
    ) -> Result<PaymentOperationResult, PaymentError> {
        self.0.capture(request).await
    }

    async fn void(
        &self,
        request: &PaymentLifecycleRequest,
    ) -> Result<PaymentOperationResult, PaymentError> {
        self.0.void(request).await
    }

    async fn refund(
        &self,
        request: &PaymentLifecycleRequest,
    ) -> Result<PaymentOperationResult, PaymentError> {
        self.0.refund(request).await
    }
}

/// Your receipt adapter (here: wraps mock; replace with your receipt service).
pub struct MyReceiptProvider(pub Arc<MockReceiptProvider>);

#[async_trait]
impl ReceiptProvider for MyReceiptProvider {
    async fn generate(
        &self,
        cart: &CartProjection,
        result: &TransactionResult,
    ) -> Result<ReceiptPayload, ReceiptError> {
        self.0.generate(cart, result).await
    }
}
