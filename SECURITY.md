# Security

## Authentication and authorization

- Use `execute_checkout_authorized` with an `AuthContext` (tenant_id, scopes) so that only callers with scope `checkout:execute` and matching tenant can run checkout.
- The HTTP service accepts any `AuthnResolver` (see `orchestrator_api::AuthnResolver`): turn bearer tokens into `AuthContext` in your API layer.
- **Static token (default):** `StaticTokenAuthnResolver` validates a single shared secret from `AUTH_BEARER_TOKEN`. Suitable for simple deployments; rotate the token periodically and keep it in a secret store.
- **Production identity (JWT/OIDC):** For production, implement `AuthnResolver` with JWT validation: verify signature via JWKS, validate issuer and audience, and map claims to `AuthContext` (e.g. `tenant_id`, `caller_id`, `scopes`). Deploy this resolver in the HTTP server instead of `StaticTokenAuthnResolver` so the service never uses a single shared static token. Use short-lived access tokens and refresh as needed.
- Idempotency keys are scoped by tenant: different tenants never share idempotency state.

## AP2 (Agent Payments Protocol) strict mode

- Set `AP2_STRICT=1` (or `true`/`yes`) to require valid AP2 artifacts on checkout. In `v0.2.0`, strict mode expects `ap2_consent_proof` to be a JSON string with `issuer`, `subject`, `mandate_id`, `payment_handler_id`, `issued_at`, `expires_at`, and `signature`, and it verifies expiry plus handler binding before checkout proceeds.
- Optionally set `AP2_TRUSTED_ISSUERS=issuer-a,issuer-b` to restrict which consent proof issuers are accepted in strict mode.
- You can still plug in a custom verifier via `orchestrator_api::Ap2MandateVerifier` when integrating a fuller AP2 credential stack with real signature validation and replay protection.

## PII and sensitive data

- Never log raw `CheckoutRequest` or payment tokens. Use `redact_checkout_request()` from `orchestrator_api::pii` for any logging or audit payloads.
- Redacted fields: payment token/reference, AP2 consent proof, customer email and full name.

## Reporting vulnerabilities

Please report security issues privately to the maintainers; do not open public issues for vulnerabilities.
