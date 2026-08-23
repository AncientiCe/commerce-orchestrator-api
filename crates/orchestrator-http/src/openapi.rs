//! OpenAPI generation for REST consumers.

use utoipa::OpenApi;

use crate::dto::*;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Commerce Orchestrator API",
        version = env!("CARGO_PKG_VERSION"),
        description = "REST API surface for cart orchestration and checkout."
    ),
    components(schemas(
        CartCommandRequest,
        CartCommandDto,
        CartProjectionDto,
        CartLineProjectionDto,
        CartStatusDto,
        CheckoutRequestDto,
        CustomerHintDto,
        LocationHintDto,
        PaymentIntentDto,
        TransactionResultDto,
        TransactionStatusDto,
        TotalsBreakdownDto,
        PaymentStateDto,
        PaymentLifecycleRequestDto,
        PaymentOperationResultDto,
        IncomingEventRequestDto,
        IncomingEventResponseDto,
        ProcessOutboxRequestDto,
        DeadLetterEntryDto,
        ReplayDeadLetterRequestDto,
        ReplayDeadLetterResponseDto,
        ReconciliationRequestDto,
        PaymentMismatchDto,
        ReconciliationReportDto,
        UcpMetadataDto,
        IdentityLinkResultDto
    ))
)]
pub struct ApiDoc;

pub fn openapi_json() -> Result<String, String> {
    ApiDoc::openapi()
        .to_json()
        .map_err(|error| format!("openapi serialization failed: {}", error))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_published_spec_reports_the_release_version() {
        let spec: serde_json::Value =
            serde_json::from_str(&openapi_json().expect("openapi")).expect("valid json");
        assert_eq!(
            spec["info"]["version"].as_str(),
            Some(env!("CARGO_PKG_VERSION")),
            "consumers pin against this version; it must not drift from the crate"
        );
    }
}
