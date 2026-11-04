//! Web Bot Auth dictionary-member coverage on RFC 9421 messages.
use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine,
};
use orchestrator_api::{MessageComponents, SigningKeyring, VerifyingKeyring};

fn verify(header: &str, component: &str) -> bool {
    let keys =
        SigningKeyring::from_seeds("agent", "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8", &[])
            .unwrap();
    let verifier =
        VerifyingKeyring::from_jwks(&serde_json::json!({"keys": keys.public_jwks()}).to_string())
            .unwrap();
    let params = "(\"@authority\" \"signature-agent\";key=\"sig1\");created=100;keyid=\"agent\";tag=\"web-bot-auth\"";
    let base = format!("\"@authority\": shop.example\n\"signature-agent\";key=\"sig1\": {component}\n\"@signature-params\": {params}");
    let signature = keys.sign(base.as_bytes());
    let signature = STANDARD.encode(URL_SAFE_NO_PAD.decode(signature.signature).unwrap());
    let components = MessageComponents::new()
        .with("@authority", "shop.example")
        .with("signature-agent", header);
    verifier
        .verify_message(
            &components,
            &format!("sig1={params}"),
            &format!("sig1=:{signature}:"),
            &[
                "@authority".into(),
                "\"signature-agent\";key=\"sig1\"".into(),
            ],
            100,
        )
        .is_ok()
}

#[test]
fn a_dictionary_member_is_verified_independently_of_other_members() {
    assert!(verify(
        "other=\"https://other.example\", sig1=\"https://agent.example\"",
        "\"https://agent.example\""
    ));
}

#[test]
fn changing_the_signed_member_invalidates_the_signature() {
    assert!(!verify(
        "sig1=\"https://attacker.example\"",
        "\"https://agent.example\""
    ));
}

#[test]
fn another_members_value_cannot_substitute_for_the_selected_member() {
    assert!(!verify(
        "other=\"https://agent.example\"",
        "\"https://agent.example\""
    ));
}
