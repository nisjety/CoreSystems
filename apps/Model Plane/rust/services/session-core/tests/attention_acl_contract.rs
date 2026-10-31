//! The `RUN_ATTENTION` path against the broker's grants (build plan WS16 S3).
//!
//! The event rides the run-event subject that already exists. Session Core publishes it through the
//! outbox, capability-core's run-watch and session-review consumers read it, and the run-event stream
//! captures it. This slice changes no grant, no CHECK and no stream, so the contract pinned here is that
//! the existing grants cover the subject the event uses, and no wider subject.

const NATS_CONF: &str = include_str!("../../../../deploy/nats.conf");
const PROVISIONER: &str = include_str!("../../../../go/services/nats-provisioner/main.go");
const OUTBOX_MIGRATION: &str = include_str!("../migrations/0016_outbox_run_lifecycle_subjects.sql");
const ATTENTION_EVENTS: &str = include_str!("../src/attention_events.rs");

/// The text of one user's block in the NATS configuration: from its `user:` line to the next user's.
fn user_block<'a>(conf: &'a str, user: &str) -> &'a str {
    let marker = format!("user: \"{user}\"");
    let start = conf
        .find(&marker)
        .unwrap_or_else(|| panic!("{user} has a block in deploy/nats.conf"));
    let rest = &conf[start + marker.len()..];
    let end = rest.find("user: \"").unwrap_or(rest.len());
    &rest[..end]
}

/// The grant list of one direction (`publish` or `subscribe`) in one user's block.
fn grants<'a>(block: &'a str, direction: &str) -> &'a str {
    let marker = format!("{direction}: [");
    let start = block
        .find(&marker)
        .unwrap_or_else(|| panic!("the block has a {direction} list"));
    let rest = &block[start..];
    &rest[..rest.find(']').expect("the grant list closes")]
}

#[test]
fn session_core_publishes_run_events_on_the_single_token_run_subject() {
    let publish = grants(user_block(NATS_CONF, "session-core-runtime"), "publish");
    assert!(
        publish.contains("\"mp.v1.run.*.event\""),
        "session-core may publish RUN_ATTENTION on the run-event subject"
    );
    assert!(
        !publish.contains("mp.v1.run.>"),
        "the run grant stays on the single-token subject"
    );
}

#[test]
fn capability_core_consumers_read_the_run_event_subject() {
    let subscribe = grants(
        user_block(NATS_CONF, "capability-core-runtime"),
        "subscribe",
    );
    assert!(
        subscribe.contains("\"mp.v1.run.*.event\""),
        "capability-core's consumers of run events read the subject RUN_ATTENTION uses"
    );
}

#[test]
fn the_run_event_stream_captures_the_subject_the_event_uses() {
    let at = PROVISIONER
        .find("\"MODEL_PLANE_RUN_EVENTS\"")
        .expect("the run-event stream is provisioned");
    let stream = PROVISIONER[at..]
        .split('}')
        .next()
        .expect("the stream block");
    assert!(
        stream.contains("\"mp.v1.run.*.event\""),
        "MODEL_PLANE_RUN_EVENTS captures mp.v1.run.*.event"
    );
}

#[test]
fn the_outbox_check_still_admits_run_event_subjects() {
    assert!(
        OUTBOX_MIGRATION.contains("OR subject LIKE 'mp.v1.run.%.event'"),
        "the outbox subject CHECK admits run events, so no migration is needed"
    );
}

#[test]
fn the_event_is_published_through_the_shared_run_event_subject() {
    assert!(
        ATTENTION_EVENTS.contains(".bind(run_event_subject(&item.run_id))"),
        "RUN_ATTENTION rows take the subject the run lifecycle events take"
    );
    assert!(
        !ATTENTION_EVENTS.contains("mp.v1.attention"),
        "no subject outside the granted families"
    );
}
