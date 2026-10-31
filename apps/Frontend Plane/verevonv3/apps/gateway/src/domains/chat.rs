pub(crate) mod documents;
pub(crate) mod history;
pub(crate) mod json_handlers;
pub(crate) mod shared;
mod search;
mod streams;
pub(crate) mod support;

use axum::{
    routing::{get, post, put},
    Router,
};

use crate::{config::AppState, middleware::require_session};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
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
        // Start a new thread from a copy of this one, up to an assistant turn (build plan WS17 S6).
        .route(
            "/api/v1/chat/threads/{thread_id}/fork",
            post(history::fork_thread),
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
        // Browser-run screenshots. The id is Quarry-v2's artifact id, which the
        // Model Plane already carries on BrowserObservationReceived.
        .route(
            "/api/v1/chat/browser-artifacts/{artifact_id}",
            get(json_handlers::get_browser_artifact),
        )
        .route("/api/v1/models", get(json_handlers::list_models))
        .route(
            "/api/v1/chat/documents",
            post(documents::upload_chat_document),
        )
        .route("/api/v1/chat/documents/extract", post(documents::extract_chat_document))
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
