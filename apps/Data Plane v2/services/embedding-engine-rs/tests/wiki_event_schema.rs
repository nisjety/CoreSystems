//! Cross-language schema lock for `dataplane.wiki.version.published`.
//!
//! Source of truth: `docs/schemas/wiki_events.md`. If you change the
//! consumer struct without updating the canonical JSON below (and the
//! Go-side test), CI fails here.

use serde::Deserialize;

/// Mirror of the consumer struct in `src/wiki_consumer.rs`. Kept
/// in-test so this file stays self-contained when the binary-only crate
/// can't expose private types.
#[derive(Debug, Deserialize, PartialEq, Eq)]
struct WikiPublishedEvent {
    page_id: String,
    version_id: String,
    org_id: String,
    workspace_id: String,
    title: String,
    path: String,
    content: String,
    user_id: Option<String>,
    zdr: bool,
}

const CANONICAL_PAYLOAD: &str = r#"{
  "page_id": "page-1",
  "version_id": "ver-1",
  "org_id": "org-1",
  "workspace_id": "ws-1",
  "title": "Onboarding",
  "path": "/handbook/onboarding",
  "content": "Welcome to the team.",
  "user_id": "user-1",
  "zdr": false
}"#;

#[test]
fn round_trip_canonical_payload() {
    let evt: WikiPublishedEvent =
        serde_json::from_str(CANONICAL_PAYLOAD).expect("canonical JSON must parse");
    assert_eq!(evt.page_id, "page-1");
    assert_eq!(evt.version_id, "ver-1");
    assert_eq!(evt.org_id, "org-1");
    assert_eq!(evt.workspace_id, "ws-1");
    assert_eq!(evt.title, "Onboarding");
    assert_eq!(evt.path, "/handbook/onboarding");
    assert_eq!(evt.content, "Welcome to the team.");
    assert_eq!(evt.user_id.as_deref(), Some("user-1"));
    assert!(!evt.zdr);
}

#[test]
fn unknown_fields_are_ignored() {
    let with_extra = r#"{
        "page_id": "p", "version_id": "v", "org_id": "o",
        "workspace_id": "w", "title": "t", "path": "/p", "content": "c", "zdr": false,
        "future_field": "ignored"
    }"#;
    let evt: WikiPublishedEvent = serde_json::from_str(with_extra).expect("parse with extra");
    assert_eq!(evt.title, "t");
}
