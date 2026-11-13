mod artifacts;
mod deck_templates;
pub(crate) mod documents;
pub(crate) mod history;
pub(crate) mod json_handlers;
mod library;
mod processes;
pub(crate) mod shared;
pub(crate) mod research_runs;
mod search;
mod snapshots;
mod streams;
pub(crate) mod support;
mod transcripts;
#[cfg(test)]
mod bff_turn_tests;
#[cfg(test)]
mod phase_switch_off_tests;

use axum::{
    extract::DefaultBodyLimit,
    routing::{delete, get, post, put},
    Router,
};

use crate::{config::AppState, middleware::require_session};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    let mut router = Router::new();
    // One addressable research run (Quarry v2 E4.6, Q4-062b): the person's read and stop. Registered
    // only while `CHAT_RESEARCH_RUNS` is on, so off both paths answer 404. Model Gateway and Session
    // Core decide that the run is the caller's own.
    if state.space_features.chat_research_runs {
        router = router
            .route(
                "/api/v1/chat/research-runs/{run_id}",
                get(research_runs::get_research_run),
            )
            .route(
                "/api/v1/chat/research-runs/{run_id}/cancel",
                post(research_runs::cancel_research_run),
            );
    }
    // WS10.S6 (GW-2): the run's durable trace, one page at a time. Read-only; Model Plane decides that
    // the run is the caller's own (a non-owner reads the same 404 as no run). Registered only while
    // `RUN_TRACE_API` is on, so off it answers 404.
    if state.space_features.run_trace_api {
        router = router.route("/api/v1/runs/{run_id}/trace", get(streams::run_trace));
    }
    router
        .route("/api/v1/chat/stream", streams::stream_route())
        .route("/api/v1/chat/invoke", post(json_handlers::invoke_chat))
        .route(
            "/api/v1/chat/threads",
            get(history::list_threads).delete(history::clear_threads),
        )
        // Full-text search over the caller's chat threads (build plan WS17 S3).
        .route("/api/v1/chat/threads/search", get(search::search_threads))
        .route(
            "/api/v1/chat/threads/{thread_id}",
            put(history::save_thread)
                .get(history::get_thread)
                .delete(history::delete_thread),
        )
        .route(
            "/api/v1/chat/threads/{thread_id}/transcript",
            get(history::get_thread_transcript),
        )
        .route(
            "/api/v1/chat/threads/{thread_id}/archive",
            post(history::archive_thread),
        )
        .route(
            "/api/v1/chat/threads/{thread_id}/unarchive",
            post(history::unarchive_thread),
        )
        // Star and read/unread state; session-core owns both (WS17-S7).
        .route(
            "/api/v1/chat/threads/{thread_id}/flags",
            put(history::update_thread_flags),
        )
        // Move a thread's active branch: a version switch, or a rewind the client confirms first (WS17 S9).
        .route(
            "/api/v1/chat/threads/{thread_id}/branch",
            post(json_handlers::branch_thread),
        )
        .route(
            "/api/v1/chat/threads/{thread_id}/rewind",
            post(json_handlers::rewind_thread),
        )
        // Undo a rewind: the leaf returns to the turn the rewind left, even past a later branch (WS17 S9).
        .route(
            "/api/v1/chat/threads/{thread_id}/undo-rewind",
            post(json_handlers::undo_rewind_thread),
        )
        // A side question about this conversation (F-t3-069): one read-only answer from Model Gateway, which keeps
        // nothing. 404 `side_chat_disabled` from Model Gateway while its switch is off.
        .route(
            "/api/v1/chat/threads/{thread_id}/side-chat",
            post(json_handlers::ask_side_chat),
        )
        // Artifact revision history and the composer's capability read (Wave 3 EP-3 S3).
        .route(
            "/api/v1/chat/threads/{thread_id}/artifacts",
            get(artifacts::list_artifact_heads),
        )
        .route(
            "/api/v1/chat/threads/{thread_id}/artifacts/{artifact_id}/revisions",
            get(artifacts::list_artifact_revisions),
        )
        // The person's edit and restore in the Output panel (EP-3 S4); the gateway refuses what it may not.
        .route(
            "/api/v1/chat/threads/{thread_id}/artifacts/{artifact_id}",
            put(artifacts::save_artifact_revision),
        )
        .route(
            "/api/v1/chat/threads/{thread_id}/artifacts/{artifact_id}/restore",
            post(artifacts::restore_artifact_revision),
        )
        .route(
            "/api/v1/chat/composer-capabilities",
            get(artifacts::get_composer_capabilities),
        )
        // Start a new thread from a copy of this one, up to an assistant turn (build plan WS17 S6).
        .route(
            "/api/v1/chat/threads/{thread_id}/fork",
            post(history::fork_thread),
        )
        // The thread's goal (Wave 3 EP-7 S1): read, set, and edit/pause/clear. 404
        // `thread_goals_disabled` from Model Gateway while its switch is off.
        .route(
            "/api/v1/chat/threads/{thread_id}/goal",
            get(history::get_thread_goal)
                .put(history::set_thread_goal)
                .patch(history::update_thread_goal),
        )
        // The owner's export of one conversation (Wave 3 EP-12 S11): Model Gateway's file, relayed
        // unchanged. 404 `thread_export_disabled` from Model Gateway while its switch is off.
        .route(
            "/api/v1/chat/threads/{thread_id}/export",
            get(history::export_thread),
        )
        // The owner's import of one export file as a new conversation (Wave 3 EP-12 S15): Model Gateway's
        // answer relayed unchanged. 404 `thread_export_disabled` from Model Gateway while its switch is off.
        .route(
            "/api/v1/chat/threads/import",
            post(history::import_thread).layer(DefaultBodyLimit::max(
                history::THREAD_IMPORT_BODY_LIMIT_BYTES,
            )),
        )
        // Governed snapshot links (W4-SH-6, F-t3-046): the owner takes, lists and revokes snapshots of a thread and
        // shares one with named members of the organisation; a granted member reads it. Model Gateway's routes,
        // relayed under the caller's own bearers. 404 `chat_snapshots_disabled` while CHAT_SNAPSHOTS_ENABLED is off.
        .route(
            "/api/v1/chat/threads/{thread_id}/snapshots",
            post(snapshots::create_snapshot).get(snapshots::list_snapshots),
        )
        .route(
            "/api/v1/chat/snapshots/{snapshot_id}",
            delete(snapshots::revoke_snapshot),
        )
        .route(
            "/api/v1/chat/snapshots/{snapshot_id}/grants",
            post(snapshots::grant_snapshot)
                .delete(snapshots::ungrant_snapshot)
                .get(snapshots::list_snapshot_grants),
        )
        .route(
            "/api/v1/chat/shared-snapshots/{snapshot_id}",
            get(snapshots::get_shared_snapshot),
        )
        // The owner's read of one background process's output in a run (W4-P3): Model Gateway's page,
        // read under the room's own sandbox read decision. 404 `process_output_disabled` while its switch is off.
        .route(
            "/api/v1/chat/runs/{run_id}/processes/{pid}/output",
            get(processes::get_run_process_output),
        )
        .route(
            "/api/v1/chat/stream/resume/{request_id}",
            get(streams::resume_stream),
        )
        .route(
            "/api/v1/chat/invocations/{request_id}/cancel",
            post(json_handlers::cancel_invocation),
        )
        // A message typed while a run is streaming. Delivered to the running
        // agent at its next tool-round boundary instead of being dropped.
        .route(
            "/api/v1/chat/invocations/{request_id}/queue",
            post(json_handlers::queue_invocation_input),
        )
        .route(
            "/api/v1/chat/threads/{thread_id}/messages",
            get(json_handlers::get_thread_messages),
        )
        .route(
            "/api/v1/chat/threads/{thread_id}/events",
            get(json_handlers::get_thread_events),
        )
        .route(
            "/api/v1/chat/threads/{thread_id}/context",
            get(json_handlers::get_thread_context),
        )
        // The files a thread keeps across runs, and the user's reset of them (WS22-S5).
        .route(
            "/api/v1/chat/threads/{thread_id}/workspace",
            get(json_handlers::get_thread_workspace).delete(json_handlers::reset_thread_workspace),
        )
        // Browser-run screenshots. The id is Quarry-v2's artifact id, which the
        // Model Plane already carries on BrowserObservationReceived.
        .route(
            "/api/v1/chat/browser-artifacts/{artifact_id}",
            get(json_handlers::get_browser_artifact),
        )
        .route("/api/v1/models", get(json_handlers::list_models))
        // The org's brand deck templates for the picker (Wave 3 EP-5 S7); the gateway answers 404
        // while its switch is off.
        .route("/api/v1/deck-templates", get(deck_templates::list_deck_templates))
        .route(
            "/api/v1/chat/documents",
            post(documents::upload_chat_document),
        )
        .route("/api/v1/chat/documents/extract", post(documents::extract_chat_document))
        // Transcribe one audio upload into a private Data Plane `media_transcript` document (W5, F-t3-090). The body
        // limit is the speech route's own. 404 `media_transcripts_disabled` while CHAT_MEDIA_TRANSCRIPTS_ENABLED is off.
        .route(
            "/api/v1/chat/transcripts",
            post(transcripts::create_chat_transcript).layer(DefaultBodyLimit::max(
                crate::domains::ai::AI_AUDIO_BODY_LIMIT_BYTES,
            )),
        )
        // The chat library (F-t3-080): saved artifacts and uploaded text, one filtered Data Plane view. 404 while
        // the artifact-promote switch is off.
        .route("/api/v1/chat/library", get(library::list_chat_library))
        .route(
            "/api/v1/chat/feedback",
            post(json_handlers::submit_feedback),
        )
        .route(
            "/api/v1/runs/{run_id}/events",
            get(streams::run_events_stream),
        )
        .route(
            "/api/v1/runs/{run_id}/events/replay",
            get(streams::run_events_replay),
        )
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

#[cfg(test)]
mod tests {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;

    /// WS10.S6 (GW-2): the run-trace route exists only while `RUN_TRACE_API` is on. Off, the path is
    /// not registered and answers 404. On, the session check answers first (no cookie is a 401), so
    /// the route is shown to exist without any upstream call.
    #[tokio::test]
    async fn the_run_trace_route_is_absent_until_run_trace_api_is_on() {
        let cases = [(false, StatusCode::NOT_FOUND), (true, StatusCode::UNAUTHORIZED)];
        for (enabled, expected) in cases {
            let mut state = crate::tests::test_state(false);
            state.space_features.run_trace_api = enabled;
            let app = super::router(state.clone()).with_state(state);
            let request = Request::builder()
                .method("GET")
                .uri("/api/v1/runs/run-1/trace")
                .body(Body::empty())
                .expect("request");
            let response = app.oneshot(request).await.expect("response");
            assert_eq!(response.status(), expected, "RUN_TRACE_API on = {enabled}");
        }
    }
}
