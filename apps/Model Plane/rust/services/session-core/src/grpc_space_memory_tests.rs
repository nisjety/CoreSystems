//! Space memory containment tests that need the private verifiers and write paths of `grpc.rs`
//! (test-only child module; WS17-S1, WS17-S2).
//!
//! * Pure tier: `verify_thread_space_decision_with_key` and `verify_append_space_decision_with_key`
//!   return the signed `space_kind` claim (empty when the decision carries none).
//! * DB tier (every test `#[ignore]`d, needs `DATABASE_URL` to a disposable `*test*` Postgres):
//!   the containment rules C1-C7 end to end through the append, create, dream loop, search, index
//!   and list paths, with the kill switch on and off.

use super::*;
use crate::memory_control::MemoryAccess;
use crate::space::memory_scope;
use crate::space::test_support;
use ed25519_dalek::Signer as _;
use mp_contracts::model_plane::v1::{
    memory_service_server::MemoryService, IndexMemoryRequest, ListMemoryRequest, SearchMemoryRequest,
};
use serde_json::{json, Value};
use std::sync::{Mutex, MutexGuard};
use wiremock::{matchers::any, Mock, MockServer, ResponseTemplate};

use super::space_w1_tests::{signing_key, StreamKeysEnv, KEY_ID};

// ---- token minting (live clock: the handler paths verify at Utc::now()) ------------------------

fn sign_claims(claims: &Value) -> String {
    let key_part = URL_SAFE_NO_PAD.encode(KEY_ID.as_bytes());
    let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims).expect("claims"));
    let signing_input = format!("{CONTROL_SPACE_DECISION_VERSION}.{key_part}.{payload}");
    format!(
        "{signing_input}.{}",
        URL_SAFE_NO_PAD.encode(signing_key().sign(signing_input.as_bytes()).to_bytes())
    )
}

/// The claims every thread decision shares. `kind` of `None` leaves `space_kind` out entirely,
/// which is what Control sent before the claim existed.
#[allow(clippy::too_many_arguments)]
fn base_claims(
    org: &str,
    user: &str,
    space: &str,
    action: &str,
    permission: &str,
    schema: &str,
    digest: &str,
    idem: &str,
    audience_ref: &str,
    audience_revision: u64,
    resource_ref: &str,
    decision_ref: &str,
    kind: Option<&str>,
) -> Value {
    let now = Utc::now();
    let mut claims = json!({
        "decision_ref": decision_ref, "org_id": org, "space_ref": space, "subject_id": user,
        "service_audience": CONTROL_SPACE_DECISION_AUDIENCE, "action_id": action,
        "action_schema_hash": schema, "payload_digest": digest, "idempotency_key": idem,
        "recipient_audience_ref": audience_ref, "recipient_audience_hash": "sha256:test-audience",
        "privacy_policy_ref": "privacy:org-1:5", "resource_authorization_ref": resource_ref,
        "purpose": "assistant_collaboration", "lawful_basis": "contract", "privacy_class": "internal",
        "third_party_processing_allowed": false, "retention_class": "standard",
        "residency": "swedencentral", "deletion_scope": "space", "zero_data_retention": false,
        "nonce": format!("nonce-{decision_ref}"), "authority_revision": 7, "membership_revision": 4,
        "privacy_revision": 5, "recipient_audience_revision": audience_revision,
        "entitlement_revision": 3, "permissions": [permission],
        "issued_at": now.to_rfc3339(),
        "expires_at": (now + chrono::Duration::minutes(2)).to_rfc3339(),
    });
    if let Some(kind) = kind {
        claims["space_kind"] = json!(kind);
    }
    claims
}

fn signed_create(org: &str, user: &str, space: &str, session_key: &str, kind: Option<&str>) -> pb::CreateThreadRequest {
    let mut request = pb::CreateThreadRequest {
        org_id: org.to_owned(),
        user_id: user.to_owned(),
        session_key: session_key.to_owned(),
        space_id: space.to_owned(),
        space_decision_ref: format!("create-{session_key}"),
        recipient_audience_ref: format!("audience:{space}:2"),
        recipient_audience_revision: 2,
        recipient_audience_hash: "sha256:test-audience".to_owned(),
        privacy_policy_ref: "privacy:org-1:5".to_owned(),
        resource_authorization_ref: format!("control:{space}:thread-create:7"),
        authority_revision: 7,
        action_schema_hash: "sha256:thread-create-v1".to_owned(),
        idempotency_key: format!("thread-create:{session_key}"),
        ..Default::default()
    };
    request.payload_digest = thread_create_payload_digest(&request);
    let claims = base_claims(
        org,
        user,
        space,
        CONTROL_THREAD_CREATE_ACTION,
        "thread:create",
        &request.action_schema_hash,
        &request.payload_digest,
        &request.idempotency_key,
        &request.recipient_audience_ref,
        request.recipient_audience_revision,
        &request.resource_authorization_ref,
        &request.space_decision_ref,
        kind,
    );
    request.space_decision_token = sign_claims(&claims);
    request
}

#[allow(clippy::too_many_arguments)]
fn signed_append(
    org: &str,
    user: &str,
    thread: &str,
    space: &str,
    content: &str,
    nonce: &str,
    kind: Option<&str>,
) -> pb::AppendMessageRequest {
    let mut request = pb::AppendMessageRequest {
        thread_id: thread.to_owned(),
        role: "user".to_owned(),
        content: content.to_owned(),
        space_id: space.to_owned(),
        space_decision_ref: format!("append-{nonce}"),
        recipient_audience_ref: format!("audience:{space}:2"),
        recipient_audience_revision: 2,
        recipient_audience_hash: "sha256:test-audience".to_owned(),
        privacy_policy_ref: "privacy:org-1:5".to_owned(),
        authority_revision: 7,
        resource_authorization_ref: format!("control:{space}:thread-append:7"),
        action_schema_hash: CONTROL_THREAD_APPEND_SCHEMA.to_owned(),
        idempotency_key: format!("thread-append:{nonce}"),
        ..Default::default()
    };
    request.payload_digest = thread_append_payload_digest(&request, org, user);
    let claims = base_claims(
        org,
        user,
        space,
        CONTROL_THREAD_APPEND_ACTION,
        "thread:append",
        &request.action_schema_hash,
        &request.payload_digest,
        &request.idempotency_key,
        &request.recipient_audience_ref,
        request.recipient_audience_revision,
        &request.resource_authorization_ref,
        &request.space_decision_ref,
        kind,
    );
    request.space_decision_token = sign_claims(&claims);
    request
}

fn verify_create_now(request: &pb::CreateThreadRequest) -> Result<String, Status> {
    verify_thread_space_decision_with_key(request, KEY_ID, &signing_key().verifying_key(), Utc::now())
}

fn verify_append_now(request: &pb::AppendMessageRequest, org: &str, user: &str) -> Result<String, Status> {
    verify_append_space_decision_with_key(request, org, user, KEY_ID, &signing_key().verifying_key(), Utc::now())
}

// ---- pure tier ---------------------------------------------------------------------------------

#[test]
fn create_decision_verifier_returns_the_signed_space_kind() {
    for (claimed, expected) in [
        (Some("room"), "room"),
        (Some("personal"), "personal"),
        (Some("dm"), "dm"),
        (None, ""),
    ] {
        let request = signed_create("org-1", "user-1", "space-1", "s-kind", claimed);
        assert_eq!(
            verify_create_now(&request).expect("a valid create decision verifies"),
            expected,
            "claimed {claimed:?}"
        );
    }
}

#[test]
fn append_decision_verifier_returns_the_signed_space_kind() {
    for (claimed, expected) in [
        (Some("room"), "room"),
        (Some("personal"), "personal"),
        (Some("project"), "project"),
        (None, ""),
    ] {
        let request = signed_append("org-1", "user-1", "thread-1", "space-1", "hello", "n1", claimed);
        assert_eq!(
            verify_append_now(&request, "org-1", "user-1").expect("a valid append decision verifies"),
            expected,
            "claimed {claimed:?}"
        );
    }
}

#[test]
fn the_kind_claim_is_not_bound_by_the_payload_digest() {
    // Two decisions over the same effect that differ only in `space_kind` carry one digest and
    // both verify: the claim is additive, so Control and Model can roll out in either order.
    let room = signed_create("org-1", "user-1", "space-1", "s-digest", Some("room"));
    let personal = signed_create("org-1", "user-1", "space-1", "s-digest", Some("personal"));
    assert_eq!(room.payload_digest, personal.payload_digest);
    assert_eq!(verify_create_now(&room).expect("room verifies"), "room");
    assert_eq!(verify_create_now(&personal).expect("personal verifies"), "personal");
}

#[test]
fn a_kind_the_column_would_refuse_is_dropped_by_the_normaliser() {
    let request = signed_create("org-1", "user-1", "space-1", "s-long", Some(&"x".repeat(40)));
    let signed = verify_create_now(&request).expect("the verifier does not judge the kind");
    assert_eq!(signed.len(), 40);
    assert_eq!(memory_scope::normalize_claim_space_kind(&signed), None);
}

// ---- database tier -----------------------------------------------------------------------------

/// Serialises the tests that depend on the `MEMORY_SPACE_CONTAINMENT` value; removes it on drop,
/// so every test starts from the default (containment on).
static CONTAINMENT_ENV: Mutex<()> = Mutex::new(());

struct ContainmentFlag {
    _guard: MutexGuard<'static, ()>,
}

impl ContainmentFlag {
    fn set(value: Option<&str>) -> Self {
        let guard = CONTAINMENT_ENV
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match value {
            Some(value) => env::set_var(mp_contracts::space_memory::MEMORY_SPACE_CONTAINMENT_ENV, value),
            None => env::remove_var(mp_contracts::space_memory::MEMORY_SPACE_CONTAINMENT_ENV),
        }
        Self { _guard: guard }
    }
}

impl Drop for ContainmentFlag {
    fn drop(&mut self) {
        env::remove_var(mp_contracts::space_memory::MEMORY_SPACE_CONTAINMENT_ENV);
    }
}

/// One test's world: unique org and Space ids, cleaned up at the end.
struct World {
    pool: PgPool,
    org: String,
    space: String,
}

impl World {
    async fn new(prefix: &str) -> Option<Self> {
        let pool = test_support::connect().await?;
        let suffix = new_ulid();
        Some(Self {
            pool,
            org: format!("org-{prefix}-{suffix}"),
            space: format!("space-{prefix}-{suffix}"),
        })
    }

    /// A Space thread owned by `owner`, with `space_kind` as given (`None` = NULL, a legacy thread).
    async fn space_thread(&self, name: &str, owner: &str, kind: Option<&str>) -> String {
        let id = format!("thread-{name}-{}", new_ulid());
        test_support::seed_space_thread(&self.pool, &self.org, &self.space, &id, owner).await;
        if let Some(kind) = kind {
            sqlx::query("UPDATE threads SET space_kind = $2 WHERE id = $1")
                .bind(&id)
                .bind(kind)
                .execute(&self.pool)
                .await
                .expect("stamp kind");
        }
        id
    }

    /// An ordinary chat thread (no Space).
    async fn plain_thread(&self, owner: &str) -> String {
        let id = format!("thread-plain-{}", new_ulid());
        sqlx::query("INSERT INTO threads (id, session_key, org_id, user_id) VALUES ($1, $1, $2, $3)")
            .bind(&id)
            .bind(&self.org)
            .bind(owner)
            .execute(&self.pool)
            .await
            .expect("plain thread");
        id
    }

    async fn memory(&self, session: Option<&str>, scope: &str, owner: &str, content: &str, links: &[&str]) -> String {
        let id = new_ulid();
        sqlx::query(
            "INSERT INTO agent_memory (id, org_id, session_id, scope, key, content, owner, source_links, review_state) \
             VALUES ($1, $2, $3, $4, $1, $5, $6, $7, 'accepted')",
        )
        .bind(&id)
        .bind(&self.org)
        .bind(session)
        .bind(scope)
        .bind(content)
        .bind(owner)
        .bind(links.iter().map(|link| (*link).to_owned()).collect::<Vec<_>>())
        .execute(&self.pool)
        .await
        .expect("seed memory");
        id
    }

    async fn user_memory_count(&self, owner: &str) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM agent_memory WHERE org_id = $1 AND scope = 'user' AND owner = $2")
            .bind(&self.org)
            .bind(owner)
            .fetch_one(&self.pool)
            .await
            .expect("count user memory")
    }

    async fn memory_count(&self) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM agent_memory WHERE org_id = $1")
            .bind(&self.org)
            .fetch_one(&self.pool)
            .await
            .expect("count memory")
    }

    async fn dream_runs(&self, thread: &str, trigger: &str) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM dream_runs WHERE thread_id = $1 AND trigger = $2")
            .bind(thread)
            .bind(trigger)
            .fetch_one(&self.pool)
            .await
            .expect("count dream runs")
    }

    async fn kind_of(&self, thread: &str) -> Option<String> {
        sqlx::query_scalar("SELECT space_kind FROM threads WHERE id = $1")
            .bind(thread)
            .fetch_one(&self.pool)
            .await
            .expect("thread kind")
    }

    async fn owner_turn(&self, thread: &str, owner: &str, content: &str) -> Result<(), Status> {
        let request = pb::AppendMessageRequest {
            thread_id: thread.to_owned(),
            role: "user".to_owned(),
            content: content.to_owned(),
            ..Default::default()
        };
        append_message_inner(&self.pool, None, MemoryRetention::Durable, request, owner)
            .await
            .map(|_| ())
    }

    async fn finish(self) {
        test_support::clean_space(&self.pool, &self.org, &self.space).await;
        for sql in [
            "DELETE FROM agent_memory WHERE org_id = $1",
            "DELETE FROM dream_runs WHERE org_id = $1",
            "DELETE FROM messages WHERE thread_id IN (SELECT id FROM threads WHERE org_id = $1)",
            "DELETE FROM threads WHERE org_id = $1",
        ] {
            sqlx::query(sql).bind(&self.org).execute(&self.pool).await.ok();
        }
    }
}

const REMEMBER: &str = "Husk at jeg heter Kari.";

#[tokio::test]
#[ignore = "needs DB harness"]
async fn contained_thread_append_extracts_nothing() {
    let _flag = ContainmentFlag::set(None);
    let Some(world) = World::new("c1").await else { return };
    // A room thread, a legacy thread with no kind (fails closed), and a dm: all contained.
    for (name, kind) in [("room", Some("room")), ("legacy", None), ("dm", Some("dm"))] {
        let thread = world.space_thread(name, "owner-1", kind).await;
        world.owner_turn(&thread, "owner-1", REMEMBER).await.expect("the owner's first turn");
        assert_eq!(world.user_memory_count("owner-1").await, 0, "{name}: nothing learned into the owner's memory");
        assert_eq!(world.dream_runs(&thread, "append_message").await, 0, "{name}: no dream run recorded");
    }
    assert_eq!(world.memory_count().await, 0);
    world.finish().await;
}

#[tokio::test]
#[ignore = "needs DB harness"]
async fn personal_thread_still_learns_and_recalls() {
    let _flag = ContainmentFlag::set(None);
    let Some(world) = World::new("pers").await else { return };
    let thread = world.space_thread("personal", "owner-1", Some("personal")).await;
    world.owner_turn(&thread, "owner-1", REMEMBER).await.expect("the owner's first turn");
    assert!(world.user_memory_count("owner-1").await >= 1, "a personal Space still learns");
    assert_eq!(world.dream_runs(&thread, "append_message").await, 1);
    let recalled = crate::dreaming::load_agent_memory_context_rows(&world.pool, &thread, None, 20, MemoryAccess::Full)
        .await
        .expect("context rows");
    assert!(recalled.iter().any(|(_, text)| text.contains("Kari")), "{recalled:?}");

    // An ordinary chat thread (no Space) is unchanged too.
    let plain = world.plain_thread("owner-2").await;
    world.owner_turn(&plain, "owner-2", REMEMBER).await.expect("plain turn");
    assert!(world.user_memory_count("owner-2").await >= 1);
    world.finish().await;
}

#[tokio::test]
#[ignore = "needs DB harness"]
async fn legacy_personal_thread_is_stamped_by_next_verified_decision_and_learns_again() {
    let _flag = ContainmentFlag::set(None);
    let _keys = StreamKeysEnv::publish();
    let Some(world) = World::new("heal").await else { return };
    let thread = world.space_thread("legacy", "owner-1", None).await;
    // Before: no kind, so contained, so the owner's first turn teaches nothing.
    world.owner_turn(&thread, "owner-1", REMEMBER).await.expect("first turn");
    assert_eq!(world.kind_of(&thread).await, None);
    assert_eq!(world.user_memory_count("owner-1").await, 0);

    // The next turn carries a verified decision whose kind is personal: the thread is stamped
    // and the very same append learns.
    let request = signed_append(&world.org, "owner-1", &thread, &world.space, "Husk at jeg heter Lise.", "heal-1", Some("personal"));
    append_message_inner(&world.pool, None, MemoryRetention::Durable, request, "owner-1")
        .await
        .expect("a verified append");
    assert_eq!(world.kind_of(&thread).await.as_deref(), Some("personal"));
    assert!(world.user_memory_count("owner-1").await >= 1, "learns again once stamped");

    // A stamped kind is not rewritten by a later decision (self-heal only fills NULL).
    let later = signed_append(&world.org, "owner-1", &thread, &world.space, "ok", "heal-2", Some("room"));
    append_message_inner(&world.pool, None, MemoryRetention::Durable, later, "owner-1")
        .await
        .expect("a later verified append");
    assert_eq!(world.kind_of(&thread).await.as_deref(), Some("personal"));
    world.finish().await;
}

#[tokio::test]
#[ignore = "needs DB harness"]
async fn legacy_thread_stamped_room_by_a_verified_decision_stays_contained() {
    let _flag = ContainmentFlag::set(None);
    let _keys = StreamKeysEnv::publish();
    let Some(world) = World::new("healroom").await else { return };
    let thread = world.space_thread("legacy", "owner-1", None).await;
    world.owner_turn(&thread, "owner-1", "hello").await.expect("first turn");
    let request = signed_append(&world.org, "owner-1", &thread, &world.space, REMEMBER, "hr-1", Some("room"));
    append_message_inner(&world.pool, None, MemoryRetention::Durable, request, "owner-1")
        .await
        .expect("a verified append");
    assert_eq!(world.kind_of(&thread).await.as_deref(), Some("room"));
    assert_eq!(world.user_memory_count("owner-1").await, 0);
    world.finish().await;
}

#[tokio::test]
#[ignore = "needs DB harness"]
async fn thread_create_with_kind_stamps_column() {
    let _flag = ContainmentFlag::set(None);
    let _keys = StreamKeysEnv::publish();
    let Some(world) = World::new("create").await else { return };
    for (claimed, stored) in [(Some("room"), Some("room")), (Some("personal"), Some("personal")), (None, None)] {
        let session = format!("sess-{}", new_ulid());
        let request = signed_create(&world.org, "owner-1", &world.space, &session, claimed);
        let created = create_thread_inner(&world.pool, request).await.expect("create").into_inner();
        assert_eq!(world.kind_of(&created.thread_id).await.as_deref(), stored, "claimed {claimed:?}");
    }
    world.finish().await;
}

#[tokio::test]
#[ignore = "needs DB harness"]
async fn dm_thread_stamps_kind_and_is_contained() {
    let _flag = ContainmentFlag::set(None);
    let _keys = StreamKeysEnv::publish();
    let Some(world) = World::new("dm").await else { return };
    let request = signed_create(&world.org, "owner-1", &world.space, "sess-dm", Some("dm"));
    let created = create_thread_inner(&world.pool, request).await.expect("create").into_inner();
    assert_eq!(world.kind_of(&created.thread_id).await.as_deref(), Some("dm"));
    assert!(memory_scope::thread_is_contained_by_id(&world.pool, &created.thread_id).await);
    assert!(matches!(
        memory_scope::reader_scope_for_thread(&world.pool, &created.thread_id).await,
        Ok(memory_scope::ReaderScope::Contained { .. })
    ));
    world.finish().await;
}

#[tokio::test]
#[ignore = "needs DB harness"]
async fn a_retried_create_stamps_a_legacy_thread_but_never_rewrites_a_kind() {
    let _flag = ContainmentFlag::set(None);
    let _keys = StreamKeysEnv::publish();
    let Some(world) = World::new("retry").await else { return };
    // The first create carries no kind (an old Control): NULL, contained.
    let first = create_thread_inner(&world.pool, signed_create(&world.org, "owner-1", &world.space, "sess-r", None))
        .await
        .expect("first create")
        .into_inner();
    assert_eq!(world.kind_of(&first.thread_id).await, None);
    // A retry with a verified kind finds the same thread and stamps it.
    let retried = create_thread_inner(&world.pool, signed_create(&world.org, "owner-1", &world.space, "sess-r", Some("personal")))
        .await
        .expect("retry")
        .into_inner();
    assert_eq!(retried.thread_id, first.thread_id);
    assert_eq!(world.kind_of(&first.thread_id).await.as_deref(), Some("personal"));
    // Another retry claiming a different kind does not rewrite it.
    create_thread_inner(&world.pool, signed_create(&world.org, "owner-1", &world.space, "sess-r", Some("room")))
        .await
        .expect("second retry");
    assert_eq!(world.kind_of(&first.thread_id).await.as_deref(), Some("personal"));
    world.finish().await;
}

#[tokio::test]
#[ignore = "needs DB harness"]
async fn contained_thread_dream_loop_skips_and_records_space_scope() {
    let _flag = ContainmentFlag::set(None);
    let Some(world) = World::new("dream").await else { return };
    let room = world.space_thread("room", "owner-1", Some("room")).await;
    let legacy = world.space_thread("legacy", "owner-1", None).await;
    let mut message_ids = Vec::new();
    for (n, thread) in [&room, &legacy].into_iter().enumerate() {
        let id = format!("{thread}-m{n}");
        test_support::seed_space_message(&world.pool, &world.space, thread, &id, "user", Some("owner-1")).await;
        sqlx::query("UPDATE messages SET content = $2 WHERE id = $1")
            .bind(&id)
            .bind(REMEMBER)
            .execute(&world.pool)
            .await
            .expect("message text");
        message_ids.push(id);
    }
    crate::dreaming::dream_once(&world.pool, None, None, None, 1000)
        .await
        .expect("dream loop");
    for thread in [&room, &legacy] {
        assert_eq!(world.dream_runs(thread, "skipped_space_scope").await, 1, "{thread}");
        assert_eq!(world.dream_runs(thread, "append_message").await, 0);
    }
    assert_eq!(world.memory_count().await, 0, "nothing read into any memory");
    // The skip is in the ledger, so the next cycle does not pick the messages up again.
    crate::dreaming::dream_once(&world.pool, None, None, None, 1000)
        .await
        .expect("second cycle");
    assert_eq!(world.dream_runs(&room, "skipped_space_scope").await, 1);
    world.finish().await;
}

fn identity_for(org: &str, user: &str) -> crate::auth::VerifiedIdentity {
    crate::auth::VerifiedIdentity::user_for_test(org, user)
}

fn rpc<T>(value: T, identity: crate::auth::VerifiedIdentity) -> tonic::Request<T> {
    let mut request = tonic::Request::new(value);
    request.extensions_mut().insert(identity);
    request
}

#[tokio::test]
#[ignore = "needs DB harness"]
async fn index_memory_refuses_user_policy_agent_workspace_in_contained_thread() {
    let _flag = ContainmentFlag::set(None);
    let Some(world) = World::new("index").await else { return };
    let room = world.space_thread("room", "owner-1", Some("room")).await;
    let service = crate::memory_grpc::MemoryGrpc::new(world.pool.clone(), None);
    for topic in ["USER", "POLICY", "AGENT", "WORKSPACE", "user", "Policy"] {
        let refused = service
            .index_memory(rpc(
                IndexMemoryRequest {
                    org_id: world.org.clone(),
                    thread_id: room.clone(),
                    topic: topic.to_owned(),
                    content: "a wide note".to_owned(),
                    ..Default::default()
                },
                identity_for(&world.org, "owner-1"),
            ))
            .await
            .expect_err("a room may not write beyond its thread");
        assert_eq!(refused.code(), tonic::Code::FailedPrecondition, "{topic}");
        assert!(refused.message().starts_with("memory_scope_contained"), "{topic}: {}", refused.message());
    }
    assert_eq!(world.memory_count().await, 0, "nothing was written");

    // A thread-scope note is still allowed, and it lands on this thread only.
    let saved = service
        .index_memory(rpc(
            IndexMemoryRequest {
                org_id: world.org.clone(),
                thread_id: room.clone(),
                topic: "MEMORY".to_owned(),
                content: "the room decided on Friday".to_owned(),
                ..Default::default()
            },
            identity_for(&world.org, "owner-1"),
        ))
        .await
        .expect("a thread note is allowed")
        .into_inner();
    assert!(!saved.memory_id.is_empty());
    let (scope, session): (String, Option<String>) =
        sqlx::query_as("SELECT scope, session_id FROM agent_memory WHERE id = $1")
            .bind(&saved.memory_id)
            .fetch_one(&world.pool)
            .await
            .expect("saved row");
    assert_eq!((scope.as_str(), session.as_deref()), ("thread", Some(room.as_str())));

    // The same wide topic from a personal Space thread is unchanged.
    let personal = world.space_thread("personal", "owner-1", Some("personal")).await;
    let ok = service
        .index_memory(rpc(
            IndexMemoryRequest {
                org_id: world.org.clone(),
                thread_id: personal,
                topic: "USER".to_owned(),
                content: "likes tea".to_owned(),
                ..Default::default()
            },
            identity_for(&world.org, "owner-1"),
        ))
        .await
        .expect("a personal thread may write USER memory")
        .into_inner();
    assert!(!ok.memory_id.is_empty());
    world.finish().await;
}

#[tokio::test]
#[ignore = "needs DB harness"]
async fn index_agent_memory_writes_nothing_wide_for_a_contained_caller() {
    let _flag = ContainmentFlag::set(None);
    let Some(world) = World::new("indexfn").await else { return };
    let room = world.space_thread("room", "owner-1", Some("room")).await;
    for topic in ["USER", "POLICY", "AGENT", "WORKSPACE"] {
        let id = crate::dreaming::index_agent_memory(&world.pool, &world.org, &room, "owner-1", topic, "x note")
            .await
            .expect("index");
        assert_eq!(id, "", "{topic}: the empty id means nothing was written");
    }
    assert_eq!(world.memory_count().await, 0);
    world.finish().await;
}

#[tokio::test]
#[ignore = "needs DB harness"]
async fn contained_reader_never_sees_user_org_or_global_rows() {
    let _flag = ContainmentFlag::set(None);
    let Some(world) = World::new("read").await else { return };
    let room = world.space_thread("room", "owner-1", Some("room")).await;
    let legacy = world.space_thread("legacy", "owner-1", None).await;
    let personal = world.space_thread("personal", "owner-1", Some("personal")).await;
    world.memory(None, "user", "owner-1", "user fact about Kari", &["thread:elsewhere"]).await;
    world.memory(None, "org", "", "org policy wording", &[]).await;
    world.memory(None, "global", "", "global note", &[]).await;
    world.memory(Some(&room), "thread", "", "room thread note", &[&format!("thread:{room}")]).await;
    world.memory(Some(&personal), "thread", "", "personal thread note", &[&format!("thread:{personal}")]).await;

    let search = |thread: String| {
        let pool = world.pool.clone();
        let org = world.org.clone();
        async move {
            crate::dreaming::search_agent_memory(
                &pool,
                &crate::dreaming::AgentMemorySearch {
                    org_id: &org,
                    thread_id: &thread,
                    owner_user_id: "owner-1",
                    query: "",
                    topic_filter: &[],
                    limit: 50,
                    updated_after: None,
                    access: MemoryAccess::Full,
                },
            )
            .await
            .expect("search")
            .into_iter()
            .map(|row| row.content)
            .collect::<Vec<_>>()
        }
    };
    let context = |thread: String| {
        let pool = world.pool.clone();
        async move {
            crate::dreaming::load_agent_memory_context_rows(&pool, &thread, None, 50, MemoryAccess::Full)
                .await
                .expect("context")
                .into_iter()
                .map(|(_, text)| text)
                .collect::<Vec<_>>()
        }
    };

    // The room (and the kind-less legacy thread, which fails closed): only its own rows.
    let in_room = context(room.clone()).await;
    assert_eq!(in_room.len(), 1, "{in_room:?}");
    assert!(in_room[0].contains("room thread note"));
    assert_eq!(search(room.clone()).await, vec!["room thread note".to_owned()]);
    assert!(context(legacy.clone()).await.is_empty());
    assert!(search(legacy.clone()).await.is_empty());

    // A personal thread still reads user, org and global rows, and its own thread rows.
    let in_personal = context(personal.clone()).await;
    for expected in ["Kari", "org policy wording", "global note", "personal thread note"] {
        assert!(in_personal.iter().any(|text| text.contains(expected)), "{expected}: {in_personal:?}");
    }
    assert!(!in_personal.iter().any(|text| text.contains("room thread note")));
    let found = search(personal).await;
    assert_eq!(found.len(), 4, "{found:?}");
    world.finish().await;
}

#[tokio::test]
#[ignore = "needs DB harness"]
async fn legacy_room_origin_row_is_excluded_for_other_threads() {
    // C7: a user-scope row learned in a shared room before containment existed carries
    // `thread:<contained id>` in source_links; no other thread may recall it.
    let _flag = ContainmentFlag::set(None);
    let Some(world) = World::new("c7").await else { return };
    let room = world.space_thread("room", "owner-1", Some("room")).await;
    let personal = world.space_thread("personal", "owner-1", Some("personal")).await;
    let plain = world.plain_thread("owner-1").await;
    world.memory(None, "user", "owner-1", "learned in the room", &[&format!("thread:{room}"), "message:m1"]).await;
    world.memory(None, "user", "owner-1", "learned in personal", &[&format!("thread:{personal}")]).await;
    world.memory(None, "user", "owner-1", "learned in chat", &[&format!("thread:{plain}")]).await;
    world.memory(None, "user", "owner-1", "learned from a vanished thread", &["thread:missing-thread"]).await;
    world.memory(None, "user", "owner-1", "no source at all", &[]).await;

    for reader in [&personal, &plain] {
        let rows = crate::dreaming::load_agent_memory_context_rows(&world.pool, reader, None, 50, MemoryAccess::Full)
            .await
            .expect("context");
        let texts: Vec<&str> = rows.iter().map(|(_, text)| text.as_str()).collect();
        assert!(!texts.iter().any(|t| t.contains("learned in the room")), "{texts:?}");
        for kept in ["learned in personal", "learned in chat", "vanished thread", "no source at all"] {
            assert!(texts.iter().any(|t| t.contains(kept)), "{kept}: {texts:?}");
        }
        let searched = crate::dreaming::search_agent_memory(
            &world.pool,
            &crate::dreaming::AgentMemorySearch {
                org_id: &world.org,
                thread_id: reader,
                owner_user_id: "owner-1",
                query: "",
                topic_filter: &[],
                limit: 50,
                updated_after: None,
                access: MemoryAccess::Full,
            },
        )
        .await
        .expect("search");
        assert!(!searched.iter().any(|row| row.content.contains("learned in the room")));
        assert_eq!(searched.len(), 4);
    }
    // The room's own thread-scope row, which links to its own thread, stays visible to it (C7).
    world.memory(Some(&room), "thread", "", "room own note", &[&format!("thread:{room}")]).await;
    let own = crate::dreaming::load_agent_memory_context_rows(&world.pool, &room, None, 50, MemoryAccess::Full)
        .await
        .expect("room context");
    assert!(own.iter().any(|(_, text)| text.contains("room own note")), "{own:?}");
    world.finish().await;
}

#[tokio::test]
#[ignore = "needs DB harness"]
async fn flag_off_restores_old_behaviour() {
    let _flag = ContainmentFlag::set(Some("off"));
    let Some(world) = World::new("off").await else { return };
    let room = world.space_thread("room", "owner-1", Some("room")).await;
    let personal = world.space_thread("personal", "owner-1", Some("personal")).await;
    world.memory(None, "user", "owner-1", "learned in the room", &[&format!("thread:{room}")]).await;
    world.memory(None, "org", "", "org policy wording", &[]).await;

    // Reads: a room thread sees user and org rows again, and the legacy row is not excluded.
    let in_room = crate::dreaming::load_agent_memory_context_rows(&world.pool, &room, None, 50, MemoryAccess::Full)
        .await
        .expect("context");
    for expected in ["learned in the room", "org policy wording"] {
        assert!(in_room.iter().any(|(_, text)| text.contains(expected)), "{expected}: {in_room:?}");
    }
    let in_personal = crate::dreaming::load_agent_memory_context_rows(&world.pool, &personal, None, 50, MemoryAccess::Full)
        .await
        .expect("personal context");
    assert!(in_personal.iter().any(|(_, text)| text.contains("learned in the room")));

    // Writes: the room's owner turn learns, and a wide topic is no longer refused.
    world.owner_turn(&room, "owner-1", REMEMBER).await.expect("turn");
    assert!(world.user_memory_count("owner-1").await >= 2, "the room turn is learned again");
    let service = crate::memory_grpc::MemoryGrpc::new(world.pool.clone(), None);
    service
        .index_memory(rpc(
            IndexMemoryRequest {
                org_id: world.org.clone(),
                thread_id: room.clone(),
                topic: "USER".to_owned(),
                content: "wide again".to_owned(),
                ..Default::default()
            },
            identity_for(&world.org, "owner-1"),
        ))
        .await
        .expect("flag off: a wide topic is allowed");

    // The owner's list shows nothing as withheld (the rows are recalled again).
    let listed = crate::dreaming::list_user_memory(&world.pool, &world.org, "owner-1", 100)
        .await
        .expect("list");
    assert!(listed.iter().all(|row| row.withheld_reason.is_empty() && row.origin_space_ref.is_empty()));
    world.finish().await;
}

#[tokio::test]
#[ignore = "needs DB harness"]
async fn legacy_room_origin_rows_are_withheld_and_listed() {
    let _flag = ContainmentFlag::set(None);
    let Some(world) = World::new("list").await else { return };
    let room = world.space_thread("room", "owner-1", Some("room")).await;
    let personal = world.space_thread("personal", "owner-1", Some("personal")).await;
    let plain = world.plain_thread("owner-1").await;
    world.memory(None, "user", "owner-1", "from the room", &[&format!("thread:{room}"), "message:m1"]).await;
    world.memory(None, "user", "owner-1", "from personal", &[&format!("thread:{personal}")]).await;
    world.memory(None, "user", "owner-1", "from chat", &[&format!("thread:{plain}")]).await;
    world.memory(None, "user", "owner-1", "ordinary", &[]).await;

    let listed = crate::dreaming::list_user_memory(&world.pool, &world.org, "owner-1", 100)
        .await
        .expect("list");
    assert_eq!(listed.len(), 4, "a withheld row is listed, never hidden");
    for row in &listed {
        if row.content == "from the room" {
            assert_eq!(row.withheld_reason, memory_scope::WITHHELD_ROOM_ORIGIN);
            assert_eq!(row.origin_space_ref, world.space);
        } else {
            assert_eq!(row.withheld_reason, "", "{}", row.content);
            assert_eq!(row.origin_space_ref, "", "{}", row.content);
        }
    }

    // The gRPC list maps both fields into MemoryEntry.
    let service = crate::memory_grpc::MemoryGrpc::new(world.pool.clone(), None);
    let response = service
        .list_memory(rpc(
            ListMemoryRequest {
                org_id: world.org.clone(),
                user_id: "owner-1".to_owned(),
                limit: 50,
            },
            identity_for(&world.org, "owner-1"),
        ))
        .await
        .expect("list_memory")
        .into_inner();
    let withheld: Vec<_> = response.entries.iter().filter(|e| e.withheld_reason == "room_origin_unverified").collect();
    assert_eq!(withheld.len(), 1);
    assert_eq!(withheld[0].origin_space_ref, world.space);
    assert_eq!(withheld[0].content, "from the room");
    assert!(response.entries.iter().filter(|e| e.withheld_reason.is_empty()).all(|e| e.origin_space_ref.is_empty()));

    // And it is the same row the recall filter withholds from a personal thread.
    let recalled = crate::dreaming::load_agent_memory_context_rows(&world.pool, &personal, None, 50, MemoryAccess::Full)
        .await
        .expect("context");
    assert!(!recalled.iter().any(|(_, text)| text.contains("from the room")));
    world.finish().await;
}

#[tokio::test]
#[ignore = "needs DB harness"]
async fn letta_not_called_for_contained_thread() {
    let _flag = ContainmentFlag::set(None);
    let Some(world) = World::new("letta").await else { return };
    // The semantic backend is reached through a token minted by auth-core: any call at all
    // would hit this server, so zero requests proves the mirror was never asked.
    let auth = MockServer::start().await;
    Mock::given(any()).respond_with(ResponseTemplate::new(500)).expect(0).mount(&auth).await;
    let letta = crate::letta_adapter::LettaMemoryAdapter::new_for_test(
        "http://127.0.0.1:9",
        &auth.uri(),
        "session-core",
        "session-core-test-credential",
    );
    let service = crate::memory_grpc::MemoryGrpc::new(world.pool.clone(), Some(letta));
    let room = world.space_thread("room", "owner-1", Some("room")).await;

    let indexed = service
        .index_memory(rpc(
            IndexMemoryRequest {
                org_id: world.org.clone(),
                thread_id: room.clone(),
                topic: "MEMORY".to_owned(),
                content: "the room decided on Friday".to_owned(),
                ..Default::default()
            },
            identity_for(&world.org, "owner-1"),
        ))
        .await
        .expect("a thread note is written")
        .into_inner();
    assert!(!indexed.memory_id.is_empty());
    assert!(indexed.degraded);
    assert_eq!(indexed.degradation_reason, memory_scope::DEGRADED_LETTA_SPACE_SUPPRESSED);

    let searched = service
        .search_memory(rpc(
            SearchMemoryRequest {
                org_id: world.org.clone(),
                thread_id: room,
                query: "Friday".to_owned(),
                limit: 5,
                ..Default::default()
            },
            identity_for(&world.org, "owner-1"),
        ))
        .await
        .expect("search")
        .into_inner();
    assert!(searched.entries.iter().all(|entry| entry.content.contains("Friday")));
    auth.verify().await;
    world.finish().await;
}
