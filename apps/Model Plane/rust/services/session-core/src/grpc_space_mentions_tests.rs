//! WS08-S2 tests that need the private verifiers and write paths of `grpc.rs` (test-only child
//! module). The pure tier of `space::mentions` (reply slot, input validation, derived wake state,
//! the shared golden digest) lives in that module.
//!
//! * Pure tier: `verify_run_wake_space_decision_with_key` admits exactly one wake (the table
//!   below), and a zero-retention caller gets the typed refusal before any database work (a lazy,
//!   unreachable pool proves nothing was read or written).
//! * DB tier (every test `#[ignore]`d, needs `DATABASE_URL` to a disposable `*test*` Postgres, run
//!   with `-- --ignored --test-threads=1`): mention rows written with the post and erased with the
//!   thread, the owner-only downgrade, reserved metadata keys, the wake's stored-fact check, the
//!   extended reply slot, start-key replay, the derived wake state and the event payload.

use super::*;
use crate::retention_receipt::RunRetention;
use crate::space::mentions::{self, VerifiedWake};
use crate::space::test_support;
use ed25519_dalek::{Signer as _, SigningKey};
use mp_contracts::space_mentions as sm;
use mp_contracts::zdr_refusal::code as zdr_code;
use serde_json::{json, Value};
use std::sync::{Mutex, MutexGuard};

use super::space_w1_tests::{signing_key, KEY_ID};

// ---- token minting -------------------------------------------------------------------------

const ORG: &str = "org-1";
const CALLER: &str = "user-2";
const THREAD: &str = "thread-1";
const SPACE: &str = "space-room-1";

fn sign_with(key: &SigningKey, claims: &Value) -> String {
    let key_part = URL_SAFE_NO_PAD.encode(KEY_ID.as_bytes());
    let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims).expect("claims"));
    let signing_input = format!("{CONTROL_SPACE_DECISION_VERSION}.{key_part}.{payload}");
    format!(
        "{signing_input}.{}",
        URL_SAFE_NO_PAD.encode(key.sign(signing_input.as_bytes()).to_bytes())
    )
}

/// The wake an issued decision covers, before its token and digest are filled in.
fn unsigned_wake() -> pb::RunWakeContext {
    pb::RunWakeContext {
        space_id: SPACE.to_owned(),
        space_decision_ref: "wake-decision-1".to_owned(),
        recipient_audience_ref: format!("audience:{SPACE}:4"),
        recipient_audience_revision: 4,
        recipient_audience_hash: "sha256:test-audience".to_owned(),
        privacy_policy_ref: "privacy:org-1:5".to_owned(),
        authority_revision: 7,
        resource_authorization_ref: format!("control:{SPACE}:run-wake:7"),
        action_schema_hash: sm::DECISION_SCHEMA.to_owned(),
        idempotency_key: "wake-idem-1".to_owned(),
        trigger_message_id: "message-9".to_owned(),
        trigger_content_digest: mentions::content_digest("hello room @agent-7"),
        agent_subject_id: "agent-7".to_owned(),
        turn_origin: sm::ORIGIN_DIRECT.to_owned(),
        ..Default::default()
    }
}

/// Signs `wake` as Control would for `org`/`user`/`thread`; `edit` runs on the claims after the
/// digest is computed, so it can forge any claim.
fn sign_wake(
    mut wake: pb::RunWakeContext,
    org: &str,
    user: &str,
    thread: &str,
    key: &SigningKey,
    edit: impl FnOnce(&mut Value),
) -> pb::RunWakeContext {
    wake.payload_digest = mentions::run_wake_payload_digest(&wake, org, user, thread);
    let now = Utc::now();
    let mut claims = json!({
        "decision_ref": wake.space_decision_ref, "org_id": org, "space_ref": wake.space_id,
        "subject_id": user, "service_audience": CONTROL_SPACE_DECISION_AUDIENCE,
        "action_id": sm::DECISION_ACTION_ID, "action_schema_hash": wake.action_schema_hash,
        "payload_digest": wake.payload_digest, "idempotency_key": wake.idempotency_key,
        "recipient_audience_ref": wake.recipient_audience_ref,
        "recipient_audience_hash": wake.recipient_audience_hash,
        "privacy_policy_ref": wake.privacy_policy_ref,
        "resource_authorization_ref": wake.resource_authorization_ref,
        "purpose": "assistant_collaboration", "lawful_basis": "contract", "privacy_class": "internal",
        "third_party_processing_allowed": false, "retention_class": "standard",
        "residency": "swedencentral", "deletion_scope": "space", "zero_data_retention": false,
        "nonce": "nonce-wake-1", "authority_revision": wake.authority_revision,
        "membership_revision": 4, "privacy_revision": 5,
        "recipient_audience_revision": wake.recipient_audience_revision,
        "entitlement_revision": 3, "permissions": [sm::PERMISSION_WAKE],
        "issued_at": now.to_rfc3339(),
        "expires_at": (now + chrono::Duration::minutes(2)).to_rfc3339(),
    });
    edit(&mut claims);
    wake.space_decision_token = sign_with(key, &claims);
    wake
}

fn verify(wake: &pb::RunWakeContext, org: &str, user: &str, thread: &str) -> Result<VerifiedWake, Status> {
    verify_run_wake_space_decision_with_key(
        wake,
        org,
        user,
        thread,
        KEY_ID,
        &signing_key().verifying_key(),
        Utc::now(),
    )
}

// ---- pure tier -----------------------------------------------------------------------------

#[test]
fn wake_verifier_table() {
    let key = signing_key();
    let good = sign_wake(unsigned_wake(), ORG, CALLER, THREAD, &key, |_| {});
    let verified = verify(&good, ORG, CALLER, THREAD).expect("an issued wake verifies");
    assert_eq!(
        verified,
        VerifiedWake {
            space_id: SPACE.to_owned(),
            trigger_message_id: "message-9".to_owned(),
            trigger_content_digest: mentions::content_digest("hello room @agent-7"),
            agent_subject_id: "agent-7".to_owned(),
            turn_origin: "direct".to_owned(),
        }
    );

    // Each row: what changed after Control signed, and the request it is verified for.
    type Tamper = fn(&mut pb::RunWakeContext);
    let after_signing: [(&str, Tamper); 9] = [
        ("another trigger post", |w| w.trigger_message_id = "message-10".to_owned()),
        ("another content digest", |w| w.trigger_content_digest = mentions::content_digest("other text")),
        ("another agent", |w| w.agent_subject_id = "agent-8".to_owned()),
        ("a forged turn origin", |w| w.turn_origin = sm::ORIGIN_AMBIENT.to_owned()),
        ("another Space", |w| w.space_id = "space-room-2".to_owned()),
        ("another idempotency key", |w| w.idempotency_key = "wake-idem-2".to_owned()),
        ("another audience revision", |w| w.recipient_audience_revision = 5),
        ("a restated payload digest", |w| w.payload_digest = format!("sha256:{}", "0".repeat(64))),
        ("a thread schema", |w| w.action_schema_hash = CONTROL_THREAD_APPEND_SCHEMA.to_owned()),
    ];
    for (name, tamper) in after_signing {
        let mut wake = good.clone();
        tamper(&mut wake);
        let status = verify(&wake, ORG, CALLER, THREAD).expect_err(name);
        assert_eq!(status.code(), tonic::Code::PermissionDenied, "{name}");
    }

    // The same token for another caller, org or thread.
    for (name, org, user, thread) in [
        ("another caller", ORG, "user-3", THREAD),
        ("another org", "org-2", CALLER, THREAD),
        ("another thread", ORG, CALLER, "thread-2"),
    ] {
        let status = verify(&good, org, user, thread).expect_err(name);
        assert_eq!(status.code(), tonic::Code::PermissionDenied, "{name}");
    }

    // Claims Control would never issue for a wake.
    type Forge = fn(&mut Value);
    let forged: [(&str, Forge); 9] = [
        ("thread append permission", |c| c["permissions"] = json!(["thread:append"])),
        ("wake plus append", |c| c["permissions"] = json!([sm::PERMISSION_WAKE, "thread:append"])),
        ("no permission", |c| c["permissions"] = json!([])),
        ("thread append action", |c| c["action_id"] = json!(CONTROL_THREAD_APPEND_ACTION)),
        ("another audience", |c| c["service_audience"] = json!("model-plane-space-stream")),
        ("another subject", |c| c["subject_id"] = json!("user-3")),
        ("expired", |c| {
            c["expires_at"] = json!((Utc::now() - chrono::Duration::seconds(1)).to_rfc3339());
        }),
        ("no nonce", |c| c["nonce"] = json!("")),
        ("no lawful basis", |c| c["lawful_basis"] = json!("")),
    ];
    for (name, forge) in forged {
        let wake = sign_wake(unsigned_wake(), ORG, CALLER, THREAD, &key, forge);
        let status = verify(&wake, ORG, CALLER, THREAD).expect_err(name);
        assert_eq!(status.code(), tonic::Code::PermissionDenied, "{name}");
    }

    // Another signer.
    let foreign = sign_wake(unsigned_wake(), ORG, CALLER, THREAD, &SigningKey::from_bytes(&[7_u8; 32]), |_| {});
    assert_eq!(
        verify(&foreign, ORG, CALLER, THREAD).expect_err("foreign key").code(),
        tonic::Code::PermissionDenied
    );
}

#[test]
fn a_thread_append_decision_is_never_a_wake() {
    // A wake-shaped context whose token is a thread append decision over the same refs.
    let key = signing_key();
    let wake = sign_wake(unsigned_wake(), ORG, CALLER, THREAD, &key, |claims| {
        claims["action_id"] = json!(CONTROL_THREAD_APPEND_ACTION);
        claims["action_schema_hash"] = json!(CONTROL_THREAD_APPEND_SCHEMA);
        claims["permissions"] = json!(["thread:append"]);
    });
    assert_eq!(
        verify(&wake, ORG, CALLER, THREAD).expect_err("disjoint").code(),
        tonic::Code::PermissionDenied
    );
}

fn lazy_pool() -> PgPool {
    sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgresql://unreachable.invalid/session")
        .expect("lazy pool")
}

fn start_request(org: &str, user: &str, thread: &str, start_key: &str) -> pb::StartManagedRunRequest {
    pb::StartManagedRunRequest {
        wake: None,
        thread_id: thread.to_owned(),
        parent_run_id: String::new(),
        agent_id: "general-v1".to_owned(),
        goal: "answer the post".to_owned(),
        mode: "execute".to_owned(),
        org_id: org.to_owned(),
        user_id: user.to_owned(),
        start_key: start_key.to_owned(),
        terminal_source: pb::ManagedRunSource::ExecutionAgent as i32,
    }
}

fn assert_zdr_refusal(status: &Status) {
    assert_eq!(status.code(), tonic::Code::PermissionDenied, "{status:?}");
    assert_eq!(
        mp_contracts::zdr_refusal::Refusal::from_status(status).map(|refusal| refusal.code()),
        Some(zdr_code::DURABLE_MUTATION_FORBIDDEN),
        "{status:?}"
    );
}

#[tokio::test]
async fn zdr_caller_gets_typed_refusal_and_no_rows() {
    // The handler: refused before the owner gate, the switch, the verifier or any read. The pool
    // is unreachable, so any database work would surface as another error.
    let service = ManagedRunLifecycleService {
        pool: lazy_pool(),
        letta_memory: None,
    };
    let mut start = start_request(ORG, CALLER, "", "zdr-wake-1");
    start.wake = Some(sign_wake(unsigned_wake(), ORG, CALLER, THREAD, &signing_key(), |_| {}));
    let mut request = Request::new(start);
    request
        .extensions_mut()
        .insert(crate::auth::VerifiedIdentity::user_for_test_with_zdr(ORG, CALLER, true));
    let status = service
        .start_managed_run(request)
        .await
        .expect_err("a ZDR caller cannot wake");
    assert_zdr_refusal(&status);

    // The inner start is a backstop with the same refusal, also before any database work.
    let wake = VerifiedWake {
        space_id: SPACE.to_owned(),
        trigger_message_id: "message-9".to_owned(),
        trigger_content_digest: mentions::content_digest("x"),
        agent_subject_id: "agent-7".to_owned(),
        turn_origin: "direct".to_owned(),
    };
    let status = crate::terminalization::start_managed_run_with_wake_inner(
        &lazy_pool(),
        start_request(ORG, CALLER, "", "zdr-wake-2"),
        RunRetention::zdr(),
        Some(&wake),
    )
    .await
    .expect_err("the backstop refuses too");
    assert_zdr_refusal(&status);
}

// ---- database tier -------------------------------------------------------------------------

/// Serialises the tests that set `SPACE_MENTION_RECORDS_CORE` and `SPACE_WAKE_CORE`; removes both
/// on drop, so every test starts from the default (both off).
static WAKE_FLAGS_ENV: Mutex<()> = Mutex::new(());

struct WakeFlags {
    _guard: MutexGuard<'static, ()>,
}

impl WakeFlags {
    fn set(records: bool, wake: bool) -> Self {
        let guard = WAKE_FLAGS_ENV
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let flags = Self { _guard: guard };
        flags.switch(records, wake);
        flags
    }

    fn switch(&self, records: bool, wake: bool) {
        env::set_var(sm::RECORDS_CORE_ENV, if records { "1" } else { "0" });
        env::set_var(sm::WAKE_CORE_ENV, if wake { "1" } else { "0" });
    }
}

impl Drop for WakeFlags {
    fn drop(&mut self) {
        env::remove_var(sm::RECORDS_CORE_ENV);
        env::remove_var(sm::WAKE_CORE_ENV);
    }
}

fn person(id: &str) -> pb::MentionInput {
    pb::MentionInput {
        subject_id: id.to_owned(),
        kind: sm::KIND_USER.to_owned(),
        ..Default::default()
    }
}

fn agent(id: &str) -> pb::MentionInput {
    pb::MentionInput {
        subject_id: id.to_owned(),
        kind: sm::KIND_AGENT.to_owned(),
        wake_expectation: sm::WAKE_EXPECTED.to_owned(),
        ..Default::default()
    }
}

fn refused_agent(id: &str, code: &str) -> pb::MentionInput {
    pb::MentionInput {
        subject_id: id.to_owned(),
        kind: sm::KIND_AGENT.to_owned(),
        wake_expectation: sm::WAKE_REFUSED.to_owned(),
        refusal_code: code.to_owned(),
        ..Default::default()
    }
}

type StoredMention = (String, String, Option<String>, Option<String>, i16);

/// One test's world: unique org and Space ids, every thread erased at the end.
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

    async fn thread(&self, owner: &str) -> String {
        let id = format!("thread-wake-{}", new_ulid());
        test_support::seed_space_thread(&self.pool, &self.org, &self.space, &id, owner).await;
        id
    }

    /// The owner's first post in an empty Space thread (admitted without a fresh decision, as the
    /// create's own first message is).
    async fn post(
        &self,
        thread: &str,
        owner: &str,
        content: &str,
        mentions: Vec<pb::MentionInput>,
    ) -> Result<pb::AppendMessageResponse, Status> {
        self.append(pb::AppendMessageRequest {
            thread_id: thread.to_owned(),
            role: "user".to_owned(),
            content: content.to_owned(),
            post_kind: "post".to_owned(),
            mentions,
            ..Default::default()
        }, owner)
        .await
    }

    async fn reply(&self, thread: &str, agent_name: &str) -> Result<pb::AppendMessageResponse, Status> {
        self.append(pb::AppendMessageRequest {
            thread_id: thread.to_owned(),
            role: "assistant".to_owned(),
            content: format!("answer from {agent_name}"),
            agent_name: agent_name.to_owned(),
            ..Default::default()
        }, "")
        .await
    }

    async fn append(&self, request: pb::AppendMessageRequest, author: &str) -> Result<pb::AppendMessageResponse, Status> {
        append_message_inner(&self.pool, None, MemoryRetention::Durable, request, author)
            .await
            .map(Response::into_inner)
    }

    async fn stored_mentions(&self, message_id: &str) -> Vec<StoredMention> {
        sqlx::query_as(
            "SELECT subject_id, subject_kind, wake_expectation, refusal_code, ordinal
             FROM message_mentions WHERE message_id = $1 ORDER BY subject_kind, ordinal, subject_id",
        )
        .bind(message_id)
        .fetch_all(&self.pool)
        .await
        .expect("stored mentions")
    }

    async fn mention_rows_in_thread(&self, thread: &str) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM message_mentions WHERE thread_id = $1")
            .bind(thread)
            .fetch_one(&self.pool)
            .await
            .expect("count mention rows")
    }

    fn verified(&self, trigger: &str, content: &str, agent: &str) -> VerifiedWake {
        VerifiedWake {
            space_id: self.space.clone(),
            trigger_message_id: trigger.to_owned(),
            trigger_content_digest: mentions::content_digest(content),
            agent_subject_id: agent.to_owned(),
            turn_origin: sm::ORIGIN_DIRECT.to_owned(),
        }
    }

    async fn start(
        &self,
        thread: &str,
        owner: &str,
        start_key: &str,
        wake: Option<&VerifiedWake>,
    ) -> Result<crate::terminalization::ManagedRunStart, Status> {
        crate::terminalization::start_managed_run_with_wake_inner(
            &self.pool,
            start_request(&self.org, owner, thread, start_key),
            RunRetention::persistent(),
            wake,
        )
        .await
    }

    async fn finish(self) {
        let threads: Vec<String> = sqlx::query_scalar("SELECT id FROM threads WHERE org_id = $1")
            .bind(&self.org)
            .fetch_all(&self.pool)
            .await
            .unwrap_or_default();
        for thread in threads {
            if let Ok(mut tx) = self.pool.begin().await {
                if delete_thread_rows(&mut tx, &thread).await.is_ok() {
                    tx.commit().await.ok();
                }
            }
        }
        test_support::clean_space(&self.pool, &self.org, &self.space).await;
    }
}

fn error_code(status: &Status) -> Option<String> {
    status
        .metadata()
        .get(crate::run_phase::ERROR_CODE_TRAILER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

#[tokio::test]
#[ignore = "needs DB harness"]
async fn mentions_are_written_with_the_message_and_cascade_on_thread_delete() {
    let flags = WakeFlags::set(true, false);
    let Some(world) = World::new("men").await else { return };
    let thread = world.thread("owner-1").await;
    let mut first = pb::AppendMessageRequest {
        thread_id: thread.clone(),
        role: "user".to_owned(),
        content: "@Ada @X @Y look".to_owned(),
        post_kind: "post".to_owned(),
        client_message_id: format!("cmid-{}", new_ulid()),
        mentions: vec![person("user-ada"), agent("agent-x"), refused_agent("agent-y", "respond_to_denied")],
        ..Default::default()
    };
    let posted = world.append(first.clone(), "owner-1").await.expect("the post");
    assert_eq!(posted.mentions.len(), 3, "{:?}", posted.mentions);
    let stored = world.stored_mentions(&posted.message_id).await;
    assert_eq!(
        stored,
        vec![
            ("agent-x".to_owned(), "agent".to_owned(), Some("expected".to_owned()), None, 0),
            ("agent-y".to_owned(), "agent".to_owned(), Some("refused".to_owned()), Some("respond_to_denied".to_owned()), 1),
            ("user-ada".to_owned(), "user".to_owned(), None, None, 0),
        ]
    );

    // A replay of the same send writes nothing and answers with the stored mentions.
    first.mentions = vec![agent("agent-z")];
    let replay = world.append(first, "owner-1").await.expect("replay");
    assert!(replay.deduplicated);
    assert_eq!(replay.message_id, posted.message_id);
    assert_eq!(replay.mentions.len(), 3);
    assert_eq!(world.mention_rows_in_thread(&thread).await, 3);

    // With the records switch off a post stays, its mentions are ignored.
    flags.switch(false, false);
    let quiet = world.thread("owner-1").await;
    let unrecorded = world.post(&quiet, "owner-1", "@X hi", vec![agent("agent-x")]).await.expect("post stays");
    assert!(unrecorded.mentions.is_empty());
    assert_eq!(world.mention_rows_in_thread(&quiet).await, 0);
    flags.switch(true, false);

    // A turn never records mentions, even with the switch on.
    let turns = world.thread("owner-1").await;
    let turn = world
        .append(pb::AppendMessageRequest {
            thread_id: turns.clone(),
            role: "user".to_owned(),
            content: "plain turn".to_owned(),
            mentions: vec![agent("agent-x")],
            ..Default::default()
        }, "owner-1")
        .await
        .expect("a turn");
    assert!(turn.mentions.is_empty());
    assert_eq!(world.mention_rows_in_thread(&turns).await, 0);

    // Deleting the thread carries the rows with it (ON DELETE CASCADE from messages).
    let mut tx = world.pool.begin().await.expect("begin");
    delete_thread_rows(&mut tx, &thread).await.expect("delete thread rows");
    tx.commit().await.expect("commit");
    assert_eq!(world.mention_rows_in_thread(&thread).await, 0);
    world.finish().await;
}

#[tokio::test]
#[ignore = "needs DB harness"]
async fn non_owner_agent_mention_is_downgraded_to_owner_only_thread() {
    let _flags = WakeFlags::set(true, true);
    let Some(world) = World::new("down").await else { return };
    let thread = world.thread("owner-1").await;
    // A member's post in the owner's thread (the member path verified its own decision; here
    // the row is seeded and the mention write is the one the append makes for a non-owner).
    let message_id = format!("m-member-{}", new_ulid());
    test_support::seed_space_message(&world.pool, &world.space, &thread, &message_id, "user", Some("member-2")).await;
    sqlx::query("UPDATE messages SET post_kind = 'post' WHERE id = $1")
        .bind(&message_id)
        .execute(&world.pool)
        .await
        .expect("mark post");
    let valid = mentions::validate_mentions(&[agent("agent-x"), person("owner-1")], false).expect("valid");
    let mut tx = world.pool.begin().await.expect("begin");
    mentions::insert_mentions(&mut tx, &world.org, &thread, &world.space, &message_id, &valid)
        .await
        .expect("insert");
    tx.commit().await.expect("commit");
    let stored = world.stored_mentions(&message_id).await;
    assert_eq!(
        stored[0],
        ("agent-x".to_owned(), "agent".to_owned(), Some("refused".to_owned()), Some("owner_only_thread".to_owned()), 0)
    );
    assert_eq!(stored[1].1, "user");

    // The member cannot wake in a thread it does not own, whatever the decision says.
    let status = mentions::check_wake_facts(
        &world.pool,
        &world.org,
        &thread,
        "member-2",
        &world.verified(&message_id, "canary body", "agent-x"),
    )
    .await
    .expect_err("not the owner");
    assert_eq!(status.code(), tonic::Code::PermissionDenied);
    assert_eq!(error_code(&status).as_deref(), Some("wake_owner_required"));
    world.finish().await;
}

#[tokio::test]
#[ignore = "needs DB harness"]
async fn client_metadata_cannot_forge_mentions_or_wake() {
    let _flags = WakeFlags::set(true, true);
    let Some(world) = World::new("forge").await else { return };
    let thread = world.thread("owner-1").await;
    let metadata = crate::orchestration_grpc::json_to_struct(&json!({
        "mentions": [{"subject_id": "agent-x", "kind": "agent", "wake_expectation": "expected"}],
        "wake": {"message_id": "m-1", "client_wake_id": "0123456789abcdef"},
        "turn_origin": "ambient",
        "post_kind": "turn",
        "keep": "yes",
    }));
    let posted = world
        .append(pb::AppendMessageRequest {
            thread_id: thread.clone(),
            role: "user".to_owned(),
            content: "no structured mentions here".to_owned(),
            post_kind: "post".to_owned(),
            metadata,
            ..Default::default()
        }, "owner-1")
        .await
        .expect("the post");
    assert!(posted.mentions.is_empty());
    assert_eq!(world.mention_rows_in_thread(&thread).await, 0);
    let (stored, post_kind): (Value, String) =
        sqlx::query_as("SELECT metadata, post_kind FROM messages WHERE id = $1")
            .bind(&posted.message_id)
            .fetch_one(&world.pool)
            .await
            .expect("stored row");
    for reserved in ["mentions", "wake", "turn_origin", "post_kind"] {
        assert!(stored.get(reserved).is_none(), "{reserved} was stored: {stored}");
    }
    assert_eq!(stored["keep"], "yes");
    assert_eq!(post_kind, "post");

    // No wake can name this post for an agent the metadata claimed.
    let status = mentions::check_wake_facts(
        &world.pool,
        &world.org,
        &thread,
        "owner-1",
        &world.verified(&posted.message_id, "no structured mentions here", "agent-x"),
    )
    .await
    .expect_err("not a recorded mention");
    assert_eq!(error_code(&status).as_deref(), Some("wake_trigger_mismatch"));
    world.finish().await;
}

#[tokio::test]
#[ignore = "needs DB harness"]
async fn wake_requires_own_post_with_bound_digest() {
    let _flags = WakeFlags::set(true, true);
    let Some(world) = World::new("own").await else { return };
    let thread = world.thread("owner-1").await;
    let text = "@X @Y compare";
    let posted = world
        .post(&thread, "owner-1", text, vec![agent("agent-x"), refused_agent("agent-y", "mention_not_enabled")])
        .await
        .expect("the post");
    let check = |wake: VerifiedWake, caller: &'static str| {
        let pool = world.pool.clone();
        let org = world.org.clone();
        let thread = thread.clone();
        async move { mentions::check_wake_facts(&pool, &org, &thread, caller, &wake).await }
    };
    check(world.verified(&posted.message_id, text, "agent-x"), "owner-1")
        .await
        .expect("the owner's own post with the bound digest admits the wake");

    for (name, wake) in [
        ("edited text", world.verified(&posted.message_id, "@X @Y compare!", "agent-x")),
        ("refused agent", world.verified(&posted.message_id, text, "agent-y")),
        ("unmentioned agent", world.verified(&posted.message_id, text, "agent-z")),
        ("unknown message", world.verified("m-unknown", text, "agent-x")),
    ] {
        let status = check(wake, "owner-1").await.expect_err(name);
        assert_eq!(status.code(), tonic::Code::FailedPrecondition, "{name}");
        assert_eq!(error_code(&status).as_deref(), Some("wake_trigger_mismatch"), "{name}");
    }

    // An assistant row is not a post.
    let answer = world.reply(&thread, "X").await.expect("the reply slot is open after the post");
    let status = check(world.verified(&answer.message_id, "answer from X", "agent-x"), "owner-1")
        .await
        .expect_err("an assistant row");
    assert_eq!(error_code(&status).as_deref(), Some("wake_trigger_mismatch"));

    // The decision's Space must be the thread's.
    let mut elsewhere = world.verified(&posted.message_id, text, "agent-x");
    elsewhere.space_id = "space-other".to_owned();
    assert_eq!(check(elsewhere, "owner-1").await.expect_err("another Space").code(), tonic::Code::PermissionDenied);

    // A trigger from another thread is not this thread's message.
    let other = world.thread("owner-1").await;
    let foreign = world.post(&other, "owner-1", text, vec![agent("agent-x")]).await.expect("other post");
    let status = check(world.verified(&foreign.message_id, text, "agent-x"), "owner-1")
        .await
        .expect_err("another thread's post");
    assert_eq!(error_code(&status).as_deref(), Some("wake_trigger_mismatch"));
    world.finish().await;
}

#[tokio::test]
#[ignore = "needs DB harness"]
async fn wake_with_member_post_is_refused() {
    let _flags = WakeFlags::set(true, true);
    let Some(world) = World::new("memb").await else { return };
    let thread = world.thread("owner-1").await;
    let message_id = format!("m-member-{}", new_ulid());
    test_support::seed_space_message(&world.pool, &world.space, &thread, &message_id, "user", Some("member-2")).await;
    sqlx::query("UPDATE messages SET post_kind = 'post' WHERE id = $1")
        .bind(&message_id)
        .execute(&world.pool)
        .await
        .expect("mark post");
    // Even an `expected` row (as if written by an owner) does not let the owner run the member's
    // words as its own turn.
    let valid = mentions::validate_mentions(&[agent("agent-x")], true).expect("valid");
    let mut tx = world.pool.begin().await.expect("begin");
    mentions::insert_mentions(&mut tx, &world.org, &thread, &world.space, &message_id, &valid)
        .await
        .expect("insert");
    tx.commit().await.expect("commit");
    let status = mentions::check_wake_facts(
        &world.pool,
        &world.org,
        &thread,
        "owner-1",
        &world.verified(&message_id, "canary body", "agent-x"),
    )
    .await
    .expect_err("a member's post");
    assert_eq!(status.code(), tonic::Code::FailedPrecondition);
    assert_eq!(error_code(&status).as_deref(), Some("wake_trigger_mismatch"));
    world.finish().await;
}

#[tokio::test]
#[ignore = "needs DB harness"]
async fn second_agent_reply_is_allowed_third_is_not() {
    let flags = WakeFlags::set(true, true);
    let Some(world) = World::new("slot").await else { return };
    let thread = world.thread("owner-1").await;
    world
        .post(&thread, "owner-1", "@X @Y compare", vec![agent("agent-x"), agent("agent-y")])
        .await
        .expect("the post");
    world.reply(&thread, "X").await.expect("the first addressed agent answers");
    world.reply(&thread, "Y").await.expect("the second addressed agent answers");
    let status = world.reply(&thread, "Z").await.expect_err("no third answer for two agents");
    assert_eq!(status.code(), tonic::Code::PermissionDenied);

    // A refused mention opens no slot.
    let refused = world.thread("owner-1").await;
    world
        .post(&refused, "owner-1", "@X @Y", vec![agent("agent-x"), refused_agent("agent-y", "respond_to_denied")])
        .await
        .expect("the post");
    world.reply(&refused, "X").await.expect("the expected agent answers");
    assert!(world.reply(&refused, "Y").await.is_err(), "a refused agent gets no slot");

    // With the wake switch off the old rule holds exactly: one answer per user row.
    flags.switch(true, false);
    let legacy = world.thread("owner-1").await;
    world
        .post(&legacy, "owner-1", "@X @Y compare", vec![agent("agent-x"), agent("agent-y")])
        .await
        .expect("the post");
    world.reply(&legacy, "X").await.expect("one answer");
    assert!(world.reply(&legacy, "Y").await.is_err(), "switch off: no second answer");
    world.finish().await;
}

#[tokio::test]
#[ignore = "needs DB harness"]
async fn replayed_start_key_returns_already_started() {
    let _flags = WakeFlags::set(true, true);
    let Some(world) = World::new("replay").await else { return };
    let thread = world.thread("owner-1").await;
    let text = "@X @Y compare";
    let posted = world
        .post(&thread, "owner-1", text, vec![agent("agent-x"), agent("agent-y")])
        .await
        .expect("the post");
    let wake_x = world.verified(&posted.message_id, text, "agent-x");
    let key = format!("wake-{}", new_ulid());
    let started = world.start(&thread, "owner-1", &key, Some(&wake_x)).await.expect("the wake starts");
    assert!(!started.already_started);
    let (origin, trigger, agent_id): (Option<String>, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT turn_origin, trigger_message_id, agent_subject_id FROM runs WHERE id = $1",
    )
    .bind(&started.run_id)
    .fetch_one(&world.pool)
    .await
    .expect("run row");
    assert_eq!(origin.as_deref(), Some("direct"));
    assert_eq!(trigger.as_deref(), Some(posted.message_id.as_str()));
    assert_eq!(agent_id.as_deref(), Some("agent-x"));

    // The same key and the same wake: the first run, nothing new.
    let replay = world.start(&thread, "owner-1", &key, Some(&wake_x)).await.expect("replay");
    assert!(replay.already_started);
    assert_eq!(replay.run_id, started.run_id);

    // The same key for another agent, or without the wake, is a different start.
    let wake_y = world.verified(&posted.message_id, text, "agent-y");
    let other = world.start(&thread, "owner-1", &key, Some(&wake_y)).await.expect_err("another agent");
    assert_eq!(other.code(), tonic::Code::AlreadyExists);
    let bare = world.start(&thread, "owner-1", &key, None).await.expect_err("no wake");
    assert_eq!(bare.code(), tonic::Code::AlreadyExists);

    let (runs,): (i64,) = sqlx::query_as("SELECT count(*) FROM runs WHERE thread_id = $1")
        .bind(&thread)
        .fetch_one(&world.pool)
        .await
        .expect("count runs");
    assert_eq!(runs, 1);

    // An ordinary start leaves every wake column NULL.
    let plain = world.thread("owner-1").await;
    let ordinary = world
        .start(&plain, "owner-1", &format!("plain-{}", new_ulid()), None)
        .await
        .expect("an ordinary start");
    let (origin, trigger): (Option<String>, Option<String>) =
        sqlx::query_as("SELECT turn_origin, trigger_message_id FROM runs WHERE id = $1")
            .bind(&ordinary.run_id)
            .fetch_one(&world.pool)
            .await
            .expect("plain run row");
    assert_eq!((origin, trigger), (None, None));
    world.finish().await;
}

#[tokio::test]
#[ignore = "needs DB harness"]
async fn derived_wake_state_follows_run_phase() {
    let _flags = WakeFlags::set(true, true);
    let Some(world) = World::new("state").await else { return };
    let thread = world.thread("owner-1").await;
    let text = "@Ada @X @Y @Z";
    let posted = world
        .post(
            &thread,
            "owner-1",
            text,
            vec![person("user-ada"), agent("agent-x"), agent("agent-y"), refused_agent("agent-z", "invocation_blocked")],
        )
        .await
        .expect("the post");
    let started = world
        .start(&thread, "owner-1", &format!("state-{}", new_ulid()), Some(&world.verified(&posted.message_id, text, "agent-x")))
        .await
        .expect("the wake starts");

    let read = || {
        let pool = world.pool.clone();
        let org = world.org.clone();
        let thread = thread.clone();
        let message_id = posted.message_id.clone();
        async move {
            let mut page = vec![
                pb::SessionMessage {
                    message_id: message_id.clone(),
                    post_kind: "post".to_owned(),
                    ..Default::default()
                },
                pb::SessionMessage {
                    message_id: "a-turn".to_owned(),
                    post_kind: "turn".to_owned(),
                    ..Default::default()
                },
            ];
            mentions::attach_mentions(&pool, &org, &thread, &mut page).await.expect("attach");
            assert!(page[1].mentions.is_empty(), "a turn carries no mentions");
            page.swap_remove(0)
                .mentions
                .into_iter()
                .map(|m| (m.subject_id, m.wake_state, m.reason))
                .collect::<Vec<_>>()
        }
    };
    fn state(rows: &[(String, String, String)], id: &str) -> (String, String) {
        rows.iter()
            .find(|(subject, _, _)| subject == id)
            .map(|(_, s, r)| (s.clone(), r.clone()))
            .expect(id)
    }

    let rows = read().await;
    assert_eq!(rows.len(), 4);
    assert_eq!(state(&rows, "agent-x"), ("running".to_owned(), String::new()));
    assert_eq!(state(&rows, "agent-y"), ("expected".to_owned(), String::new()));
    assert_eq!(state(&rows, "agent-z"), ("refused".to_owned(), "invocation_blocked".to_owned()));
    assert_eq!(state(&rows, "user-ada"), (String::new(), String::new()));

    for (status, derived) in [("completed", "completed"), ("failed", "failed"), ("cancelled", "cancelled")] {
        sqlx::query("UPDATE runs SET status = $2 WHERE id = $1")
            .bind(&started.run_id)
            .bind(status)
            .execute(&world.pool)
            .await
            .expect("move the run");
        assert_eq!(state(&read().await, "agent-x").0, derived, "{status}");
    }
    world.finish().await;
}

#[tokio::test]
#[ignore = "needs DB harness"]
async fn message_appended_payload_has_counts_only() {
    let _flags = WakeFlags::set(true, true);
    let Some(world) = World::new("event").await else { return };
    let thread = world.thread("owner-1").await;
    let text = "secret words for @Ada and @X";
    let posted = world
        .post(&thread, "owner-1", text, vec![person("person-secret"), agent("agent-secret")])
        .await
        .expect("the post");
    let payload: Value = sqlx::query_scalar(
        "SELECT payload FROM events WHERE event_type = 'MESSAGE_APPENDED' AND payload ->> 'message_id' = $1",
    )
    .bind(&posted.message_id)
    .fetch_one(&world.pool)
    .await
    .expect("the event");
    assert_eq!(payload["mention_count"], 2);
    assert_eq!(payload["agent_mention_count"], 1);
    let serialized = payload.to_string();
    for leak in ["person-secret", "agent-secret", "secret words"] {
        assert!(!serialized.contains(leak), "{leak} reached the event: {serialized}");
    }

    // A post without mentions keeps the former payload exactly.
    let plain = world.thread("owner-1").await;
    let quiet = world.post(&plain, "owner-1", "no one", Vec::new()).await.expect("plain post");
    let payload: Value = sqlx::query_scalar(
        "SELECT payload FROM events WHERE event_type = 'MESSAGE_APPENDED' AND payload ->> 'message_id' = $1",
    )
    .bind(&quiet.message_id)
    .fetch_one(&world.pool)
    .await
    .expect("the plain event");
    assert!(payload.get("mention_count").is_none());
    assert!(payload.get("agent_mention_count").is_none());
    world.finish().await;
}
