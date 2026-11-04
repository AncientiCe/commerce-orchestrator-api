//! Web Bot Auth identities are bound to the directory that supplies the key.
use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine,
};
use orchestrator_api::{MessageComponents, SigningKeyring, WebBotSignature};
use serde_json::json;
use sha2::{Digest, Sha256};

fn fixture(agent: &str, tag: &str, expires: i64) -> (String, String, serde_json::Value) {
    let key = SigningKeyring::from_seeds(
        "operator-label",
        "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8",
        &[],
    )
    .unwrap();
    let mut jwk = serde_json::to_value(&key.public_jwks()[0]).unwrap();
    let canonical = json!({"crv":jwk["crv"],"kty":jwk["kty"],"x":jwk["x"]}).to_string();
    let thumb = URL_SAFE_NO_PAD.encode(Sha256::digest(canonical));
    jwk["kid"] = json!(thumb);
    let params = format!("(\"@authority\" \"signature-agent\";key=\"sig1\");created=100;expires={expires};keyid=\"{thumb}\";tag=\"{tag}\"");
    let base = format!("\"@authority\": shop.example\n\"signature-agent\";key=\"sig1\": \"{agent}\"\n\"@signature-params\": {params}");
    let signature = STANDARD.encode(
        URL_SAFE_NO_PAD
            .decode(key.sign(base.as_bytes()).signature)
            .unwrap(),
    );
    (
        format!("sig1={params}"),
        format!("sig1=:{signature}:"),
        json!({"keys":[jwk]}),
    )
}

#[test]
fn a_verified_identity_uses_the_resolved_url_and_key_thumbprint() {
    let agent = "https://agent.example/keys?revision=1";
    let (input, signature, jwks) = fixture(agent, "web-bot-auth", 200);
    let header = format!("sig1=\"{agent}\"");
    let proof = WebBotSignature::parse(&input, &header).unwrap().remove(0);
    let components = MessageComponents::new()
        .with("@authority", "shop.example")
        .with("signature-agent", &header);
    let identity = proof.verify(&components, &signature, &jwks, 150).unwrap();
    assert_eq!(identity.agent_url, "https://agent.example/keys");
}

#[test]
fn unknown_tags_and_insecure_directory_urls_are_rejected() {
    let (input, _, _) = fixture("https://agent.example/keys", "unrelated", 200);
    assert!(WebBotSignature::parse(&input, "sig1=\"https://agent.example/keys\"").is_err());
    let (input, _, _) = fixture("http://agent.example/keys", "web-bot-auth", 200);
    assert!(WebBotSignature::parse(&input, "sig1=\"http://agent.example/keys\"").is_err());
}

#[test]
fn expired_signatures_and_expired_keys_are_rejected() {
    let agent = "https://agent.example/keys";
    let (input, signature, mut jwks) = fixture(agent, "web-bot-auth", 200);
    let header = format!("sig1=\"{agent}\"");
    let proof = WebBotSignature::parse(&input, &header).unwrap().remove(0);
    let components = MessageComponents::new()
        .with("@authority", "shop.example")
        .with("signature-agent", &header);
    assert!(proof.verify(&components, &signature, &jwks, 201).is_err());
    jwks["keys"][0]["exp"] = json!(140);
    assert!(proof.verify(&components, &signature, &jwks, 150).is_err());
}

#[test]
fn an_unrelated_directory_cannot_verify_the_request() {
    let agent = "https://agent.example/keys";
    let (input, signature, _) = fixture(agent, "web-bot-auth", 200);
    let header = format!("sig1=\"{agent}\"");
    let proof = WebBotSignature::parse(&input, &header).unwrap().remove(0);
    let components = MessageComponents::new()
        .with("@authority", "shop.example")
        .with("signature-agent", &header);
    assert!(proof
        .verify(&components, &signature, &json!({"keys":[]}), 150)
        .is_err());
}

#[test]
fn duplicate_security_parameters_are_not_ambiguous() {
    let (input, _, _) = fixture("https://agent.example/keys", "web-bot-auth", 200);
    assert!(WebBotSignature::parse(
        &(input + ";expires=300"),
        "sig1=\"https://agent.example/keys\""
    )
    .is_err());
}
