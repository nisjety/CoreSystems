//! Guards the WS15 S1 lifecycle frames: `retry_scheduled`, `compaction` and
//! `context_change` carry only counts and closed tokens, and the feature
//! families gate them the way the SPA expects.
//!
//! The vocabulary lives in `mp_contracts::lifecycle_notice`; these tests read
//! the same lists the producers use, so a token or key added to one side
//! without the other fails here.

use mp_contracts::lifecycle_notice::{
    is_closed_token, COMPACTION_KEYS, COMPACTION_OUTCOMES, COMPACTION_SCOPES, CONTEXT_CHANGES,
    CONTEXT_CHANGE_KEYS, RETRY_REASONS, RETRY_SCHEDULED_KEYS, RETRY_SCOPES,
};
use model_gateway::sse_events::ChatEvent;

fn retry() -> ChatEvent {
    ChatEvent::RetryScheduled {
        scope: "tool",
        tool: "knowledge_search".into(),
        attempt: 2,
        max_attempts: 3,
        wait_ms: 250,
        reason: "rate_limited",
    }
}

fn compaction() -> ChatEvent {
    ChatEvent::Compaction {
        scope: "history",
        outcome: "summarized",
        messages_compacted: Some(12),
        messages_kept: Some(6),
        pinned_kept: Some(2),
    }
}

fn context_change() -> ChatEvent {
    ChatEvent::ContextChange {
        change: "run_context_skipped",
        messages_dropped: Some(4),
        skills_truncated: Some(1),
        skills_dropped: Some(0),
        layer: Some("memory"),
        state: Some("skipped_by_policy"),
    }
}

#[test]
fn each_lifecycle_frame_has_its_wire_name_and_family() {
    assert_eq!(retry().name(), "retry_scheduled");
    assert_eq!(retry().family(), Some("steps"));
    assert_eq!(compaction().name(), "compaction");
    assert_eq!(compaction().family(), None);
    assert_eq!(context_change().name(), "context_change");
    assert_eq!(context_change().family(), None);
}

#[test]
fn a_client_that_asked_for_nothing_hears_compaction_and_context_change_only() {
    let plain: Vec<String> = Vec::new();
    assert!(compaction().should_emit(&plain));
    assert!(context_change().should_emit(&plain));
    assert!(!retry().should_emit(&plain));
    assert!(retry().should_emit(&["steps".to_owned()]));
}

#[test]
fn payload_keys_stay_inside_each_allowlist_and_admit_no_content() {
    for (event, allowed) in [
        (retry(), &RETRY_SCHEDULED_KEYS[..]),
        (compaction(), &COMPACTION_KEYS[..]),
        (context_change(), &CONTEXT_CHANGE_KEYS[..]),
    ] {
        let payload = event.payload("req-1");
        let object = payload.as_object().expect("an SSE payload is a JSON object");
        for key in object.keys() {
            assert!(
                key == "request_id" || allowed.contains(&key.as_str()),
                "{} carries {key}, which its allowlist does not name",
                event.name()
            );
        }
        assert!(!object.contains_key("summary"), "{} must never carry a summary", event.name());
    }
}

#[test]
fn every_token_a_frame_carries_is_from_its_closed_list() {
    let retry_payload = retry().payload("req-1");
    assert!(is_closed_token(&RETRY_SCOPES, retry_payload["scope"].as_str().unwrap()));
    assert!(is_closed_token(&RETRY_REASONS, retry_payload["reason"].as_str().unwrap()));

    let compaction_payload = compaction().payload("req-1");
    assert!(is_closed_token(&COMPACTION_SCOPES, compaction_payload["scope"].as_str().unwrap()));
    assert!(is_closed_token(&COMPACTION_OUTCOMES, compaction_payload["outcome"].as_str().unwrap()));

    let context_payload = context_change().payload("req-1");
    assert!(is_closed_token(&CONTEXT_CHANGES, context_payload["change"].as_str().unwrap()));
}

#[test]
fn counts_are_numbers_and_an_unmeasured_count_is_omitted_not_null() {
    let retry_payload = retry().payload("req-1");
    assert!(retry_payload["attempt"].is_u64());
    assert!(retry_payload["max_attempts"].is_u64());
    assert!(retry_payload["wait_ms"].is_u64());

    let unmeasured = ChatEvent::Compaction {
        scope: "history",
        outcome: "truncated",
        messages_compacted: None,
        messages_kept: None,
        pinned_kept: None,
    }
    .payload("req-1");
    for key in ["messages_compacted", "messages_kept", "pinned_kept"] {
        assert!(unmeasured.get(key).is_none(), "{key} must be omitted when unmeasured");
    }
}
