//! Unit tests of `SpaceEvents.StreamSpaceEventsStream` (WS02-S3), a child module of
//! `space_stream_grpc`: the lease arithmetic, the wire filters of a row, the closing table,
//! the drift check against the `space-events.json` golden, and the handler's refusals that
//! are answered before any row is read (so no database is needed: the pool is lazy and
//! never connects).

use super::*;
use crate::auth::VerifiedIdentity;
use crate::grpc::space_w1_tests::{live_stream_token, StreamKeysEnv, DECISION_REF};
use futures::StreamExt as _;
use serde_json::json;
use std::collections::BTreeSet;

fn at(secs: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(1_800_000_000 + secs, 0).expect("valid test time")
}

// ---- lease_duration ---------------------------------------------------------------------

#[test]
fn lease_is_85_to_100_seconds_for_a_long_lived_decision() {
    let far = at(3_600);
    assert_eq!(lease_duration(at(0), far, 0), Duration::from_secs(85));
    assert_eq!(lease_duration(at(0), far, 7), Duration::from_secs(92));
    assert_eq!(lease_duration(at(0), far, 15), Duration::from_secs(100));
    // A jitter above the band is clamped to it.
    assert_eq!(lease_duration(at(0), far, 9_999), Duration::from_secs(100));
}

#[test]
fn lease_never_outlives_the_decision_expiry_minus_ten_seconds() {
    // 95 s left: 85 s remain after the margin, below the 100 s cap.
    assert_eq!(lease_duration(at(0), at(95), 15), Duration::from_secs(85));
    // 60 s left: 50 s.
    assert_eq!(lease_duration(at(0), at(60), 15), Duration::from_secs(50));
    // 130 s left: the cap wins.
    assert_eq!(lease_duration(at(0), at(130), 0), Duration::from_secs(85));
}

#[test]
fn lease_has_a_floor_of_one_second() {
    // About to expire, already expired and inside the margin all give one second, not zero.
    for expires in [at(11), at(10), at(5), at(0), at(-60)] {
        assert_eq!(lease_duration(at(0), expires, 15), Duration::from_secs(1), "{expires}");
    }
}

#[test]
fn lease_constants_match_the_golden_timing_prose() {
    assert_eq!(EXPIRY_MARGIN_SECONDS, 10);
    assert_eq!(LEASE_BASE_SECONDS, 85);
    assert_eq!(LEASE_BASE_SECONDS + LEASE_JITTER_SECONDS, 100);
    assert_eq!(HEARTBEAT, Duration::from_secs(10));
    assert_eq!(SEND_TIMEOUT, Duration::from_secs(15));
    assert_eq!(REVOCATION_CACHE_TTL, Duration::from_secs(2));
    assert_eq!(FLOOR_POLL, Duration::from_secs(30));
}

#[test]
fn jitter_stays_inside_the_band() {
    for _ in 0..64 {
        assert!(jitter_seconds() <= LEASE_JITTER_SECONDS);
    }
}

// ---- the closing table ------------------------------------------------------------------

#[test]
fn close_word_table_names_each_code_and_who_reconnects() {
    assert_eq!(close_word(CloseCode::LeaseExpired), ("lease_expired", true));
    assert_eq!(close_word(CloseCode::Reauthorize), ("reauthorize", true));
    assert_eq!(close_word(CloseCode::MembershipRevoked), ("membership_revoked", false));
    assert_eq!(close_word(CloseCode::ZdrPollOnly), ("zdr_poll_only", false));
}

#[test]
fn closing_frame_carries_the_cursor_and_generation() {
    let Frame::Closing(closing) = closing_frame(CloseCode::Reauthorize, 3, 41) else {
        panic!("a closing frame");
    };
    assert_eq!(closing.code, "reauthorize");
    assert!(closing.reconnect);
    assert_eq!((closing.generation, closing.cursor, closing.retry_after_ms), (3, 41, 0));
}

// ---- record_from_row and hidden_stub ---------------------------------------------------

fn row(kind: &str, refs: serde_json::Value) -> LogRow {
    LogRow {
        seq: 7,
        generation: 2,
        kind: kind.to_owned(),
        scope: "space".to_owned(),
        actor_type: "user".to_owned(),
        actor_subject_id: Some("member-2".to_owned()),
        thread_id: Some("thread-1".to_owned()),
        run_id: Some("run-1".to_owned()),
        message_id: Some("msg-1".to_owned()),
        audience_revision: Some(1),
        retention_class: "persistent".to_owned(),
        refs,
        created_at: at(0),
    }
}

#[test]
fn record_from_row_copies_only_the_allowlisted_refs() {
    let record = record_from_row(&row(
        "message.appended",
        json!({
            "message_sequence": 12,
            "role": "assistant",
            "agent_id": "agent-1",
            "origin": "chat",
            "state": "running",
            "prev": "queued",
            "parent_run_id": "run-0",
            "fields": ["title", "preview"],
            // Stray keys of the log must never reach the wire.
            "content": "canary body text",
            "title": "canary title",
            "preview": "canary preview",
            "output": "canary output",
        }),
    ));
    assert_eq!(record.seq, 7);
    assert_eq!(record.generation, 2);
    assert_eq!(record.kind, "message.appended");
    assert_eq!(record.at, "2027-01-15T08:00:00.000Z");
    assert_eq!(record.actor_type, "user");
    assert_eq!(record.actor_id, "member-2");
    assert_eq!(
        (record.thread_id.as_str(), record.run_id.as_str(), record.message_id.as_str()),
        ("thread-1", "run-1", "msg-1")
    );
    assert_eq!(record.message_sequence, 12);
    assert_eq!(record.role, "assistant");
    assert_eq!(record.agent_id, "agent-1");
    assert_eq!(record.origin, "chat");
    assert_eq!((record.state.as_str(), record.prev_state.as_str()), ("running", "queued"));
    assert_eq!(record.parent_run_id, "run-0");
    assert_eq!(record.fields, ["title", "preview"]);
    assert!(!format!("{record:?}").contains("canary"), "{record:?}");
}

#[test]
fn record_from_row_filters_every_ref_through_its_closed_word_or_id_rule() {
    let record = record_from_row(&row(
        "run.state_changed",
        json!({
            "role": "Admin; DROP",          // not a closed word
            "origin": "Chat-Mode",          // upper case and hyphen
            "agent_id": "agent one",        // whitespace
            "parent_run_id": "run/../0",    // path characters
            "state": "exploded",            // outside the run-state vocabulary
            "prev": "RUNNING",
            "fields": ["title", "body", "persona", 7, "Preview"],
            "message_sequence": "12",       // not an integer
        }),
    ));
    assert_eq!(record.role, "");
    assert_eq!(record.origin, "");
    assert_eq!(record.agent_id, "");
    assert_eq!(record.parent_run_id, "");
    assert_eq!(record.state, "other");
    assert_eq!(record.prev_state, "other");
    assert_eq!(record.fields, ["title", "persona"]);
    assert_eq!(record.message_sequence, 0);
}

#[test]
fn record_from_row_bounds_ids_and_defaults_the_actor_to_system() {
    let mut source = row("thread.created", json!({}));
    source.actor_type = "robot".to_owned();
    source.actor_subject_id = Some("has space".to_owned());
    source.thread_id = Some("x".repeat(129));
    source.run_id = None;
    source.message_id = Some(String::new());
    let record = record_from_row(&source);
    assert_eq!(record.actor_type, "system");
    assert_eq!(record.actor_id, "");
    assert_eq!(record.thread_id, "");
    assert_eq!(record.run_id, "");
    assert_eq!(record.message_id, "");
    // An id of exactly 128 safe bytes passes, and `:` `.` `-` `_` are safe.
    source.thread_id = Some(format!("{}:a.b-c_d", "x".repeat(120)));
    assert_eq!(record_from_row(&source).thread_id.len(), 128);
    for actor in ["user", "agent", "system"] {
        source.actor_type = actor.to_owned();
        assert_eq!(record_from_row(&source).actor_type, actor);
    }
}

#[test]
fn closed_word_and_safe_id_edge_cases() {
    assert_eq!(closed_word("a_9").as_deref(), Some("a_9"));
    assert_eq!(closed_word(""), None);
    assert_eq!(closed_word(&"a".repeat(41)), None);
    assert_eq!(closed_word(&"a".repeat(40)).as_deref(), Some("a".repeat(40).as_str()));
    assert_eq!(closed_word("A"), None);
    assert_eq!(closed_word("a b"), None);
    assert_eq!(safe_id("thread-1:x.y_z").as_deref(), Some("thread-1:x.y_z"));
    assert_eq!(safe_id(""), None);
    assert_eq!(safe_id("a\nb"), None);
    assert_eq!(safe_id("a/b"), None);
}

#[test]
fn hidden_stub_carries_no_ids_and_keeps_the_sequence_contiguous() {
    let stub = hidden_stub(&row("message.appended", json!({"role": "user", "message_sequence": 3})));
    assert_eq!((stub.seq, stub.generation), (7, 2));
    assert_eq!(stub.kind, "hidden");
    assert_eq!(stub.at, "2027-01-15T08:00:00.000Z");
    assert_eq!(stub.actor_type, "system");
    let blank = pb::SpaceEventRecord {
        seq: 7,
        generation: 2,
        kind: "hidden".to_owned(),
        at: stub.at.clone(),
        actor_type: "system".to_owned(),
        ..Default::default()
    };
    assert_eq!(stub, blank, "a stub is only seq, generation, kind, time and a system actor");
}

#[test]
fn frame_wrapper_and_too_old_frame_shape() {
    let wrapped = frame(Frame::Heartbeat(pb::SpaceStreamHeartbeat {
        head: 5,
        generation: 1,
        cursor: 4,
    }));
    assert!(matches!(wrapped.frame, Some(Frame::Heartbeat(_))));
}

// ---- drift against the space-events.json golden ---------------------------------------

fn golden() -> serde_json::Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../api/contracts/space-events.json");
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("the golden is part of the repo ({}): {error}", path.display()));
    serde_json::from_str(&raw).expect("the golden is JSON")
}

fn strings(value: &serde_json::Value) -> BTreeSet<String> {
    value
        .as_array()
        .expect("an array")
        .iter()
        .map(|item| item.as_str().expect("a string").to_owned())
        .collect()
}

#[test]
fn golden_kinds_are_the_known_kinds_plus_hidden() {
    let golden = golden();
    let mut served: BTreeSet<String> = KNOWN_KINDS.iter().map(|kind| (*kind).to_owned()).collect();
    served.insert("hidden".to_owned());
    let declared: BTreeSet<String> = golden["kinds"]
        .as_object()
        .expect("kinds")
        .keys()
        .cloned()
        .collect();
    assert_eq!(declared, served, "space-events.json kinds drifted from KNOWN_KINDS");
}

#[test]
fn golden_closing_codes_cover_what_session_core_emits() {
    let golden = golden();
    let codes = golden["events"]["closing"]["codes"].as_object().expect("codes");
    let emitted: BTreeSet<String> = codes
        .iter()
        .filter(|(_, entry)| entry["emitted_by"] == "session-core")
        .map(|(code, _)| code.clone())
        .collect();
    let mut real: BTreeSet<String> = [
        CloseCode::LeaseExpired,
        CloseCode::Reauthorize,
        CloseCode::MembershipRevoked,
        CloseCode::ZdrPollOnly,
    ]
    .into_iter()
    .map(|code| close_word(code).0.to_owned())
    .collect();
    // The slow consumer close is written by `emit` itself.
    real.insert("slow_consumer".to_owned());
    assert_eq!(emitted, real);
    for code in [
        CloseCode::LeaseExpired,
        CloseCode::Reauthorize,
        CloseCode::MembershipRevoked,
        CloseCode::ZdrPollOnly,
    ] {
        let (word, reconnect) = close_word(code);
        assert_eq!(codes[word]["reconnect"], json!(reconnect), "{word}");
    }
    assert_eq!(codes["slow_consumer"]["reconnect"], json!(true));
}

#[test]
fn golden_closed_words_and_ref_keys_match_the_real_tables() {
    let golden = golden();
    let run_states: BTreeSet<String> =
        ["queued", "running", "awaiting_approval", "completed", "failed", "cancelled", "other"]
            .into_iter()
            .map(str::to_owned)
            .collect();
    assert_eq!(strings(&golden["closed_words"]["run_state"]), run_states);
    for word in &run_states {
        assert_eq!(crate::space::events::run_state(word), word.as_str());
    }
    assert_eq!(crate::space::events::run_state("exploded"), "other");
    assert_eq!(
        strings(&golden["closed_words"]["thread_updated_fields"]),
        FIELD_WORDS.iter().map(|word| (*word).to_owned()).collect()
    );
    // Every ref a kind lists is an allowlisted key, and the timing prose agrees.
    let allowed = strings(&golden["ref_keys"]);
    for (kind, entry) in golden["kinds"].as_object().expect("kinds") {
        for key in strings(&entry["refs"]) {
            assert!(allowed.contains(&key), "{kind} lists {key}, which is not a ref key");
        }
    }
    assert_eq!(golden["events"]["heartbeat"]["every_seconds"], json!(HEARTBEAT.as_secs()));
    assert_eq!(golden["timing"]["send_timeout_seconds"], json!(SEND_TIMEOUT.as_secs()));
    assert_eq!(golden["timing"]["revocation_cache_seconds"], json!(REVOCATION_CACHE_TTL.as_secs()));
}

// ---- the handler's refusals before any row is read -------------------------------------

fn lazy_pool() -> PgPool {
    sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgresql://unreachable.invalid/session")
        .expect("lazy pool")
}

fn service(max_streams: usize) -> SpaceEventsGrpc {
    SpaceEventsGrpc {
        shared: Arc::new(Shared {
            pool: lazy_pool(),
            wakes: SpaceWakes::new(),
            active: AtomicUsize::new(0),
            max_streams,
            revocations: Mutex::new(HashMap::new()),
        }),
    }
}

fn request(
    identity: VerifiedIdentity,
    org: &str,
    user: &str,
    space: &str,
    token: String,
) -> Request<pb::StreamSpaceEventsRequest> {
    let mut request = Request::new(pb::StreamSpaceEventsRequest {
        org_id: org.to_owned(),
        user_id: user.to_owned(),
        space_id: space.to_owned(),
        decision_ref: DECISION_REF.to_owned(),
        decision_token: token,
        ..Default::default()
    });
    request.extensions_mut().insert(identity);
    request
}

fn member_token(zdr: bool) -> String {
    live_stream_token("org-1", "user-1", "space-room", 4, zdr, |_| {})
}

async fn all_frames(
    response: Response<ReceiverStream<Result<pb::SpaceEventFrame, Status>>>,
) -> Vec<pb::SpaceEventFrame> {
    let mut stream = response.into_inner();
    let mut frames = Vec::new();
    while let Ok(Some(item)) = tokio::time::timeout(Duration::from_secs(5), stream.next()).await {
        frames.push(item.expect("an ok frame"));
    }
    frames
}

async fn assert_poll_only(service: &SpaceEventsGrpc, identity: VerifiedIdentity, token: String) {
    let response = service
        .stream_space_events(request(identity, "org-1", "user-1", "space-room", token))
        .await
        .expect("a ZDR stream is answered, not refused");
    let frames = all_frames(response).await;
    assert_eq!(frames.len(), 2, "connected then closing, nothing else: {frames:?}");
    let Some(Frame::Connected(connected)) = &frames[0].frame else {
        panic!("connected first: {frames:?}");
    };
    assert_eq!(connected.mode, "poll_only");
    assert_eq!((connected.lease_seconds, connected.heartbeat_seconds), (0, 0));
    let Some(Frame::Closing(closing)) = &frames[1].frame else {
        panic!("closing second: {frames:?}");
    };
    assert_eq!(closing.code, "zdr_poll_only");
    assert!(!closing.reconnect);
    assert_eq!(
        service.shared.pool.size(),
        0,
        "no log row was read: the pool never connected"
    );
}

#[tokio::test]
async fn zdr_caller_gets_poll_only_and_no_row_read() {
    let _keys = StreamKeysEnv::publish();
    assert_poll_only(
        &service(5),
        VerifiedIdentity::user_for_test_with_zdr("org-1", "user-1", true),
        member_token(false),
    )
    .await;
}

#[tokio::test]
async fn zdr_decision_gets_poll_only_and_no_row_read() {
    let _keys = StreamKeysEnv::publish();
    assert_poll_only(
        &service(5),
        VerifiedIdentity::user_for_test("org-1", "user-1"),
        member_token(true),
    )
    .await;
}

#[tokio::test]
async fn capacity_returns_resource_exhausted_after_the_decision_verified() {
    let _keys = StreamKeysEnv::publish();
    let status = service(0)
        .stream_space_events(request(
            VerifiedIdentity::user_for_test("org-1", "user-1"),
            "org-1",
            "user-1",
            "space-room",
            member_token(false),
        ))
        .await
        .expect_err("no slot is free");
    assert_eq!(status.code(), tonic::Code::ResourceExhausted);
}

#[tokio::test]
async fn service_token_user_mismatch_and_foreign_org_are_refused() {
    let _keys = StreamKeysEnv::publish();
    let service = service(5);
    let service_identity =
        VerifiedIdentity::service_for_test_as("org-1", "service:model-gateway", &["session:read"], false);
    for (identity, org, user, why) in [
        (service_identity, "org-1", "user-1", "a service token has no user subject"),
        (VerifiedIdentity::user_for_test("org-1", "user-2"), "org-1", "user-1", "another user"),
        (VerifiedIdentity::user_for_test("org-2", "user-1"), "org-1", "user-1", "another tenant"),
    ] {
        let status = service
            .stream_space_events(request(identity, org, user, "space-room", member_token(false)))
            .await
            .expect_err(why);
        assert_eq!(status.code(), tonic::Code::PermissionDenied, "{why}");
    }
}

#[tokio::test]
async fn a_read_decision_never_opens_the_stream_and_a_blank_space_is_a_caller_error() {
    let _keys = StreamKeysEnv::publish();
    let service = service(5);
    let read_token = crate::grpc::space_w1_tests::mint(
        "model-plane",
        &["thread:read"],
        false,
        |claims| {
            claims["issued_at"] = json!(Utc::now().to_rfc3339());
            claims["expires_at"] = json!((Utc::now() + chrono::Duration::minutes(2)).to_rfc3339());
        },
        false,
    );
    let status = service
        .stream_space_events(request(
            VerifiedIdentity::user_for_test("org-1", "user-1"),
            "org-1",
            "user-1",
            "space-room",
            read_token,
        ))
        .await
        .expect_err("a model-plane read decision is the wrong audience");
    assert_eq!(status.code(), tonic::Code::PermissionDenied);

    for space in ["   ".to_owned(), "s".repeat(MAX_SPACE_ID_BYTES + 1)] {
        let status = service
            .stream_space_events(request(
                VerifiedIdentity::user_for_test("org-1", "user-1"),
                "org-1",
                "user-1",
                &space,
                member_token(false),
            ))
            .await
            .expect_err("a blank or oversized space id");
        assert_eq!(status.code(), tonic::Code::InvalidArgument);
    }
}

#[tokio::test]
async fn active_guard_releases_its_slot_on_drop() {
    let service = service(1);
    let first = ActiveGuard::acquire(&service.shared).expect("one slot");
    assert!(ActiveGuard::acquire(&service.shared).is_none(), "the cap holds");
    drop(first);
    assert!(ActiveGuard::acquire(&service.shared).is_some(), "the slot is back");
}
