//! W3-EP-3-S5 reload relays: a reopened thread shows the context chips a user turn was sent with
//! (`contextRefs`) and the deep-research plan an assistant turn ended with (`researchPlan`), read
//! from Model Gateway's flattened message metadata. Only the allowlisted shape is re-emitted; every
//! other key (gateway bookkeeping, selection ranges, plan caps) is stripped.

use serde_json::{json, Value};

use super::{canonical_messages_to_transcript, CanonicalMessagesResponse};

fn transcript_of(messages: Value) -> Vec<Value> {
    let response: CanonicalMessagesResponse =
        serde_json::from_value(json!({ "messages": messages })).expect("model-gateway shape");
    canonical_messages_to_transcript("thread-1", response.messages)
        .expect("a transcript")
        .turns
}

#[test]
fn context_refs_are_relayed_as_kind_id_label_only() {
    let turns = transcript_of(json!([{
        "message_id": "m1",
        "role": "user",
        "content": "Look at this",
        "context_refs": [
            { "kind": "document", "id": "doc-1", "label": "Plan.pdf",
              "range": { "start": 1, "end": 9 }, "org_id": "org-1", "space_decision": "tok" },
            { "kind": "artifact_selection", "id": "art-1", "label": "x".repeat(300) },
            { "kind": "secret_store", "id": "s-1", "label": "nope" },
            { "kind": "thread", "id": "", "label": "empty id" },
            { "kind": "skill", "id": "i".repeat(129), "label": "too long id" },
            { "kind": "space" },
            "not-an-object",
            { "kind": "thread", "id": "thread-2" },
        ],
    }]));
    let refs = turns[0]["contextRefs"].as_array().expect("contextRefs");
    assert_eq!(
        refs,
        &vec![
            json!({ "kind": "document", "id": "doc-1", "label": "Plan.pdf" }),
            json!({ "kind": "artifact_selection", "id": "art-1", "label": "x".repeat(120) }),
            json!({ "kind": "thread", "id": "thread-2", "label": "" }),
        ]
    );
    assert!(
        turns[0].get("context_refs").is_none(),
        "the raw metadata key never reaches the browser"
    );
}

#[test]
fn at_most_twenty_chips_are_relayed() {
    let many: Vec<Value> = (0..30)
        .map(|i| json!({ "kind": "document", "id": format!("doc-{i}"), "label": "d" }))
        .collect();
    let turns = transcript_of(json!([{ "role": "user", "content": "x", "context_refs": many }]));
    assert_eq!(turns[0]["contextRefs"].as_array().expect("refs").len(), 20);
}

#[test]
fn junk_context_refs_leave_no_key() {
    for junk in [
        json!("document:doc-1"),
        json!({ "kind": "document", "id": "doc-1" }),
        json!([]),
        json!([{ "kind": "unknown", "id": "x" }]),
        json!(null),
        json!(7),
    ] {
        let turns =
            transcript_of(json!([{ "role": "user", "content": "x", "context_refs": junk }]));
        assert!(turns[0].get("contextRefs").is_none(), "{junk}");
    }
}

#[test]
fn a_research_plan_is_relayed_as_sub_queries_and_at_most_three_questions() {
    let turns = transcript_of(json!([{
        "role": "assistant",
        "content": "Here is the plan",
        "research_plan": {
            "version": 2,
            "cap": 12,
            "sub_queries": ["a", "  ", 7, "b", "q".repeat(400)],
            "questions": ["one?", "two?", "three?", "four?"],
            "internal_notes": "never relayed",
        },
    }]));
    assert_eq!(
        turns[0]["researchPlan"],
        json!({
            "sub_queries": ["a", "b", "q".repeat(200)],
            "questions": ["one?", "two?", "three?"],
        })
    );
    assert!(turns[0].get("research_plan").is_none());
}

#[test]
fn junk_research_plans_leave_no_key() {
    for junk in [
        json!(["a", "b"]),
        json!("plan"),
        json!({ "questions": ["only questions?"] }),
        json!({ "sub_queries": [] }),
        json!({ "sub_queries": ["   ", 3] }),
        json!(null),
    ] {
        let turns =
            transcript_of(json!([{ "role": "assistant", "content": "x", "research_plan": junk }]));
        assert!(turns[0].get("researchPlan").is_none(), "{junk}");
    }
}

#[test]
fn a_turn_without_either_field_is_unchanged() {
    let turns = transcript_of(json!([{ "role": "user", "content": "plain" }]));
    assert!(turns[0].get("contextRefs").is_none());
    assert!(turns[0].get("researchPlan").is_none());
}
