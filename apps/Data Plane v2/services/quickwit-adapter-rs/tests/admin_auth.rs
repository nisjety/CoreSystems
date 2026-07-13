use std::{
    sync::OnceLock,
    time::{SystemTime, UNIX_EPOCH},
};

use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use quickwit_adapter_rs::auth::{
    self, authorize_approval, authorize_rebuild, AdminClaims, AdminVerifier, RebuildAuthorization,
    RebuildIntent,
};
use rsa::{
    pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding},
    rand_core::OsRng,
    RsaPrivateKey,
};

fn claims(scopes: &[&str]) -> AdminClaims {
    AdminClaims {
        user_id: Some("operator-1".into()),
        service_id: None,
        principal_type: "user".into(),
        org_id: "org-a".into(),
        scopes: scopes.iter().map(|scope| (*scope).to_string()).collect(),
        issuer: "https://control.example/api/convex-auth".into(),
        audience: "data-plane".into(),
        subject: "operator-1".into(),
        expires_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_secs()
            + 300,
    }
}

fn service_claims(scopes: &[&str]) -> AdminClaims {
    AdminClaims {
        user_id: None,
        service_id: Some("service:search-operator".into()),
        principal_type: "service".into(),
        org_id: "org-a".into(),
        scopes: scopes.iter().map(|scope| (*scope).to_string()).collect(),
        issuer: "https://control.example/api/convex-auth".into(),
        audience: "data-plane".into(),
        subject: "service:search-operator".into(),
        expires_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_secs()
            + 300,
    }
}

fn intent() -> RebuildIntent {
    RebuildIntent {
        requested_org_id: None,
        global: false,
        break_glass: false,
        dry_run: true,
        clear: false,
        approval_id: None,
        idempotency_key: None,
        reason: None,
    }
}

#[test]
fn org_rebuild_defaults_to_verified_tenant() {
    let auth = authorize_rebuild(&claims(&["data:search:rebuild"]), &intent())
        .expect("authorized org preview");
    assert_eq!(
        auth,
        RebuildAuthorization::Org {
            org_id: "org-a".into(),
            dry_run: true,
            clear: false,
        }
    );
}

#[test]
fn spoofed_tenant_is_rejected() {
    let mut request = intent();
    request.requested_org_id = Some("org-b".into());
    assert!(authorize_rebuild(&claims(&["data:search:rebuild"]), &request).is_err());
}

#[test]
fn dedicated_admin_scope_is_required() {
    assert!(authorize_rebuild(&claims(&["org:data:read_all"]), &intent()).is_err());
}

#[test]
fn global_rebuild_requires_break_glass_scope_and_approval_fields() {
    let mut request = intent();
    request.global = true;
    request.break_glass = true;
    request.dry_run = false;
    request.approval_id = Some("approval-1".into());
    request.idempotency_key = Some("idem-1".into());
    request.reason = Some("approved recovery".into());

    assert!(authorize_rebuild(&claims(&["data:search:rebuild"]), &request).is_err());

    let authorization = authorize_rebuild(
        &claims(&["data:search:rebuild", "data:search:rebuild:global"]),
        &request,
    )
    .expect("global mutation enters durable approval workflow");
    assert!(matches!(authorization, RebuildAuthorization::Global { .. }));

    request.dry_run = true;
    let auth = authorize_rebuild(
        &claims(&["data:search:rebuild", "data:search:rebuild:global"]),
        &request,
    )
    .expect("global break-glass preview authorization");
    assert!(matches!(auth, RebuildAuthorization::Global { .. }));
}

#[test]
fn mutation_requires_approval_idempotency_and_reason() {
    let mut request = intent();
    request.dry_run = false;
    assert!(authorize_rebuild(&claims(&["data:search:rebuild"]), &request).is_err());
}

#[test]
fn approval_requires_dedicated_scope_and_claim_tenant() {
    assert!(authorize_approval(
        &claims(&["data:search:rebuild"]),
        Some("org-a"),
        false,
        false,
    )
    .is_err());
    assert!(authorize_approval(
        &claims(&["data:search:rebuild:approve"]),
        Some("org-b"),
        false,
        false,
    )
    .is_err());
    authorize_approval(
        &claims(&["data:search:rebuild:approve"]),
        Some("org-a"),
        false,
        false,
    )
    .expect("tenant approver");
}

#[test]
fn global_approval_requires_separate_scope_and_break_glass() {
    let scoped = claims(&[
        "data:search:rebuild:approve",
        "data:search:rebuild:approve:global",
    ]);
    assert!(authorize_approval(&scoped, None, true, false).is_err());
    authorize_approval(&scoped, None, true, true).expect("global break-glass approval");
}

#[test]
fn mutation_rejects_unbounded_admin_metadata() {
    let mut request = intent();
    request.dry_run = false;
    request.approval_id = Some("approval-123".into());
    request.idempotency_key = Some("idempotency-123".into());
    request.reason = Some("x".repeat(501));

    assert_eq!(
        authorize_rebuild(&claims(&["data:search:rebuild"]), &request),
        Err(auth::AuthError::BadRequest)
    );
}

#[test]
fn mutation_rejects_ambiguous_short_reason() {
    let mut request = intent();
    request.dry_run = false;
    request.approval_id = Some("approval-123".into());
    request.idempotency_key = Some("idempotency-123".into());
    request.reason = Some("x".into());
    assert_eq!(
        authorize_rebuild(&claims(&["data:search:rebuild"]), &request),
        Err(auth::AuthError::BadRequest)
    );
}

fn verifier_and_private_key() -> (AdminVerifier, RsaPrivateKey) {
    static PRIVATE_KEY: OnceLock<RsaPrivateKey> = OnceLock::new();
    let private = PRIVATE_KEY
        .get_or_init(|| RsaPrivateKey::new(&mut OsRng, 2048).expect("test RSA key"))
        .clone();
    let public_pem = private
        .to_public_key()
        .to_public_key_pem(LineEnding::LF)
        .expect("public PEM");
    let verifier = AdminVerifier::from_pem(
        public_pem.as_bytes(),
        "data-plane",
        "https://control.example/api/convex-auth",
    )
    .expect("verifier");
    (verifier, private)
}

fn encode_claims(private: &RsaPrivateKey, claims: &AdminClaims) -> String {
    let private_pem = private.to_pkcs8_pem(LineEnding::LF).expect("private PEM");
    encode(
        &Header::new(Algorithm::RS256),
        claims,
        &EncodingKey::from_rsa_pem(private_pem.as_bytes()).expect("encoding key"),
    )
    .expect("signed token")
}

#[test]
fn missing_or_unsigned_credentials_are_rejected() {
    let (verifier, _) = verifier_and_private_key();
    assert!(verifier.verify_authorization(None).is_err());
    assert!(verifier
        .verify_authorization(Some("Bearer eyJhbGciOiJub25lIiwidHlwIjoiSldUIn0.e30."))
        .is_err());
}

#[test]
fn signed_scoped_credential_is_verified() {
    let (verifier, private) = verifier_and_private_key();
    let token = encode_claims(&private, &claims(&["data:search:rebuild"]));

    let verified = verifier
        .verify_authorization(Some(&format!("Bearer {token}")))
        .expect("verified token");
    assert_eq!(verified.org_id, "org-a");
}

#[test]
fn signed_scoped_service_principal_is_verified_without_user_impersonation() {
    let (verifier, private) = verifier_and_private_key();
    let token = encode_claims(&private, &service_claims(&["data:search:rebuild"]));

    let verified = verifier
        .verify_authorization(Some(&format!("Bearer {token}")))
        .expect("verified service token");
    assert_eq!(
        verified.service_id.as_deref(),
        Some("service:search-operator")
    );
    assert!(verified.user_id.is_none());
}

#[test]
fn wrong_standard_claims_and_ambiguous_service_identity_fail_closed() {
    let (verifier, private) = verifier_and_private_key();

    let mut wrong_issuer = claims(&["data:search:rebuild"]);
    wrong_issuer.issuer = "https://untrusted.invalid".into();
    let token = encode_claims(&private, &wrong_issuer);
    assert!(verifier
        .verify_authorization(Some(&format!("Bearer {token}")))
        .is_err());

    let mut wrong_audience = claims(&["data:search:rebuild"]);
    wrong_audience.audience = "another-plane".into();
    let token = encode_claims(&private, &wrong_audience);
    assert!(verifier
        .verify_authorization(Some(&format!("Bearer {token}")))
        .is_err());

    let mut expired = claims(&["data:search:rebuild"]);
    expired.expires_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_secs()
        - 31;
    let token = encode_claims(&private, &expired);
    assert!(verifier
        .verify_authorization(Some(&format!("Bearer {token}")))
        .is_err());

    let mut ambiguous_service = service_claims(&["data:search:rebuild"]);
    ambiguous_service.service_id = Some("search-operator".into());
    let token = encode_claims(&private, &ambiguous_service);
    assert!(verifier
        .verify_authorization(Some(&format!("Bearer {token}")))
        .is_err());
}
