# Consumption guide: dependencies and provider skeleton

How to add the Commerce Orchestrator as a dependency and implement the six provider traits in your own crate. Use this together with [consumer-integration.md](consumer-integration.md).

## Adding the dependency

In your application's `Cargo.toml`, depend on the orchestrator via git. For integration you need `orchestrator-api` (facade), `provider-contracts` (traits), and `orchestrator-core` (contract types such as `CartCommand`, `CheckoutRequest`, `PolicyEngine`).

### Option A: Git tag (recommended for production)

Pin to a released tag so upgrades are explicit and versioned.

```toml
[dependencies]
orchestrator-api = { git = "https://github.com/your-org/commerce-orchestrator", tag = "v0.1.0" }
provider-contracts = { git = "https://github.com/your-org/commerce-orchestrator", tag = "v0.1.0" }
orchestrator-core = { git = "https://github.com/your-org/commerce-orchestrator", tag = "v0.1.0" }

tokio = { version = "1", features = ["full"] }
tracing = "0.1"
```

Replace `your-org` and the tag with your actual repo and release tag (see [release-checklist.md](release-checklist.md) and `CHANGELOG.md`).

### Option B: Git revision (for a specific commit)

Use when you need a pre-release or a precise commit.

```toml
orchestrator-api = { git = "https://github.com/your-org/commerce-orchestrator", rev = "abc1234" }
provider-contracts = { git = "https://github.com/your-org/commerce-orchestrator", rev = "abc1234" }
orchestrator-core = { git = "https://github.com/your-org/commerce-orchestrator", rev = "abc1234" }
```

### Option C: Internal registry

If you publish the workspace crates to a private registry, depend on the published names and versions:

```toml
orchestrator-api = "0.1.0"
provider-contracts = "0.1.0"
orchestrator-core = "0.1.0"
```

Configure the registry in `.cargo/config.toml` or `Cargo.toml` as usual.

---

## Minimal provider-implementation skeleton

Implement all six traits from `provider-contracts`; wrap each in `Arc<dyn Trait>` and pass them into `OrchestratorFacade::new`. Below is a minimal skeleton: one full implementation (catalog) and the pattern for the others.

### 1. Catalog provider (full example)

```rust
use async_trait::async_trait;
use provider_contracts::{CatalogItem, CatalogProvider, CatalogError};

pub struct MyCatalogProvider {
    // Your backend (DB, API client, etc.)
}

impl MyCatalogProvider {
    pub fn new(/* ... */) -> Self {
        Self { /* ... */ }
    }
}

#[async_trait]
impl CatalogProvider for MyCatalogProvider {
    async fn get_item(&self, item_id: &str) -> Result<CatalogItem, CatalogError> {
        // Look up item_id in your catalog; return CatalogItem { id, title, price_minor }.
        // Return Err(CatalogError::NotFound(item_id.to_string())) if missing.
        todo!("integrate your catalog backend")
    }
}
```

### 2. Pricing provider

```rust
use async_trait::async_trait;
use orchestrator_core::contract::CartProjection;
use provider_contracts::{PricingProvider, PricingError, LinePrice};

pub struct MyPricingProvider { /* ... */ }

#[async_trait]
impl PricingProvider for MyPricingProvider {
    async fn resolve_prices(&self, cart: &CartProjection) -> Result<Vec<LinePrice>, PricingError> {
        // For each line in cart, compute unit_price_minor and total_minor; return Vec<LinePrice>.
        todo!("integrate your pricing backend")
    }
}
```

### 3. Tax provider

```rust
use async_trait::async_trait;
use orchestrator_core::contract::CartProjection;
use provider_contracts::{TaxProvider, TaxError, TaxResult};

pub struct MyTaxProvider { /* ... */ }

#[async_trait]
impl TaxProvider for MyTaxProvider {
    async fn resolve_tax(&self, cart: &CartProjection) -> Result<TaxResult, TaxError> {
        // Compute total_tax_minor from cart and your tax rules.
        todo!("integrate your tax backend")
    }
}
```

### 4. Geo provider

```rust
use async_trait::async_trait;
use orchestrator_core::contract::{CartProjection, CheckoutRequest};
use provider_contracts::{GeoProvider, GeoError, GeoCheckResult};

pub struct MyGeoProvider { /* ... */ }

#[async_trait]
impl GeoProvider for MyGeoProvider {
    async fn check(
        &self,
        cart: &CartProjection,
        request: &CheckoutRequest,
    ) -> Result<GeoCheckResult, GeoError> {
        // Evaluate shipping/region rules; return GeoCheckResult { allowed: true/false }.
        todo!("integrate your geo backend")
    }
}
```

### 5. Payment provider

```rust
use async_trait::async_trait;
use orchestrator_core::contract::{CheckoutRequest, PaymentLifecycleRequest};
use provider_contracts::{PaymentProvider, AuthResult, PaymentOperationResult, PaymentError};

pub struct MyPaymentProvider { /* ... */ }

#[async_trait]
impl PaymentProvider for MyPaymentProvider {
    async fn authorize(&self, request: &CheckoutRequest) -> Result<AuthResult, PaymentError> {
        // Authorize payment; return AuthResult { authorized, reference }.
        todo!("integrate your payment gateway")
    }
    async fn capture(&self, request: &PaymentLifecycleRequest) -> Result<PaymentOperationResult, PaymentError> {
        todo!("integrate capture")
    }
    async fn void(&self, request: &PaymentLifecycleRequest) -> Result<PaymentOperationResult, PaymentError> {
        todo!("integrate void")
    }
    async fn refund(&self, request: &PaymentLifecycleRequest) -> Result<PaymentOperationResult, PaymentError> {
        todo!("integrate refund")
    }
}
```

### 6. Receipt provider

```rust
use async_trait::async_trait;
use orchestrator_core::contract::{CartProjection, TransactionResult};
use provider_contracts::{ReceiptProvider, ReceiptError, ReceiptPayload};

pub struct MyReceiptProvider { /* ... */ }

#[async_trait]
impl ReceiptProvider for MyReceiptProvider {
    async fn generate(
        &self,
        cart: &CartProjection,
        result: &TransactionResult,
    ) -> Result<ReceiptPayload, ReceiptError> {
        // Build receipt content (e.g. HTML or PDF); return ReceiptPayload { content }.
        todo!("integrate your receipt generation")
    }
}
```

### Wiring the facade

In your application startup (e.g. `main` or app factory):

```rust
use std::sync::Arc;
use orchestrator_api::OrchestratorFacade;
use orchestrator_core::policy::PolicyEngine;

let catalog = Arc::new(MyCatalogProvider::new(/* ... */));
let pricing = Arc::new(MyPricingProvider::new(/* ... */));
let tax = Arc::new(MyTaxProvider::new(/* ... */));
let geo = Arc::new(MyGeoProvider::new(/* ... */));
let payment = Arc::new(MyPaymentProvider::new(/* ... */));
let receipt = Arc::new(MyReceiptProvider::new(/* ... */));

let facade = OrchestratorFacade::new(
    catalog,
    pricing,
    tax,
    geo,
    payment,
    receipt,
    PolicyEngine::default(),
);

// Use facade.dispatch_cart_command(...), facade.execute_checkout(...), etc.
```

For persistent file-backed stores (event store, idempotency, outbox), use `OrchestratorFacade::new_persistent(..., base_path).await` instead of `new`.

---

## Next steps

- Run the in-repo [happy_path](../examples/happy_path/) example to see a full flow with mocks.
- See the [consumer example](../examples/consumer_example/) for a copy-paste template that wires custom providers and runs a happy-path checkout test.
