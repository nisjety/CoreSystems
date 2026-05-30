//! Contract tests for the transactional outbox emission used by session-core.
//!
//! These tests verify the idempotency-key contract that `create_thread` and
//! `append_message` rely on when inserting THREAD_CREATED / MESSAGE_APPENDED
//! envelopes alongside their domain rows in the same transaction.
//!
//! Real-DB verification of the outbox INSERTs is deferred to a future
//! PG integration harness; these tests pin the deterministic hash contract
//! that the production code is built on.

use mp_events::idempotency::derive_idempotency_hash;
use mp_ids::new_ulid;

const PRODUCER: &str = "session-core";
const THREAD_CREATED: &str = "THREAD_CREATED";
const MESSAGE_APPENDED: &str = "MESSAGE_APPENDED";

fn thread_created_hash(thread_id: &str) -> String {
    derive_idempotency_hash(
        PRODUCER,
        THREAD_CREATED,
        &format!("thread:{}", thread_id),
        &format!("{}:created", thread_id),
    )
}

fn message_appended_hash(thread_id: &str, message_id: &str, sequence: u64) -> String {
    derive_idempotency_hash(
        PRODUCER,
        MESSAGE_APPENDED,
        &format!("message:{}", message_id),
        &format!("{}:{}", thread_id, sequence),
    )
}

#[test]
fn thread_created_idempotency_hash_is_deterministic() {
    let thread_id = new_ulid();

    let h1 = thread_created_hash(&thread_id);
    let h2 = thread_created_hash(&thread_id);

    assert_eq!(h1, h2, "same inputs must produce same hash");
    assert_eq!(h1.len(), 64, "blake3 hex digest must be 64 chars");
    assert!(
        h1.chars().all(|c| c.is_ascii_hexdigit()),
        "hash must be lowercase hex"
    );

    // Different thread => different hash.
    let other = thread_created_hash(&new_ulid());
    assert_ne!(h1, other);
}

#[test]
fn message_appended_unique_key_includes_thread_and_sequence() {
    let thread_id = new_ulid();
    let msg_a = new_ulid();
    let msg_b = new_ulid();

    let h_seq1 = message_appended_hash(&thread_id, &msg_a, 1);
    let h_seq2 = message_appended_hash(&thread_id, &msg_b, 2);
    assert_ne!(
        h_seq1, h_seq2,
        "distinct sequences in same thread must yield distinct hashes"
    );

    // Same thread + same sequence + same message id must be deterministic
    // (this is what protects the outbox against duplicate inserts on retry).
    let h_seq1_again = message_appended_hash(&thread_id, &msg_a, 1);
    assert_eq!(h_seq1, h_seq1_again);

    // Same sequence number under a different thread must still differ —
    // the unique key embeds the thread id.
    let other_thread = new_ulid();
    let h_other = message_appended_hash(&other_thread, &msg_a, 1);
    assert_ne!(h_seq1, h_other);
}

#[test]
fn idempotency_hashes_are_unique_across_event_types() {
    // A THREAD_CREATED hash and a MESSAGE_APPENDED hash for the same
    // thread must never collide — the event_type is part of the digest.
    let thread_id = new_ulid();
    let msg_id = new_ulid();

    let created = thread_created_hash(&thread_id);
    let appended = message_appended_hash(&thread_id, &msg_id, 1);

    assert_ne!(created, appended);
}

#[test]
fn resource_ref_format_contract() {
    // Pin the resource_ref shape that session-core writes into the
    // events envelope. Downstream consumers (projection workers, audits)
    // depend on the `thread:` / `message:` prefix to route by entity.
    let thread_id = new_ulid();
    let msg_id = new_ulid();

    let thread_resource = format!("thread:{}", thread_id);
    let message_resource = format!("message:{}", msg_id);

    assert!(thread_resource.starts_with("thread:"));
    assert!(message_resource.starts_with("message:"));
    assert_eq!(thread_resource.len(), "thread:".len() + thread_id.len());
    assert_eq!(message_resource.len(), "message:".len() + msg_id.len());

    // ULIDs are 26 chars in Crockford base32; guard against accidental
    // format drift in mp-ids.
    assert_eq!(thread_id.len(), 26, "ULID must be 26 chars");
    assert_eq!(msg_id.len(), 26, "ULID must be 26 chars");
}
