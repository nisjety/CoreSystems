//! Space W1 tests for the Control decision verifiers in `grpc.rs` (test-only child module).
//!
//! * WS02-S3: the Space event stream decision (`model-plane-space-stream`) and its separation
//!   from the `model-plane` read decision.
//! * WS06-S2: the `zero_data_retention` flag of the create and append decisions under
//!   `SESSION_CORE_SPACE_ZDR` (observe by default, enforce on the exact word).
//! * WS06-S1: the typed refusal of `record_run_output` for a ZDR caller.
//!
//! Everything here is pure: tokens are minted with a test key and verified with the keyed cores,
//! so no database, network or key environment is read.

use super::*;
use ed25519_dalek::{Signer as _, SigningKey};
use mp_contracts::zdr_refusal::code as zdr_code;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

const KEY_ID: &str = "control-w1-key";
const DECISION_REF: &str = "stream-decision-1";

fn now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-10-27T12:00:00Z")
        .expect("test time")
        .with_timezone(&Utc)
}

fn signing_key() -> SigningKey {
    SigningKey::from_bytes(&[41_u8; 32])
}

fn sign(key: &SigningKey, claims: &Value) -> String {
    let key_part = URL_SAFE_NO_PAD.encode(KEY_ID.as_bytes());
    let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims).expect("claims"));
    let signing_input = format!("{CONTROL_SPACE_DECISION_VERSION}.{key_part}.{payload}");
    format!(
        "{signing_input}.{}",
        URL_SAFE_NO_PAD.encode(key.sign(signing_input.as_bytes()).to_bytes())
    )
}

fn read_claims(audience: &str, permissions: &[&str], zdr: bool) -> Value {
    json!({
        "decision_ref": DECISION_REF,
        "org_id": "org-1",
        "space_ref": "space-room",
        "subject_id": "user-1",
        "service_audience": audience,
        "action_id": CONTROL_THREAD_READ_ACTION,
        "action_schema_hash": CONTROL_THREAD_READ_SCHEMA,
        "payload_digest": "",
        "idempotency_key": "space-read-1",
        "recipient_audience_ref": "audience:space-room:4",
        "recipient_audience_hash": "sha256:test-audience",
        "privacy_policy_ref": "privacy:org-1:5",
        "resource_authorization_ref": "control:space-room:thread-read:7",
        "purpose": "assistant_collaboration",
        "lawful_basis": "contract",
        "privacy_class": "internal",
        "third_party_processing_allowed": false,
        "retention_class": "standard",
        "residency": "swedencentral",
        "deletion_scope": "space",
        "zero_data_retention": zdr,
        "nonce": "nonce-1",
        "authority_revision": 7,
        "membership_revision": 4,
        "privacy_revision": 5,
        "recipient_audience_revision": 4,
        "entitlement_revision": 3,
        "permissions": permissions,
        "issued_at": now().to_rfc3339(),
        "expires_at": (now() + chrono::Duration::minutes(1)).to_rfc3339(),
    })
}

/// Mints a read-shaped decision for `audience`. `edit` runs before the digest is computed, so
/// it can change any bound field; `tamper_digest` replaces the digest after it was computed.
fn mint(
    audience: &str,
    permissions: &[&str],
    zdr: bool,
    edit: impl FnOnce(&mut Value),
    tamper_digest: bool,
) -> String {
    let mut claims = read_claims(audience, permissions, zdr);
    edit(&mut claims);
    let parsed: ControlSpaceDecisionClaims =
        serde_json::from_value(claims.clone()).expect("claims fixture");
    claims["payload_digest"] = if tamper_digest {
        json!(format!("sha256:{}", "0".repeat(64)))
    } else {
        json!(thread_read_payload_digest(&parsed))
    };
    sign(&signing_key(), &claims)
}

fn stream_token() -> String {
    mint(
        CONTROL_SPACE_STREAM_AUDIENCE,
        &["thread:read"],
        false,
        |_| {},
        false,
    )
}

fn verify_stream_at(
    token: &str,
    org: &str,
    user: &str,
    space: &str,
    at: DateTime<Utc>,
) -> Result<VerifiedReadClaims, Status> {
    verify_thread_read_claims_with_key(
        token,
        DECISION_REF,
        org,
        user,
        space,
        CONTROL_SPACE_STREAM_AUDIENCE,
        KEY_ID,
        &signing_key().verifying_key(),
        at,
    )
}

fn verify_stream(token: &str) -> Result<VerifiedReadClaims, Status> {
    verify_stream_at(token, "org-1", "user-1", "space-room", now())
}

// ---- WS02-S3: the stream decision -------------------------------------------------------

#[test]
fn stream_audience_is_its_own_word() {
    assert_eq!(CONTROL_SPACE_STREAM_AUDIENCE, "model-plane-space-stream");
    assert_ne!(CONTROL_SPACE_STREAM_AUDIENCE, CONTROL_SPACE_DECISION_AUDIENCE);
}

#[test]
fn stream_decision_verifies_and_carries_what_the_lease_needs() {
    let verified = verify_stream(&stream_token()).expect("a current stream decision verifies");
    assert_eq!(verified.recipient_audience_revision, 4);
    assert_eq!(verified.issued_at, now());
    assert_eq!(verified.expires_at, now() + chrono::Duration::minutes(1));
    assert!(!verified.zero_data_retention);
}

#[test]
fn stream_decision_carries_the_zdr_flag_for_the_poll_only_answer() {
    let token = mint(
        CONTROL_SPACE_STREAM_AUDIENCE,
        &["thread:read"],
        true,
        |_| {},
        false,
    );
    let verified = verify_stream(&token).expect("a ZDR decision still verifies");
    assert!(
        verified.zero_data_retention,
        "the stream handler answers poll_only on it"
    );
}

#[test]
fn stream_refuses_a_read_decision_of_the_model_plane_audience() {
    let read_token = mint(
        CONTROL_SPACE_DECISION_AUDIENCE,
        &["thread:read"],
        false,
        |_| {},
        false,
    );
    assert_eq!(
        verify_stream(&read_token)
            .expect_err("a read token never opens a stream")
            .code(),
        tonic::Code::PermissionDenied
    );
}

#[test]
fn list_threads_refuses_a_stream_decision() {
    let status = verify_thread_read_space_decision_with_key(
        &stream_token(),
        DECISION_REF,
        "org-1",
        "user-1",
        "space-room",
        KEY_ID,
        &signing_key().verifying_key(),
        now(),
    )
    .expect_err("a stream token never reads a transcript");
    assert_eq!(status.code(), tonic::Code::PermissionDenied);
}

#[test]
fn stream_decision_refuses_expired_and_not_yet_valid_windows() {
    let token = stream_token();
    // Exactly at expiry and long after both refuse.
    for at in [
        now() + chrono::Duration::minutes(1),
        now() + chrono::Duration::minutes(30),
    ] {
        assert_eq!(
            verify_stream_at(&token, "org-1", "user-1", "space-room", at)
                .expect_err("an expired decision is refused")
                .code(),
            tonic::Code::PermissionDenied
        );
    }
    // Issued more than a minute in the future (clock skew bound).
    let future = mint(
        CONTROL_SPACE_STREAM_AUDIENCE,
        &["thread:read"],
        false,
        |claims| claims["issued_at"] = json!((now() + chrono::Duration::minutes(5)).to_rfc3339()),
        false,
    );
    assert_eq!(
        verify_stream(&future)
            .expect_err("a future-issued decision is refused")
            .code(),
        tonic::Code::PermissionDenied
    );
}

#[test]
fn stream_decision_refuses_another_subject_space_and_org() {
    let token = stream_token();
    for (org, user, space, why) in [
        ("org-2", "user-1", "space-room", "another tenant"),
        ("org-1", "user-2", "space-room", "another subject"),
        ("org-1", "user-1", "space-other", "another Space"),
    ] {
        assert_eq!(
            verify_stream_at(&token, org, user, space, now())
                .expect_err(why)
                .code(),
            tonic::Code::PermissionDenied,
            "a stream decision must not open {why}"
        );
    }
    // A different decision ref is also not the one the caller presented.
    let status = verify_thread_read_claims_with_key(
        &token,
        "another-decision",
        "org-1",
        "user-1",
        "space-room",
        CONTROL_SPACE_STREAM_AUDIENCE,
        KEY_ID,
        &signing_key().verifying_key(),
        now(),
    )
    .expect_err("the decision ref is bound");
    assert_eq!(status.code(), tonic::Code::PermissionDenied);
}

#[test]
fn stream_decision_refuses_write_permissions_and_requires_read() {
    for write in ["thread:append", "thread:create"] {
        let token = mint(
            CONTROL_SPACE_STREAM_AUDIENCE,
            &["thread:read", write],
            false,
            |_| {},
            false,
        );
        assert_eq!(
            verify_stream(&token)
                .expect_err("a stream decision carries no write")
                .code(),
            tonic::Code::PermissionDenied,
            "{write}"
        );
    }
    let no_read = mint(CONTROL_SPACE_STREAM_AUDIENCE, &[], false, |_| {}, false);
    assert_eq!(
        verify_stream(&no_read)
            .expect_err("thread:read is required")
            .code(),
        tonic::Code::PermissionDenied
    );
}

#[test]
fn stream_decision_refuses_a_tampered_payload_digest() {
    let token = mint(
        CONTROL_SPACE_STREAM_AUDIENCE,
        &["thread:read"],
        false,
        |_| {},
        true,
    );
    assert_eq!(
        verify_stream(&token)
            .expect_err("the digest contract is pinned")
            .code(),
        tonic::Code::PermissionDenied
    );
}

#[test]
fn stream_decision_refuses_a_foreign_signer() {
    let foreign = SigningKey::from_bytes(&[42_u8; 32]);
    let mut claims = read_claims(CONTROL_SPACE_STREAM_AUDIENCE, &["thread:read"], false);
    let parsed: ControlSpaceDecisionClaims =
        serde_json::from_value(claims.clone()).expect("claims");
    claims["payload_digest"] = json!(thread_read_payload_digest(&parsed));
    let forged = sign(&foreign, &claims);
    assert_eq!(
        verify_stream(&forged)
            .expect_err("a foreign signer is refused")
            .code(),
        tonic::Code::PermissionDenied
    );
}

#[test]
fn stream_wrapper_requires_space_ref_token_and_a_user_before_any_key_lookup() {
    // The wrapper answers these before it reads the key environment.
    for (token, decision_ref, space) in [
        ("", DECISION_REF, "space-room"),
        ("v2.k.p.s", "", "space-room"),
        ("v2.k.p.s", DECISION_REF, "  "),
    ] {
        let status =
            verify_space_stream_decision(token, decision_ref, "org-1", "user-1", space, now())
                .expect_err("half a decision is a caller error");
        assert_eq!(status.code(), tonic::Code::InvalidArgument);
    }
    let status =
        verify_space_stream_decision("v2.k.p.s", DECISION_REF, "org-1", "  ", "space-room", now())
            .expect_err("a stream needs a user subject");
    assert_eq!(status.code(), tonic::Code::PermissionDenied);
}

// ---- WS06-S2: the zero_data_retention flag of create and append ---------------------------

/// Serialises the tests that set `SESSION_CORE_SPACE_ZDR`; clears the variable on drop.
static ZDR_ENV: Mutex<()> = Mutex::new(());

struct ZdrEnv {
    _guard: std::sync::MutexGuard<'static, ()>,
}

impl ZdrEnv {
    fn set(value: Option<&str>) -> Self {
        let guard = ZDR_ENV
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match value {
            Some(value) => env::set_var("SESSION_CORE_SPACE_ZDR", value),
            None => env::remove_var("SESSION_CORE_SPACE_ZDR"),
        }
        Self { _guard: guard }
    }
}

impl Drop for ZdrEnv {
    fn drop(&mut self) {
        env::remove_var("SESSION_CORE_SPACE_ZDR");
    }
}

fn create_request(zdr: bool) -> pb::CreateThreadRequest {
    let mut request = pb::CreateThreadRequest {
        org_id: "org-1".to_owned(),
        user_id: "user-1".to_owned(),
        session_key: "session-1".to_owned(),
        space_id: "space-personal".to_owned(),
        space_decision_ref: "decision-1".to_owned(),
        recipient_audience_ref: "audience:space-personal:2".to_owned(),
        recipient_audience_revision: 2,
        recipient_audience_hash: "sha256:test-audience".to_owned(),
        privacy_policy_ref: "privacy:org-1:5".to_owned(),
        resource_authorization_ref: "control:space-personal:thread-create:7".to_owned(),
        authority_revision: 7,
        action_schema_hash: "sha256:thread-create-v1".to_owned(),
        idempotency_key: "thread-create:1".to_owned(),
        ..Default::default()
    };
    request.payload_digest = thread_create_payload_digest(&request);
    let claims = json!({
        "decision_ref": request.space_decision_ref, "org_id": request.org_id,
        "space_ref": request.space_id, "subject_id": request.user_id,
        "service_audience": CONTROL_SPACE_DECISION_AUDIENCE, "action_id": CONTROL_THREAD_CREATE_ACTION,
        "action_schema_hash": request.action_schema_hash, "payload_digest": request.payload_digest,
        "idempotency_key": request.idempotency_key,
        "recipient_audience_ref": request.recipient_audience_ref,
        "recipient_audience_hash": request.recipient_audience_hash,
        "privacy_policy_ref": request.privacy_policy_ref,
        "resource_authorization_ref": request.resource_authorization_ref,
        "purpose": "assistant_collaboration", "lawful_basis": "contract", "privacy_class": "internal",
        "third_party_processing_allowed": false, "retention_class": "standard",
        "residency": "swedencentral", "deletion_scope": "space", "zero_data_retention": zdr,
        "nonce": "nonce-1", "authority_revision": request.authority_revision,
        "membership_revision": 4, "privacy_revision": 5,
        "recipient_audience_revision": request.recipient_audience_revision,
        "entitlement_revision": 3, "permissions": ["thread:create"],
        "issued_at": now().to_rfc3339(),
        "expires_at": (now() + chrono::Duration::minutes(1)).to_rfc3339(),
    });
    request.space_decision_token = sign(&signing_key(), &claims);
    request
}

fn append_request(zdr: bool) -> pb::AppendMessageRequest {
    let mut request = pb::AppendMessageRequest {
        thread_id: "thread-1".to_owned(),
        role: "user".to_owned(),
        content: "exact message".to_owned(),
        space_id: "space-personal".to_owned(),
        space_decision_ref: "append-decision-1".to_owned(),
        recipient_audience_ref: "audience:space-personal:2".to_owned(),
        recipient_audience_revision: 2,
        recipient_audience_hash: "sha256:test-audience".to_owned(),
        privacy_policy_ref: "privacy:org-1:5".to_owned(),
        authority_revision: 7,
        resource_authorization_ref: "control:space-personal:thread-append:7".to_owned(),
        action_schema_hash: "sha256:thread-append-v1".to_owned(),
        idempotency_key: "thread-append:1".to_owned(),
        ..Default::default()
    };
    request.payload_digest = thread_append_payload_digest(&request, "org-1", "user-1");
    let claims = json!({
        "decision_ref": request.space_decision_ref, "org_id": "org-1", "space_ref": request.space_id,
        "subject_id": "user-1", "service_audience": CONTROL_SPACE_DECISION_AUDIENCE,
        "action_id": CONTROL_THREAD_APPEND_ACTION, "action_schema_hash": request.action_schema_hash,
        "payload_digest": request.payload_digest, "idempotency_key": request.idempotency_key,
        "recipient_audience_ref": request.recipient_audience_ref,
        "recipient_audience_hash": request.recipient_audience_hash,
        "privacy_policy_ref": request.privacy_policy_ref,
        "resource_authorization_ref": request.resource_authorization_ref,
        "purpose": "assistant_collaboration", "lawful_basis": "contract", "privacy_class": "internal",
        "third_party_processing_allowed": false, "retention_class": "standard",
        "residency": "swedencentral", "deletion_scope": "space", "zero_data_retention": zdr,
        "nonce": "nonce-1", "authority_revision": request.authority_revision,
        "membership_revision": 4, "privacy_revision": 5,
        "recipient_audience_revision": request.recipient_audience_revision,
        "entitlement_revision": 3, "permissions": ["thread:append"],
        "issued_at": now().to_rfc3339(),
        "expires_at": (now() + chrono::Duration::minutes(1)).to_rfc3339(),
    });
    request.space_decision_token = sign(&signing_key(), &claims);
    request
}

fn verify_create(request: &pb::CreateThreadRequest) -> Result<String, Status> {
    verify_thread_space_decision_with_key(request, KEY_ID, &signing_key().verifying_key(), now())
}

fn verify_append(request: &pb::AppendMessageRequest) -> Result<String, Status> {
    verify_append_space_decision_with_key(
        request,
        "org-1",
        "user-1",
        KEY_ID,
        &signing_key().verifying_key(),
        now(),
    )
}

/// Counter recorder for `metrics::with_local_recorder`: key rendered as `name{label=value,...}`.
#[derive(Default, Clone)]
struct CounterRecorder {
    counters: Arc<Mutex<HashMap<String, u64>>>,
}

struct RecordedCounter {
    key: String,
    counters: Arc<Mutex<HashMap<String, u64>>>,
}

impl metrics::CounterFn for RecordedCounter {
    fn increment(&self, value: u64) {
        *self
            .counters
            .lock()
            .unwrap()
            .entry(self.key.clone())
            .or_default() += value;
    }
    fn absolute(&self, value: u64) {
        self.counters
            .lock()
            .unwrap()
            .insert(self.key.clone(), value);
    }
}

impl metrics::Recorder for CounterRecorder {
    fn describe_counter(
        &self,
        _: metrics::KeyName,
        _: Option<metrics::Unit>,
        _: metrics::SharedString,
    ) {
    }
    fn describe_gauge(
        &self,
        _: metrics::KeyName,
        _: Option<metrics::Unit>,
        _: metrics::SharedString,
    ) {
    }
    fn describe_histogram(
        &self,
        _: metrics::KeyName,
        _: Option<metrics::Unit>,
        _: metrics::SharedString,
    ) {
    }
    fn register_counter(&self, key: &metrics::Key, _: &metrics::Metadata<'_>) -> metrics::Counter {
        let mut labels: Vec<String> = key
            .labels()
            .map(|label| format!("{}={}", label.key(), label.value()))
            .collect();
        labels.sort();
        metrics::Counter::from_arc(Arc::new(RecordedCounter {
            key: format!("{}{{{}}}", key.name(), labels.join(",")),
            counters: Arc::clone(&self.counters),
        }))
    }
    fn register_gauge(&self, _: &metrics::Key, _: &metrics::Metadata<'_>) -> metrics::Gauge {
        metrics::Gauge::noop()
    }
    fn register_histogram(
        &self,
        _: &metrics::Key,
        _: &metrics::Metadata<'_>,
    ) -> metrics::Histogram {
        metrics::Histogram::noop()
    }
}

fn assert_zdr_forbidden(status: &Status) {
    assert_eq!(status.code(), tonic::Code::FailedPrecondition);
    assert_eq!(
        Refusal::from_status(status).map(|refusal| refusal.code()),
        Some(zdr_code::DURABLE_MUTATION_FORBIDDEN)
    );
}

#[test]
fn space_zdr_observe_counts_and_proceeds() {
    let recorder = CounterRecorder::default();
    metrics::with_local_recorder(&recorder, || {
        assert!(
            space_decision_retention_gate(true, "thread_create", SpaceZdrMode::Observe).is_ok()
        );
        assert!(
            space_decision_retention_gate(true, "message_append", SpaceZdrMode::Observe).is_ok()
        );
        // Flag false is a no-op: nothing is counted.
        assert!(
            space_decision_retention_gate(false, "thread_create", SpaceZdrMode::Observe).is_ok()
        );
        assert!(
            space_decision_retention_gate(false, "thread_create", SpaceZdrMode::Enforce).is_ok()
        );
    });
    let counters = recorder.counters.lock().unwrap();
    assert_eq!(
        counters.get("mp_session_space_zdr_flag_total{action=thread_create,enforced=false}"),
        Some(&1)
    );
    assert_eq!(
        counters.get("mp_session_space_zdr_flag_total{action=message_append,enforced=false}"),
        Some(&1)
    );
    assert_eq!(
        counters.len(),
        2,
        "the false flag counts nothing: {counters:?}"
    );
}

#[test]
fn space_zdr_observe_is_the_default_for_the_real_verifiers() {
    {
        let _env = ZdrEnv::set(None);
        // The default (unset) is observe: the write proceeds.
        verify_create(&create_request(true)).expect("flag true proceeds under observe");
        verify_append(&append_request(true)).expect("flag true proceeds under observe");
    }
    let _env = ZdrEnv::set(Some("Enforce"));
    verify_create(&create_request(true)).expect("a mis-cased word never enforces");
}

#[test]
fn space_zdr_enforce_refuses_create_and_append_with_flag_true() {
    let _env = ZdrEnv::set(Some("enforce"));
    assert_zdr_forbidden(&verify_create(&create_request(true)).expect_err("enforce refuses create"));
    assert_zdr_forbidden(
        &verify_append(&append_request(true)).expect_err("enforce refuses append"),
    );
    // Flag false stays accepted under enforce.
    verify_create(&create_request(false)).expect("flag false still creates");
    verify_append(&append_request(false)).expect("flag false still appends");
}

#[test]
fn space_zdr_gate_enforce_is_the_typed_refusal_and_counts_enforced_true() {
    let recorder = CounterRecorder::default();
    let status = metrics::with_local_recorder(&recorder, || {
        space_decision_retention_gate(true, "message_append", SpaceZdrMode::Enforce)
            .expect_err("enforce refuses")
    });
    assert_zdr_forbidden(&status);
    assert_eq!(
        recorder
            .counters
            .lock()
            .unwrap()
            .get("mp_session_space_zdr_flag_total{action=message_append,enforced=true}"),
        Some(&1)
    );
}

#[test]
fn space_zdr_flag_does_not_widen_the_binding_checks() {
    // The flag is a rollout switch, not an authorization input: a ZDR decision for another
    // effect is still a permission denial before the gate runs.
    let _env = ZdrEnv::set(None);
    let mut request = create_request(true);
    request.session_key = "another-session".to_owned();
    assert_eq!(
        verify_create(&request).expect_err("another effect").code(),
        tonic::Code::PermissionDenied
    );
}

#[test]
fn space_zdr_read_and_stream_verifiers_ignore_the_mode() {
    let _env = ZdrEnv::set(Some("enforce"));
    // The read decision carries the flag back to the reader; it is not refused here.
    let token = mint(
        CONTROL_SPACE_DECISION_AUDIENCE,
        &["thread:read"],
        true,
        |_| {},
        false,
    );
    let verified = verify_thread_read_space_decision_with_key(
        &token,
        DECISION_REF,
        "org-1",
        "user-1",
        "space-room",
        KEY_ID,
        &signing_key().verifying_key(),
        now(),
    )
    .expect("the read verifier is unchanged");
    assert_eq!(verified.recipient_audience_revision, 4);
    let stream = mint(
        CONTROL_SPACE_STREAM_AUDIENCE,
        &["thread:read"],
        true,
        |_| {},
        false,
    );
    assert!(
        verify_stream(&stream)
            .expect("the stream verifier is unchanged")
            .zero_data_retention
    );
}

#[test]
fn space_zdr_scheduled_run_verifier_still_refuses_the_flag_in_every_mode() {
    let key_id = "control-scheduled-run-preparation";
    let mut request = pb::PrepareScheduledRunThreadRequest {
        org_id: "org-1".to_owned(),
        human_subject_id: "user-1".to_owned(),
        space_id: "space-1".to_owned(),
        schedule_id: "schedule-1".to_owned(),
        fire_key: "fire_20260815t120000z".to_owned(),
        run_id: "task_scheduled_1".to_owned(),
        system_thread_key: "schedule/schedule-1/fire_20260815t120000z".to_owned(),
        template_digest: format!("sha256:{}", "a".repeat(64)),
        idempotency_key: "schedule-1:fire_20260815t120000z".to_owned(),
        space_decision_ref: "decision-1".to_owned(),
        recipient_audience_ref: "audience-1".to_owned(),
        recipient_audience_revision: 2,
        recipient_audience_hash: "sha256:audience".to_owned(),
        privacy_policy_ref: "privacy-1".to_owned(),
        resource_authorization_ref: "resource-1".to_owned(),
        authority_revision: 7,
        action_schema_hash: "sha256:space-scheduled-run-v1".to_owned(),
        ..Default::default()
    };
    let keys = BTreeMap::from([(key_id.to_owned(), signing_key().verifying_key())]);
    let mut run_with_flag = |zdr: bool| {
        let mut claims = json!({
            "decision_ref": "decision-1", "org_id": "org-1", "space_ref": "space-1",
            "subject_id": "user-1", "service_audience": "model-plane-capability-core",
            "action_id": "model.schedule.run", "action_schema_hash": "sha256:space-scheduled-run-v1",
            "payload_digest": "", "idempotency_key": request.idempotency_key.as_str(),
            "recipient_audience_ref": "audience-1", "recipient_audience_revision": 2,
            "recipient_audience_hash": "sha256:audience", "privacy_policy_ref": "privacy-1",
            "resource_authorization_ref": "resource-1", "purpose": "agent_work",
            "lawful_basis": "contract", "privacy_class": "internal",
            "third_party_processing_allowed": false, "retention_class": "standard",
            "residency": "swedencentral", "deletion_scope": "space", "zero_data_retention": zdr,
            "nonce": "nonce-1", "authority_revision": 7, "membership_revision": 4,
            "privacy_revision": 5, "entitlement_revision": 3, "permissions": ["schedule:run"],
            "issued_at": (now() - chrono::Duration::seconds(1)).to_rfc3339(),
            "expires_at": (now() + chrono::Duration::minutes(1)).to_rfc3339(),
        });
        let decoded: ControlSpaceDecisionClaims =
            serde_json::from_value(claims.clone()).expect("claims");
        request.payload_digest = scheduled_run_payload_digest(&request, &decoded);
        claims["payload_digest"] = json!(request.payload_digest.as_str());
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).expect("claims"));
        let encoded_key = URL_SAFE_NO_PAD.encode(key_id.as_bytes());
        let signing_input = format!("{CONTROL_SPACE_DECISION_VERSION}.{encoded_key}.{payload}");
        request.control_decision_token = format!(
            "{signing_input}.{}",
            URL_SAFE_NO_PAD.encode(signing_key().sign(signing_input.as_bytes()).to_bytes())
        );
        verify_scheduled_run_decision_with_keys(&request, &keys, now())
    };
    for mode in [None, Some("enforce")] {
        let _env = ZdrEnv::set(mode);
        run_with_flag(false).expect("flag false authorizes the scheduled run");
        assert_eq!(
            run_with_flag(true)
                .expect_err("a ZDR scheduled run is never authorized")
                .code(),
            tonic::Code::PermissionDenied
        );
    }
}

// ---- WS06-S1: record_run_output refuses a ZDR caller with the typed kind ---------------------

#[tokio::test]
async fn zdr_refusal_record_run_output_is_durable_mutation_forbidden() {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgresql://unreachable.invalid/session")
        .expect("lazy pool");
    let service = ManagedRunLifecycleService {
        pool,
        letta_memory: None,
    };
    let mut request = Request::new(pb::RecordRunOutputRequest {
        run_id: "run-1".to_owned(),
        output: "answer".to_owned(),
    });
    request
        .extensions_mut()
        .insert(crate::auth::VerifiedIdentity::user_for_test_with_zdr(
            "org-1", "user-1", true,
        ));
    let status = service
        .record_run_output(request)
        .await
        .expect_err("a ZDR run has no durable output");
    assert_eq!(status.code(), tonic::Code::PermissionDenied);
    assert_eq!(
        Refusal::from_status(&status).map(|refusal| refusal.code()),
        Some(zdr_code::DURABLE_MUTATION_FORBIDDEN)
    );
}
