# Security

## Authentication and authorization

- Use `execute_checkout_authorized` with an `AuthContext` (tenant_id, scopes) so that only callers with scope `checkout:execute` and matching tenant can run checkout.
- Implement `AuthnResolver` in your API layer (e.g. HTTP middleware) to turn bearer tokens into `AuthContext` before calling the facade.
- Idempotency keys are scoped by tenant: different tenants never share idempotency state.

## PII and sensitive data

- Never log raw `CheckoutRequest` or payment tokens. Use `redact_checkout_request()` from `orchestrator_api::pii` for any logging or audit payloads.
- Redacted fields: payment token/reference, AP2 consent proof, customer email and full name.

## Reporting vulnerabilities

Please report security issues privately to the maintainers; do not open public issues for vulnerabilities.
