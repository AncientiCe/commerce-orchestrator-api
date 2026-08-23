//! UCP request/response signing: canonical bases, header parsing, inbound
//! verification with clock skew, and kid-based key rotation.

use orchestrator_api::{
    request_signing_base, response_signing_base, SignatureError, SignatureHeader, SigningKeyring,
    VerifyingKeyring, MAX_SIGNATURE_SKEW_SECONDS,
};

const AGENT_SEED: &str = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8";
const AGENT_SEED_B: &str = "IB8eHRwbGhkYFxYVFBMSERAPDg0MCwoJCAcGBQQDAgE";
const ORCH_SEED: &str = "MTIzNDU2Nzg5MDEyMzQ1Njc4OTAxMjM0NTY3ODkwMTI";

fn agent_keyring(kid: &str, seed: &str) -> SigningKeyring {
    SigningKeyring::from_seeds(kid, seed, &[]).expect("agent keyring")
}

/// The verifier the orchestrator builds from the agent's advertised public JWKs.
fn verifier_for(keyrings: &[&SigningKeyring]) -> VerifyingKeyring {
    let jwks: Vec<_> = keyrings
        .iter()
        .flat_map(|k| k.public_jwks())
        .map(|jwk| (jwk.kid, jwk.x.expect("ed25519 x")))
        .collect();
    let pairs: Vec<(&str, &str)> = jwks
        .iter()
        .map(|(kid, x)| (kid.as_str(), x.as_str()))
        .collect();
    VerifyingKeyring::from_public_keys(&pairs).expect("verifying keyring")
}

#[test]
fn a_correctly_signed_request_is_accepted() {
    let agent = agent_keyring("agent-1", AGENT_SEED);
    let verifier = verifier_for(&[&agent]);
    let body = br#"{"merchant_id":"m1"}"#;
    let now = 1_760_000_000;

    let signature = agent.sign(&request_signing_base(
        "POST",
        "/api/v1/ucp/carts",
        now,
        body,
    ));
    let header = SignatureHeader::new(signature.kid, signature.signature);

    verifier
        .verify_request(
            "POST",
            "/api/v1/ucp/carts",
            &header.to_string(),
            &now.to_string(),
            body,
            now + 2,
        )
        .expect("valid signature is accepted");
}

#[test]
fn a_tampered_body_fails_verification() {
    let agent = agent_keyring("agent-1", AGENT_SEED);
    let verifier = verifier_for(&[&agent]);
    let now = 1_760_000_000;
    let signature = agent.sign(&request_signing_base(
        "POST",
        "/api/v1/ucp/carts",
        now,
        br#"{"amount_minor":100}"#,
    ));
    let header = SignatureHeader::new(signature.kid, signature.signature);

    let err = verifier
        .verify_request(
            "POST",
            "/api/v1/ucp/carts",
            &header.to_string(),
            &now.to_string(),
            br#"{"amount_minor":1}"#,
            now,
        )
        .expect_err("a modified body must not verify");
    assert!(matches!(err, SignatureError::VerificationFailed));
}

#[test]
fn signing_is_bound_to_method_and_path() {
    let agent = agent_keyring("agent-1", AGENT_SEED);
    let verifier = verifier_for(&[&agent]);
    let now = 1_760_000_000;
    let body = b"{}";
    let signature = agent.sign(&request_signing_base(
        "POST",
        "/api/v1/ucp/carts",
        now,
        body,
    ));
    let header = SignatureHeader::new(signature.kid, signature.signature);

    let err = verifier
        .verify_request(
            "POST",
            "/api/v1/ucp/checkouts",
            &header.to_string(),
            &now.to_string(),
            body,
            now,
        )
        .expect_err("a signature must not be replayable against another path");
    assert!(matches!(err, SignatureError::VerificationFailed));
}

#[test]
fn a_stale_timestamp_is_rejected_even_when_the_signature_is_valid() {
    let agent = agent_keyring("agent-1", AGENT_SEED);
    let verifier = verifier_for(&[&agent]);
    let signed_at = 1_760_000_000;
    let body = b"{}";
    let signature = agent.sign(&request_signing_base(
        "POST",
        "/api/v1/ucp/carts",
        signed_at,
        body,
    ));
    let header = SignatureHeader::new(signature.kid, signature.signature);

    let err = verifier
        .verify_request(
            "POST",
            "/api/v1/ucp/carts",
            &header.to_string(),
            &signed_at.to_string(),
            body,
            signed_at + MAX_SIGNATURE_SKEW_SECONDS + 1,
        )
        .expect_err("replay outside the skew window must be rejected");
    assert!(matches!(err, SignatureError::StaleTimestamp { .. }));
}

#[test]
fn a_timestamp_from_the_future_is_rejected() {
    let agent = agent_keyring("agent-1", AGENT_SEED);
    let verifier = verifier_for(&[&agent]);
    let signed_at = 1_760_000_000;
    let body = b"{}";
    let signature = agent.sign(&request_signing_base(
        "POST",
        "/api/v1/ucp/carts",
        signed_at,
        body,
    ));
    let header = SignatureHeader::new(signature.kid, signature.signature);

    let err = verifier
        .verify_request(
            "POST",
            "/api/v1/ucp/carts",
            &header.to_string(),
            &signed_at.to_string(),
            body,
            signed_at - MAX_SIGNATURE_SKEW_SECONDS - 1,
        )
        .expect_err("a far-future timestamp must be rejected");
    assert!(matches!(err, SignatureError::StaleTimestamp { .. }));
}

#[test]
fn a_signature_from_an_unknown_kid_is_rejected() {
    let known = agent_keyring("agent-1", AGENT_SEED);
    let stranger = agent_keyring("agent-rogue", AGENT_SEED_B);
    let verifier = verifier_for(&[&known]);
    let now = 1_760_000_000;
    let body = b"{}";
    let signature = stranger.sign(&request_signing_base(
        "POST",
        "/api/v1/ucp/carts",
        now,
        body,
    ));
    let header = SignatureHeader::new(signature.kid, signature.signature);

    let err = verifier
        .verify_request(
            "POST",
            "/api/v1/ucp/carts",
            &header.to_string(),
            &now.to_string(),
            body,
            now,
        )
        .expect_err("an unknown kid must be rejected");
    assert!(matches!(err, SignatureError::UnknownKid(kid) if kid == "agent-rogue"));
}

#[test]
fn both_keys_verify_during_a_kid_rotation() {
    let old = agent_keyring("agent-2025", AGENT_SEED);
    let new = agent_keyring("agent-2026", AGENT_SEED_B);
    let verifier = verifier_for(&[&old, &new]);
    let now = 1_760_000_000;
    let body = b"{}";

    for keyring in [&old, &new] {
        let signature = keyring.sign(&request_signing_base(
            "POST",
            "/api/v1/ucp/carts",
            now,
            body,
        ));
        let header = SignatureHeader::new(signature.kid, signature.signature);
        verifier
            .verify_request(
                "POST",
                "/api/v1/ucp/carts",
                &header.to_string(),
                &now.to_string(),
                body,
                now,
            )
            .expect("both the outgoing and incoming key verify during rotation");
    }
}

#[test]
fn a_malformed_signature_header_is_reported_as_malformed() {
    let agent = agent_keyring("agent-1", AGENT_SEED);
    let verifier = verifier_for(&[&agent]);
    let err = verifier
        .verify_request(
            "POST",
            "/api/v1/ucp/carts",
            "garbage",
            "1760000000",
            b"{}",
            1_760_000_000,
        )
        .expect_err("a header without kid/sig is malformed");
    assert!(matches!(err, SignatureError::MalformedHeader(_)));
}

#[test]
fn a_non_numeric_timestamp_is_reported_as_invalid() {
    let agent = agent_keyring("agent-1", AGENT_SEED);
    let verifier = verifier_for(&[&agent]);
    let signature = agent.sign(&request_signing_base(
        "POST",
        "/api/v1/ucp/carts",
        1_760_000_000,
        b"{}",
    ));
    let header = SignatureHeader::new(signature.kid, signature.signature);
    let err = verifier
        .verify_request(
            "POST",
            "/api/v1/ucp/carts",
            &header.to_string(),
            "not-a-timestamp",
            b"{}",
            1_760_000_000,
        )
        .expect_err("timestamp must be unix seconds");
    assert!(matches!(err, SignatureError::InvalidTimestamp));
}

#[test]
fn an_unsupported_algorithm_is_rejected() {
    let agent = agent_keyring("agent-1", AGENT_SEED);
    let verifier = verifier_for(&[&agent]);
    let err = verifier
        .verify_request(
            "POST",
            "/api/v1/ucp/carts",
            r#"kid="agent-1",alg="hs256",sig="AAAA""#,
            "1760000000",
            b"{}",
            1_760_000_000,
        )
        .expect_err("only ed25519 is supported");
    assert!(matches!(err, SignatureError::UnsupportedAlgorithm(alg) if alg == "hs256"));
}

#[test]
fn a_signature_header_round_trips_through_parsing() {
    let header = SignatureHeader::new("orch-2026-a".to_string(), "c2ln".to_string());
    let parsed = SignatureHeader::parse(&header.to_string()).expect("round trip");
    assert_eq!(parsed.kid, "orch-2026-a");
    assert_eq!(parsed.signature, "c2ln");
    assert_eq!(parsed.algorithm, "ed25519");
}

#[test]
fn responses_are_signed_over_status_timestamp_and_body() {
    let orchestrator = SigningKeyring::from_seeds("orch-2026-a", ORCH_SEED, &[]).expect("keyring");
    let now = 1_760_000_000;
    let body = br#"{"id":"cart_1"}"#;
    let signature = orchestrator.sign(&response_signing_base(200, now, body));

    orchestrator
        .verify(
            &signature.kid,
            &response_signing_base(200, now, body),
            &signature.signature,
        )
        .expect("a client with the discovery JWK can verify the response");
    assert!(orchestrator
        .verify(
            &signature.kid,
            &response_signing_base(500, now, body),
            &signature.signature,
        )
        .is_err());
}

#[test]
fn a_verifying_keyring_rejects_malformed_public_key_material() {
    let err = VerifyingKeyring::from_public_keys(&[("agent-1", "not-base64!!")])
        .expect_err("garbage key material must fail loudly at config time");
    assert!(err.to_string().contains("agent-1"), "got: {err}");
}

#[test]
fn a_verifying_keyring_rejects_duplicate_kids() {
    let agent = agent_keyring("agent-1", AGENT_SEED);
    let x = agent.public_jwks()[0].x.clone().expect("x");
    let err =
        VerifyingKeyring::from_public_keys(&[("agent-1", x.as_str()), ("agent-1", x.as_str())])
            .expect_err("duplicate kids are ambiguous");
    assert!(err.to_string().contains("agent-1"), "got: {err}");
}
