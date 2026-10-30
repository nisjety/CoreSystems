use aws_lc_rs::{
    rand::SystemRandom,
    rsa::KeySize,
    signature::{KeyPair, RsaKeyPair, RSA_PKCS1_SHA256},
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::Utc;
use jsonwebtoken::jwk::JwkSet;
use serde_json::{json, Value};
use xero_auth::identity::verify_id_token_with_jwks;

fn signer() -> (RsaKeyPair, JwkSet) {
    let key = RsaKeyPair::generate(KeySize::Rsa2048).unwrap();
    let public = key.public_key();
    let jwks: JwkSet = serde_json::from_value(json!({"keys":[{
        "kty":"RSA", "kid":"test-key", "alg":"RS256", "use":"sig",
        "n":URL_SAFE_NO_PAD.encode(public.modulus().big_endian_without_leading_zero()),
        "e":URL_SAFE_NO_PAD.encode(public.exponent().big_endian_without_leading_zero())
    }]}))
    .unwrap();
    (key, jwks)
}

fn claims(nonce: &str) -> Value {
    let now = Utc::now().timestamp();
    json!({
        "iss":"https://identity.xero.com",
        "aud":"xero-app-id",
        "sub":"xero-person-1",
        "exp":now+300,
        "iat":now,
        "nonce":nonce,
        "email":"user@example.com"
    })
}

fn signed_claims(key: &RsaKeyPair, claims: &Value) -> String {
    let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","typ":"JWT","kid":"test-key"}"#);
    let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims).unwrap());
    let message = format!("{header}.{payload}");
    let mut signature = vec![0; key.public_modulus_len()];
    key.sign(
        &RSA_PKCS1_SHA256,
        &SystemRandom::new(),
        message.as_bytes(),
        &mut signature,
    )
    .unwrap();
    format!("{message}.{}", URL_SAFE_NO_PAD.encode(signature))
}

fn signed_identity(nonce: &str) -> (String, JwkSet) {
    let (key, jwks) = signer();
    let token = signed_claims(&key, &claims(nonce));
    (token, jwks)
}

#[test]
fn verifies_xero_identity_and_rejects_wrong_nonce_or_audience() {
    let (token, jwks) = signed_identity("expected-nonce");
    let identity =
        verify_id_token_with_jwks(&token, &jwks, "xero-app-id", "expected-nonce").unwrap();
    assert_eq!(identity.subject, "xero-person-1");
    assert_eq!(identity.email.as_deref(), Some("user@example.com"));
    assert!(verify_id_token_with_jwks(&token, &jwks, "xero-app-id", "other-nonce").is_err());
    assert!(verify_id_token_with_jwks(&token, &jwks, "another-app", "expected-nonce").is_err());
}

#[test]
fn rejects_tampering_and_unknown_signing_key() {
    let (token, jwks) = signed_identity("nonce");
    let mut parts = token.split('.').map(str::to_owned).collect::<Vec<_>>();
    let replacement = if parts[1].starts_with('a') { "b" } else { "a" };
    parts[1].replace_range(0..1, replacement);
    assert!(verify_id_token_with_jwks(&parts.join("."), &jwks, "xero-app-id", "nonce").is_err());
    let empty: JwkSet = serde_json::from_value(json!({"keys":[]})).unwrap();
    assert!(verify_id_token_with_jwks(&token, &empty, "xero-app-id", "nonce").is_err());
}

#[test]
fn rejects_invalid_claims_even_with_a_valid_signature() {
    let (key, jwks) = signer();
    let base = claims("nonce");
    for (field, replacement) in [
        ("iss", json!("https://attacker.example")),
        ("aud", json!(["xero-app-id", "other-app"])),
        ("sub", json!("")),
        ("exp", json!(Utc::now().timestamp() - 300)),
        ("iat", json!(Utc::now().timestamp() + 300)),
        ("nonce", json!("other-nonce")),
    ] {
        let mut bad = base.clone();
        bad[field] = replacement;
        assert!(
            verify_id_token_with_jwks(&signed_claims(&key, &bad), &jwks, "xero-app-id", "nonce")
                .is_err(),
            "accepted invalid {field}"
        );
    }
    assert!(verify_id_token_with_jwks(&signed_claims(&key, &base), &jwks, "", "nonce").is_err());
    assert!(
        verify_id_token_with_jwks(&signed_claims(&key, &base), &jwks, "xero-app-id", "").is_err()
    );
    assert!(verify_id_token_with_jwks(&"x".repeat(8193), &jwks, "xero-app-id", "nonce").is_err());
}
