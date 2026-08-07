//! OpenAPI generation for REST consumers.

use utoipa::OpenApi;

use crate::dto::*;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Commerce Orchestrator API",
        version = "0.8.0",
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
