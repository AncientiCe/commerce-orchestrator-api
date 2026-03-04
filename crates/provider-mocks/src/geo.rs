//! Mock geo provider.

use orchestrator_core::contract::{CartProjection, CheckoutRequest};
use provider_contracts::{GeoCheckResult, GeoProvider};

#[derive(Default)]
pub struct MockGeoProvider;

#[async_trait::async_trait]
impl GeoProvider for MockGeoProvider {
    async fn check(
        &self,
        _cart: &CartProjection,
        request: &CheckoutRequest,
    ) -> Result<GeoCheckResult, provider_contracts::GeoError> {
        let allowed = request
            .location
            .as_ref()
            .and_then(|l| l.country_code.as_ref())
            .map(|cc| cc != "ZZ")
            .unwrap_or(true);
        Ok(GeoCheckResult { allowed })
    }
}
