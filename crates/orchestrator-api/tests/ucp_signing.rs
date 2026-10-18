//! UCP message signatures: RFC 9421 signature bases, RFC 9530 Content-Digest,
//! required coverage, freshness, key lookup, and Ed25519 + ES256 algorithms.

use orchestrator_api::{
    content_digest, verify_content_digest, KeyAlgorithm, KeySpec, MessageComponents,
    SignatureError, SigningKeyring, VerifyingKeyring, MAX_SIGNATURE_SKEW_SECONDS,
};

const AGENT_SEED: &str = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8";
const AGENT_SEED_B: &str = "IB8eHRwbGhkYFxYVFBMSERAPDg0MCwoJCAcGBQQDAgE";
/// A valid P-256 private scalar (base64url, 32 bytes).
const P256_SECRET: &str = "AQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHyA";

const NOW: i64 = 1_760_000_000;

fn ed_keyring(kid: &str, seed: &str) -> SigningKeyring {
    SigningKeyring::from_seeds(kid, seed, &[]).expect("ed25519 keyring")
}

fn es_keyring(kid: &str) -> SigningKeyring {
    SigningKeyring::from_specs(
        KeySpec {
            kid,
            algorithm: KeyAlgorithm::Es256,
            secret: P256_SECRET,
        },
        &[],
    )
    .expect("es256 keyring")
}

/// The verifier an orchestrator builds from the JWKs an agent publishes.
fn verifier_for(keyrings: &[&SigningKeyring]) -> VerifyingKeyring {
    let keys: Vec<_> = keyrings.iter().flat_map(|k| k.public_jwks()).collect();
    let jwks = serde_json::json!({ "keys": keys }).to_string();
    VerifyingKeyring::from_jwks(&jwks).expect("verifying keyring")
}

const BODY: &[u8] = br#"{"line_items":[]}"#;

fn request_components() -> MessageComponents {
    MessageComponents::new()
        .with("@method", "POST")
        .with("@authority", "merchant.example.com")
        .with("@path", "/api/v1/ucp/cart")
        .with("content-type", "application/json")
        .with("content-digest", &content_digest(BODY))
        .with("idempotency-key", "550e8400-e29b-41d4-a716-446655440000")
}

const COVERED: &[&str] = &[
    "@method",
    "@authority",
    "@path",
    "idempotency-key",
    "content-digest",
    "content-type",
];

fn required() -> Vec<String> {
    COVERED.iter().map(|c| (*c).to_string()).collect()
}

#[test]
fn content_digest_is_sha256_in_structured_field_binary_form() {
    // sha-256 of the empty string, RFC 9530 shape.
    assert_eq!(
        content_digest(b""),
        "sha-256=:47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=:"
    );
}

#[test]
fn a_tampered_body_fails_the_digest_check() {
    let header = content_digest(BODY);
    verify_content_digest(&header, BODY).expect("matching body");
    assert_eq!(
        verify_content_digest(&header, br#"{"line_items":[1]}"#),
        Err(SignatureError::DigestMismatch)
    );
    assert_eq!(
        verify_content_digest("sha-512=:AAAA:", BODY),
        Err(SignatureError::DigestMismatch),
        "sha-256 is mandatory"
    );
}

#[test]
fn signature_input_lists_covered_components_created_and_keyid() {
    let agent = ed_keyring("agent-1", AGENT_SEED);
    let signed = agent
        .sign_message(&request_components(), COVERED, NOW)
        .unwrap();

    assert_eq!(
        signed.signature_input,
        format!(
            "sig1=(\"@method\" \"@authority\" \"@path\" \"idempotency-key\" \
             \"content-digest\" \"content-type\");created={NOW};keyid=\"agent-1\""
        )
    );
    assert!(signed.signature.starts_with("sig1=:") && signed.signature.ends_with(':'));
}

#[test]
fn ed25519_signed_request_verifies() {
    let agent = ed_keyring("agent-1", AGENT_SEED);
    let signed = agent
        .sign_message(&request_components(), COVERED, NOW)
        .unwrap();

    let kid = verifier_for(&[&agent])
        .verify_message(
            &request_components(),
            &signed.signature_input,
            &signed.signature,
            &required(),
            NOW + 2,
        )
        .expect("valid signature is accepted");
    assert_eq!(kid, "agent-1");
}

#[test]
fn es256_signed_request_verifies() {
    // ES256 is the algorithm every UCP verifier must support.
    let agent = es_keyring("agent-ec");
    let jwk = &agent.public_jwks()[0];
    assert_eq!(
        (jwk.kty.as_str(), jwk.crv.as_deref(), jwk.alg.as_str()),
        ("EC", Some("P-256"), "ES256")
    );
    assert!(jwk.x.is_some() && jwk.y.is_some());

    let signed = agent
        .sign_message(&request_components(), COVERED, NOW)
        .unwrap();
    verifier_for(&[&agent])
        .verify_message(
            &request_components(),
            &signed.signature_input,
            &signed.signature,
            &required(),
            NOW,
        )
        .expect("ES256 signature is accepted");
}

#[test]
fn a_changed_component_fails_verification() {
    let agent = ed_keyring("agent-1", AGENT_SEED);
    let signed = agent
        .sign_message(&request_components(), COVERED, NOW)
        .unwrap();
    let moved = request_components().with("@path", "/api/v1/ucp/checkout");

    assert_eq!(
        verifier_for(&[&agent]).verify_message(
            &moved,
            &signed.signature_input,
            &signed.signature,
            &required(),
            NOW,
        ),
        Err(SignatureError::VerificationFailed)
    );
}

#[test]
fn a_signature_by_another_key_with_the_same_kid_is_rejected() {
    let agent = ed_keyring("agent-1", AGENT_SEED);
    let rogue = ed_keyring("agent-1", AGENT_SEED_B);
    let signed = rogue
        .sign_message(&request_components(), COVERED, NOW)
        .unwrap();

    assert_eq!(
        verifier_for(&[&agent]).verify_message(
            &request_components(),
            &signed.signature_input,
            &signed.signature,
            &required(),
            NOW,
        ),
        Err(SignatureError::VerificationFailed)
    );
}

#[test]
fn an_unknown_keyid_is_key_not_found() {
    let agent = ed_keyring("agent-1", AGENT_SEED);
    let stranger = ed_keyring("agent-x", AGENT_SEED_B);
    let signed = stranger
        .sign_message(&request_components(), COVERED, NOW)
        .unwrap();

    let error = verifier_for(&[&agent])
        .verify_message(
            &request_components(),
            &signed.signature_input,
            &signed.signature,
            &required(),
            NOW,
        )
        .unwrap_err();
    assert_eq!(error, SignatureError::KeyNotFound("agent-x".to_string()));
    assert_eq!((error.code(), error.http_status()), ("key_not_found", 401));
}

#[test]
fn a_signature_that_leaves_out_a_required_component_is_rejected() {
    // Signing only the method would let the body or path be swapped freely.
    let agent = ed_keyring("agent-1", AGENT_SEED);
    let signed = agent
        .sign_message(&request_components(), &["@method", "@path"], NOW)
        .unwrap();

    assert_eq!(
        verifier_for(&[&agent]).verify_message(
            &request_components(),
            &signed.signature_input,
            &signed.signature,
            &required(),
            NOW,
        ),
        Err(SignatureError::CoverageInsufficient(
            "@authority".to_string()
        ))
    );
}

#[test]
fn a_stale_created_parameter_is_rejected() {
    let agent = ed_keyring("agent-1", AGENT_SEED);
    let created = NOW - MAX_SIGNATURE_SKEW_SECONDS - 60;
    let signed = agent
        .sign_message(&request_components(), COVERED, created)
        .unwrap();

    let error = verifier_for(&[&agent])
        .verify_message(
            &request_components(),
            &signed.signature_input,
            &signed.signature,
            &required(),
            NOW,
        )
        .unwrap_err();
    assert!(matches!(error, SignatureError::Stale { .. }), "{error:?}");
    assert_eq!(error.code(), "signature_invalid");
}

#[test]
fn an_expired_signature_is_rejected() {
    let agent = ed_keyring("agent-1", AGENT_SEED);
    let resigned = agent
        .sign_message_with_params(&request_components(), COVERED, NOW, Some(NOW + 10))
        .unwrap();
    assert!(resigned
        .signature_input
        .ends_with(&format!(";expires={}", NOW + 10)));

    assert_eq!(
        verifier_for(&[&agent]).verify_message(
            &request_components(),
            &resigned.signature_input,
            &resigned.signature,
            &required(),
            NOW + 11,
        ),
        Err(SignatureError::Expired)
    );
}

#[test]
fn missing_headers_are_signature_missing() {
    let agent = ed_keyring("agent-1", AGENT_SEED);
    let error = verifier_for(&[&agent])
        .verify_message(&request_components(), "", "", &required(), NOW)
        .unwrap_err();
    assert_eq!(error, SignatureError::Missing);
    assert_eq!(
        (error.code(), error.http_status()),
        ("signature_missing", 401)
    );
}

#[test]
fn a_mismatched_alg_parameter_is_algorithm_unsupported() {
    let agent = ed_keyring("agent-1", AGENT_SEED);
    let signed = agent
        .sign_message(&request_components(), COVERED, NOW)
        .unwrap();
    let input = format!("{};alg=\"rsa-pss-sha512\"", signed.signature_input);

    let error = verifier_for(&[&agent])
        .verify_message(
            &request_components(),
            &input,
            &signed.signature,
            &required(),
            NOW,
        )
        .unwrap_err();
    assert_eq!(
        error,
        SignatureError::AlgorithmUnsupported("rsa-pss-sha512".to_string())
    );
    assert_eq!(error.http_status(), 400);
}

#[test]
fn one_valid_signature_among_several_is_enough() {
    let agent = ed_keyring("agent-1", AGENT_SEED);
    let stranger = ed_keyring("agent-x", AGENT_SEED_B);
    let good = agent
        .sign_message(&request_components(), COVERED, NOW)
        .unwrap();
    let bad = stranger
        .sign_message(&request_components(), COVERED, NOW)
        .unwrap();
    // Relabel the stranger's signature so both can travel together.
    let input = format!(
        "{}, {}",
        bad.signature_input.replacen("sig1=", "proxy=", 1),
        good.signature_input
    );
    let signature = format!(
        "{}, {}",
        bad.signature.replacen("sig1=", "proxy=", 1),
        good.signature
    );

    let kid = verifier_for(&[&agent])
        .verify_message(&request_components(), &input, &signature, &required(), NOW)
        .expect("the agent's own signature verifies");
    assert_eq!(kid, "agent-1");
}

#[test]
fn rfc_9421_ed25519_test_vector_signature_base_and_value() {
    // RFC 9421 Appendix B.2.6 (test-key-ed25519).
    let keyring = SigningKeyring::from_seeds(
        "test-key-ed25519",
        "n4Ni-HpISpVObnQMW0wOhCKROaIKqKtW_2ZYb2p9KcU",
        &[],
    )
    .unwrap();
    let components = MessageComponents::new()
        .with("date", "Tue, 20 Apr 2021 02:07:55 GMT")
        .with("@method", "POST")
        .with("@path", "/foo")
        .with("@authority", "example.com")
        .with("content-type", "application/json")
        .with("content-length", "18");
    let signed = keyring
        .sign_message(
            &components,
            &[
                "date",
                "@method",
                "@path",
                "@authority",
                "content-type",
                "content-length",
            ],
            1_618_884_473,
        )
        .unwrap();

    assert_eq!(
        signed.signature,
        "sig1=:wqcAqbmYJ2ji2glfAMaRy4gruYYnx2nEFN2HN6jrnDnQCK1u02Gb04v9EDgwUPiu4A0w6vuQv5lIp5WPpBKRCw==:"
    );
}

#[test]
fn both_kids_verify_during_a_rotation() {
    let old = ed_keyring("agent-2025", AGENT_SEED);
    let new = es_keyring("agent-2026");
    let verifier = verifier_for(&[&old, &new]);

    for keyring in [&old, &new] {
        let signed = keyring
            .sign_message(&request_components(), COVERED, NOW)
            .unwrap();
        verifier
            .verify_message(
                &request_components(),
                &signed.signature_input,
                &signed.signature,
                &required(),
                NOW,
            )
            .expect("both kids accepted during rotation");
    }
}

#[test]
fn legacy_kid_and_raw_public_key_pairs_still_build_a_verifier() {
    let agent = ed_keyring("agent-1", AGENT_SEED);
    let x = agent.public_jwks()[0].x.clone().unwrap();
    let verifier = VerifyingKeyring::from_public_keys(&[("agent-1", x.as_str())]).unwrap();
    let signed = agent
        .sign_message(&request_components(), COVERED, NOW)
        .unwrap();
    verifier
        .verify_message(
            &request_components(),
            &signed.signature_input,
            &signed.signature,
            &required(),
            NOW,
        )
        .expect("kid:x pairs remain a valid way to configure Ed25519 agent keys");
}

#[test]
fn a_jwks_with_an_unsupported_key_type_is_refused() {
    let jwks = r#"{"keys":[{"kid":"rsa-1","kty":"RSA","n":"AQAB","e":"AQAB"}]}"#;
    assert!(VerifyingKeyring::from_jwks(jwks).is_err());
}
