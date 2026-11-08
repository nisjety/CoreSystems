//! Per-request context that the runtime can read without a parameter.
//!
//! The edge's request-id layer (`quarry-edge/src/request_id.rs`) runs each
//! request inside [`scope`], so code below the handler (for example the event
//! sink, WS10.S3) can stamp the id on what it emits. The id is a correlation
//! key for support lookups only. It is never an authorization, tenancy or
//! idempotency key.
//!
//! The task-local does not follow `tokio::spawn`. Code that runs in a spawned
//! task sees `None` from [`current_opt`] and must not assume an id exists.

use std::future::Future;

tokio::task_local! {
    static REQUEST_ID: String;
}

/// Runs `fut` with `id` as the current request id.
pub async fn scope<F: Future>(id: String, fut: F) -> F::Output {
    REQUEST_ID.scope(id, fut).await
}

/// The request id of the enclosing [`scope`], or `None` outside one.
pub fn current_opt() -> Option<String> {
    REQUEST_ID.try_with(|id| id.clone()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn there_is_no_request_id_outside_a_scope() {
        assert_eq!(current_opt(), None);
    }

    #[tokio::test]
    async fn the_scope_exposes_its_id_to_the_future_it_wraps() {
        let seen = scope("req_scoped-1".to_owned(), async { current_opt() }).await;
        assert_eq!(seen.as_deref(), Some("req_scoped-1"));
        assert_eq!(current_opt(), None);
    }
}
