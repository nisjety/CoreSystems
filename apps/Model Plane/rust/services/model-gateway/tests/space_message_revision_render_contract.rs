//! WS12-S4: how a revised Space turn reaches the model's history. Session Core folds each message to
//! its effective content in `ListConversation`; `load_recent_thread_messages` then renders every turn
//! through `mp_contracts::message_revision::render_for_model`, so a deleted turn becomes one static
//! system line and an edited turn carries its text plus the marker. This is an integration test so it
//! does not depend on the crate's unit test target. The loader is private, so its wiring is pinned on
//! its source.

use mp_contracts::message_revision::{render_for_model, DELETED_MODEL_LINE, EDITED_MODEL_MARKER};

#[test]
fn history_renders_tombstone_and_edited_marker() {
    // A deleted turn is one static system line, whatever text it had.
    assert_eq!(
        render_for_model("user", "", "deleted"),
        Some(("system".to_owned(), DELETED_MODEL_LINE.to_owned()))
    );
    // An edited turn carries its effective text and the marker.
    assert_eq!(
        render_for_model("user", "the new words", "edited"),
        Some((
            "user".to_owned(),
            format!("the new words\n{EDITED_MODEL_MARKER}")
        ))
    );
    // An edited turn with no text left sends no bare marker.
    assert_eq!(render_for_model("user", "", "edited"), None);
    // An unrevised turn is unchanged, and an empty unrevised turn is left out.
    assert_eq!(
        render_for_model("assistant", "an answer", ""),
        Some(("assistant".to_owned(), "an answer".to_owned()))
    );
    assert_eq!(render_for_model("assistant", "", ""), None);
}

#[test]
fn load_recent_thread_messages_renders_every_turn_before_the_provider_sees_it() {
    let source = include_str!("../src/sse.rs");
    let start = source
        .find("async fn load_recent_thread_messages(")
        .expect("the recent-thread loader");
    let rest = &source[start..];
    let end = rest.find("\n}").expect("the loader's closing brace");
    let body = &rest[..end];
    let render = body
        .find("mp_contracts::message_revision::render_for_model(")
        .expect("the loader renders revised turns");
    assert!(
        body.contains("&message.revision_state"),
        "the render call reads the state session-core folded in"
    );
    let provider_turn = body
        .find("ChatMessage {")
        .expect("the loader builds the provider turn");
    assert!(
        render < provider_turn,
        "the render happens before the provider turn is built"
    );
}
