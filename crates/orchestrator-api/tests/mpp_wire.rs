//! Published MPP HMAC vectors and hostile credential binding cases.
use orchestrator_api::mpp::{MppChallenge, MppCredential};
use serde_json::json;

fn challenge() -> MppChallenge {
    MppChallenge {
        id: String::new(),
        realm: "api.example.com".into(),
        method: "tempo".into(),
        intent: "charge".into(),
        request: "eyJhbW91bnQiOiIxMDAwMDAwIn0".into(),
        expires: None,
        digest: None,
        opaque: None,
        header: None,
        description: None,
    }
}
#[test]
fn published_hmac_vectors_match_both_header_layouts() {
    let mut c = challenge();
    c.bind(b"test-vector-secret").unwrap();
    assert_eq!(c.id, "X6v1eo7fJ76gAxqY0xN9Jd__4lUyDDYmriryOM-5FO4");
    c.header = Some("Payment-Authorization".into());
    c.bind(b"test-vector-secret").unwrap();
    assert_eq!(c.id, "S91xi-OFGZPMs-j7GsX0FDpIkmCcZT1P9XyV58WNy_U");
    c.opaque = Some("eyJwaSI6InBpXzEyMyJ9".into());
    c.bind(b"test-vector-secret").unwrap();
    assert_eq!(c.id, "CJ4X1O4aTDmS59hfdhnhBtxIQjWDOf0bcrhsswwMOW8");
}
#[test]
fn modified_amounts_and_header_choices_fail_binding() {
    let mut c = challenge();
    c.bind(b"secret").unwrap();
    c.request = "eyJhbW91bnQiOiIxIn0".into();
    assert!(c.verify_binding(b"secret").is_err());
    let mut c = challenge();
    c.bind(b"secret").unwrap();
    c.header = Some("Payment-Authorization".into());
    assert!(c.verify_binding(b"secret").is_err());
}
#[test]
fn credentials_round_trip_and_require_an_object_payload() {
    let mut c = challenge();
    c.bind(b"secret").unwrap();
    let credential = MppCredential {
        challenge: c,
        payload: json!({"proof":"opaque"}),
        source: None,
    };
    let wire = credential.to_header().unwrap();
    assert_eq!(
        MppCredential::from_header(&wire).unwrap().payload,
        credential.payload
    );
    let mut invalid = credential;
    invalid.payload = json!("not an object");
    assert!(invalid.to_header().is_err());
    assert!(MppCredential::from_header("Payment padded==").is_err());
}
#[test]
fn malformed_or_noncanonical_challenges_cannot_be_bound() {
    let mut c = challenge();
    c.realm = "api.example.com\r\nX-Evil: yes".into();
    assert!(c.bind(b"secret").is_err());
    let mut c = challenge();
    c.header = Some("Authorization".into());
    assert!(c.bind(b"secret").is_err());
    let mut c = challenge();
    c.request = "not-json".into();
    assert!(c.bind(b"secret").is_err());
    let mut c = challenge();
    assert!(c.bind(b"").is_err());
}
