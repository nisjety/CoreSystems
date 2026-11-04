//! WS10-S7.ctl: the `model.approval.decide` payload digest is a wire contract between Control
//! (Go, `approvalDecidePayloadDigest`) and Model Plane. This golden is the same hex that
//! `approval_decide_decision_test.go` asserts for the same inputs; the hex was produced by an
//! independent implementation of the documented byte layout. Change both together or neither.

use mp_contracts::space_approvals::{decide_payload_digest, DECIDE_SCHEMA};

const GOLDEN: &str = "sha256:686970f46c3e6ac25e6c62d8fbbcd5bc7bde610eac8e885a584495bd7d2ce025";

#[test]
fn decide_payload_digest_matches_the_control_golden() {
    let digest = decide_payload_digest(
        "org-1",
        "user-1",
        "approval-1",
        "room-1",
        "decision-1",
        "granted",
        "once",
    );
    assert_eq!(digest, GOLDEN);
    assert_eq!(DECIDE_SCHEMA, "sha256:approval-decide-v1");
}

#[test]
fn decide_payload_digest_binds_every_field() {
    let base = decide_payload_digest("o", "u", "a", "s", "d", "granted", "once");
    let variants = [
        decide_payload_digest("o2", "u", "a", "s", "d", "granted", "once"),
        decide_payload_digest("o", "u2", "a", "s", "d", "granted", "once"),
        decide_payload_digest("o", "u", "a2", "s", "d", "granted", "once"),
        decide_payload_digest("o", "u", "a", "s2", "d", "granted", "once"),
        decide_payload_digest("o", "u", "a", "s", "d2", "granted", "once"),
        decide_payload_digest("o", "u", "a", "s", "d", "denied", "once"),
        decide_payload_digest("o", "u", "a", "s", "d", "granted", "thread"),
    ];
    for (index, variant) in variants.iter().enumerate() {
        assert_ne!(&base, variant, "field {index} is not bound by the digest");
    }
}
