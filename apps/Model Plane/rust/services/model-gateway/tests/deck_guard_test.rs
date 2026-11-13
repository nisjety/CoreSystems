//! EP-5 S3 deck guard (W3-EP-5-S3-deckguard): a slide deck is written only by the deck tool, which owns
//! its manifest. `update_artifact` on a deck is refused and names the deck tool, and leaves the deck's
//! stored manifest and version as they were; on a document it revises the document exactly as before.
//! Both drive the real `dispatch_tool` against an offline `AppState`.

use model_gateway::artifacts::ArtifactKind;
use model_gateway::state::AppState;
use model_gateway::tool_loop::{dispatch_tool, ToolOutcome};
use mp_contracts::model_plane::v1::ToolCall;

fn call(name: &str, arguments: serde_json::Value) -> ToolCall {
    ToolCall {
        id: "call-1".to_owned(),
        name: name.to_owned(),
        arguments_json: arguments.to_string(),
    }
}

async fn dispatch(state: &AppState, call: &ToolCall) -> ToolOutcome {
    dispatch_tool(
        state,
        "run",
        "org",
        "user",
        "thread",
        "",
        &[],
        "",
        None,
        None,
        "",
        "",
        None,
        false,
        false,
        false,
        call,
        None,
        None,
        None,
    )
    .await
}

const MANIFEST: &str = r#"{"slides":[{"id":"s1","state":"ok"}]}"#;

fn seed_deck(state: &AppState) {
    // As the deck tool stores a deck: a version, then the text, then the kind.
    assert_eq!(state.artifact_versions.next_version("thread", "q3-deck"), 1);
    state
        .artifact_versions
        .remember("thread", "q3-deck", "Q3 deck", MANIFEST);
    state
        .artifact_versions
        .remember_kind("thread", "q3-deck", ArtifactKind::Deck);
}

#[tokio::test]
async fn update_artifact_refuses_a_deck() {
    let state = AppState::new();
    seed_deck(&state);
    let forged = r#"{"slides":[{"id":"s1","state":"ok"},{"id":"s2","state":"ok"}]}"#;
    let outcome = dispatch(
        &state,
        &call(
            "update_artifact",
            serde_json::json!({ "id": "q3-deck", "content": forged }),
        ),
    )
    .await;
    let error = outcome.error.expect("a deck rewrite is refused");
    assert!(error.contains("slide deck"), "{error}");
    assert!(
        error.contains("deck tool"),
        "the refusal names the owning tool: {error}"
    );
    assert_eq!(
        state
            .artifact_versions
            .content_of("thread", "q3-deck")
            .as_deref(),
        Some(MANIFEST),
        "the deck's manifest is unchanged"
    );
    assert_eq!(
        state.artifact_versions.current_version("thread", "q3-deck"),
        Some(1)
    );

    // A near-miss id and a title-only match resolve to the deck and are refused the same way.
    for arguments in [
        serde_json::json!({ "id": "Q3_DECK", "content": forged }),
        serde_json::json!({ "id": "other-id", "title": "Q3 deck", "content": forged }),
    ] {
        let outcome = dispatch(&state, &call("update_artifact", arguments.clone())).await;
        assert!(
            outcome
                .error
                .as_deref()
                .is_some_and(|error| error.contains("slide deck")),
            "{arguments}: {:?}",
            outcome.error
        );
    }
    assert_eq!(
        state.artifact_versions.current_version("thread", "q3-deck"),
        Some(1)
    );

    // create_artifact cannot author a deck kind, nor turn the deck into a document by its title.
    let create_deck = dispatch(
        &state,
        &call(
            "create_artifact",
            serde_json::json!({ "id": "new-deck", "kind": "deck", "title": "New", "content": forged }),
        ),
    )
    .await;
    assert!(
        create_deck.error.is_some(),
        "a deck kind is never authored directly"
    );
    let by_title = dispatch(
        &state,
        &call(
            "create_artifact",
            serde_json::json!({ "id": "x", "kind": "document", "title": "Q3 deck", "content": "text" }),
        ),
    )
    .await;
    assert!(
        by_title.error.is_some(),
        "a create resolving to the deck may not change its kind"
    );
    assert_eq!(
        state
            .artifact_versions
            .content_of("thread", "q3-deck")
            .as_deref(),
        Some(MANIFEST)
    );
    assert_eq!(
        state.artifact_versions.kind_of("thread", "q3-deck"),
        Some(ArtifactKind::Deck)
    );
}

#[tokio::test]
async fn update_artifact_on_document_unchanged() {
    let state = AppState::new();
    seed_deck(&state);
    let created = dispatch(
        &state,
        &call(
            "create_artifact",
            serde_json::json!({ "id": "notes", "kind": "document", "title": "Notes", "content": "v1" }),
        ),
    )
    .await;
    assert!(created.error.is_none(), "{:?}", created.error);
    let updated = dispatch(
        &state,
        &call(
            "update_artifact",
            serde_json::json!({ "id": "notes", "content": "v2" }),
        ),
    )
    .await;
    assert!(updated.error.is_none(), "{:?}", updated.error);
    assert_eq!(
        state.artifact_versions.current_version("thread", "notes"),
        Some(2)
    );
    assert_eq!(
        state
            .artifact_versions
            .content_of("thread", "notes")
            .as_deref(),
        Some("v2")
    );
    assert_eq!(
        state.artifact_versions.kind_of("thread", "notes"),
        Some(ArtifactKind::Document)
    );
    // An unknown id is still the existing refusal, not the deck one.
    let unknown = dispatch(
        &state,
        &call(
            "update_artifact",
            serde_json::json!({ "id": "zzz", "content": "v" }),
        ),
    )
    .await;
    let error = unknown.error.expect("unknown id is refused");
    assert!(error.contains("no artifact 'zzz'"), "{error}");
    assert!(!error.contains("slide deck"), "{error}");
}
