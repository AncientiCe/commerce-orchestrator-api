//! Signing keys must be real, configured, and never a published test vector.
//!
//! Discovery used to advertise the RFC 8037 Ed25519 test key, so anyone verifying
//! against it failed and anyone trusting it was trusting a key whose private half
//! is printed in an RFC.

use orchestrator_api::signing::{SigningKeyError, SigningKeyring};

/// Deterministic 32-byte seed, base64url, for tests only.
const SEED_A: &str = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8";
const SEED_B: &str = "IB8eHRwbGhkYFxYVFBMSERAPDg0MCwoJCAcGBQQDAgE";
/// The RFC 8037 test vector public key that used to be advertised.
const RFC_8037_TEST_VECTOR_X: &str = "11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo";

fn keyring() -> SigningKeyring {
    SigningKeyring::from_seeds("orch-2026-a", SEED_A, &[]).expect("valid keyring")
}

#[test]
fn a_keyring_publishes_the_public_half_as_a_jwk() {
    let jwks = keyring().public_jwks();

    assert_eq!(jwks.len(), 1);
    assert_eq!(jwks[0].kid, "orch-2026-a");
    assert_eq!(jwks[0].kty, "OKP");
    assert_eq!(jwks[0].crv.as_deref(), Some("Ed25519"));
    assert_eq!(jwks[0].alg, "EdDSA");
    assert_eq!(jwks[0].use_.as_deref(), Some("sig"));
    let x = jwks[0].x.as_deref().expect("public key material");
    assert!(!x.is_empty());
    assert_ne!(
        x, RFC_8037_TEST_VECTOR_X,
        "a published test vector must never be advertised as a signing key"
    );
}

#[test]
fn a_signature_verifies_against_the_advertised_key() {
    let keyring = keyring();
    let signature = keyring.sign(b"payload");

    assert_eq!(signature.kid, "orch-2026-a");
    keyring
        .verify("orch-2026-a", b"payload", &signature.signature)
        .expect("its own signature must verify");
}

#[test]
fn a_tampered_payload_fails_verification() {
    let keyring = keyring();
    let signature = keyring.sign(b"payload");

    let err = keyring
        .verify("orch-2026-a", b"payl0ad", &signature.signature)
        .expect_err("a changed payload must not verify");
    assert!(matches!(err, SigningKeyError::VerificationFailed));
}

#[test]
fn an_unknown_kid_is_rejected() {
    let err = keyring()
        .verify("some-other-key", b"payload", "AAAA")
        .expect_err("an unknown kid must be rejected");
    assert!(matches!(err, SigningKeyError::UnknownKid(_)));
}

#[test]
fn rotation_keeps_verifying_the_previous_key_but_signs_with_the_active_one() {
    let keyring = SigningKeyring::from_seeds("orch-2026-b", SEED_B, &[("orch-2026-a", SEED_A)])
        .expect("valid keyring");

    assert_eq!(
        keyring.sign(b"payload").kid,
        "orch-2026-b",
        "new traffic is signed with the active key"
    );

    let jwks = keyring.public_jwks();
    assert_eq!(jwks.len(), 2, "both keys stay advertised during rotation");
    assert_eq!(
        jwks[0].kid, "orch-2026-b",
        "the active key is advertised first"
    );

    let old = SigningKeyring::from_seeds("orch-2026-a", SEED_A, &[]).unwrap();
    let signed_with_old = old.sign(b"payload");
    keyring
        .verify("orch-2026-a", b"payload", &signed_with_old.signature)
        .expect("signatures made before rotation must still verify");
}

#[test]
fn a_duplicate_kid_is_refused() {
    let err = SigningKeyring::from_seeds("orch-2026-a", SEED_A, &[("orch-2026-a", SEED_B)])
        .expect_err("two keys cannot share a kid");
    assert!(matches!(err, SigningKeyError::DuplicateKid(_)));
}

#[test]
fn malformed_key_material_is_refused_at_load_time() {
    let err = SigningKeyring::from_seeds("orch-2026-a", "not-base64!!", &[])
        .expect_err("garbage key material must not load");
    assert!(matches!(err, SigningKeyError::InvalidKeyMaterial { .. }));

    let err = SigningKeyring::from_seeds("orch-2026-a", "c2hvcnQ", &[])
        .expect_err("a short seed must not load");
    assert!(matches!(err, SigningKeyError::InvalidKeyMaterial { .. }));
}

#[test]
fn an_empty_kid_is_refused() {
    let err = SigningKeyring::from_seeds("", SEED_A, &[]).expect_err("a key needs an id");
    assert!(matches!(err, SigningKeyError::MissingKid));
}
