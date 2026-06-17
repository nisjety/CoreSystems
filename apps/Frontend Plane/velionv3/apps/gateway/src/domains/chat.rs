mod documents;
mod json_handlers;
pub(crate) mod shared;
mod streams;

use axum::{
    routing::{get, post},
    Router,
};

use crate::{config::AppState, middleware::require_session};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/chat/stream", post(streams::stream_chat))
        .route("/api/v1/chat/invoke", post(json_handlers::invoke_chat))
        .route(
            "/api/v1/chat/stream/resume/:request_id",
            get(streams::resume_stream),
        )
        .route(
            "/api/v1/chat/invocations/:request_id/cancel",
            post(json_handlers::cancel_invocation),
        )
        .route(
            "/api/v1/chat/threads/:thread_id/messages",
            get(json_handlers::get_thread_messages),
        )
        .route("/api/v1/models", get(json_handlers::list_models))
        .route(
            "/api/v1/chat/documents",
            post(documents::upload_chat_document),
        )
        .route(
            "/api/v1/chat/feedback",
            post(json_handlers::submit_feedback),
        )
        .route(
            "/api/v1/runs/:run_id/events",
            get(streams::run_events_stream),
        )
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}
