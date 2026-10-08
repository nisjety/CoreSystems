use std::sync::Arc;

use axum::{
    extract::State,
    http::{header, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Extension, Json, Router,
};
use nats_connection::erasure_health::ErasureReadiness;

use crate::budget::BudgetGate;
use crate::config::ContextualRetrievalState;

mod embed;

/// The admin routes. `contextual` is what the boot-time contract of contextual retrieval
/// decided (CR-T13), reported by `/readyz`; `budget` is the process's background budget gate
/// (CR-T9b), whose counter `/metrics` serves.
pub fn router(contextual: ContextualRetrievalState, budget: Arc<BudgetGate>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/readyz", get(readyz))
        .route("/metrics", get(metrics))
        .route("/v1/embed-text", post(embed::embed_text))
        .layer(Extension(budget))
        .with_state(contextual)
}

/// `GET /metrics`: the one counter this service keeps, `ctx_budget_exceeded_total` (CR-T9b), in
/// the Prometheus text format. It holds a count by budget mode and nothing of any organization:
/// the organization is in the log line that goes with each alert.
async fn metrics(Extension(budget): Extension<Arc<BudgetGate>>) -> impl IntoResponse {
    (
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        budget.render_metrics(),
    )
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok", "service": "embedding-engine-rs"}))
}

async fn readyz(
    State(contextual): State<ContextualRetrievalState>,
) -> (StatusCode, Json<serde_json::Value>) {
    // The GDPR erasure consumer is part of readiness because its failure was
    // otherwise invisible: the supervisor retries forever and only logs, so this
    // process reported ready while org erasure silently stopped being applied.
    readiness(nats_connection::erasure_health::readiness(), &contextual)
}

/// The `/readyz` answer. The incident kill switch of contextual retrieval is reported (state and
/// reason) and never makes the service not ready: it is a deliberate state in which chunks are
/// embedded raw, and failing the health check would have the platform restart a service that is
/// doing what it was told.
fn readiness(
    erasure: ErasureReadiness,
    contextual: &ContextualRetrievalState,
) -> (StatusCode, Json<serde_json::Value>) {
    let status = if erasure.is_ready() {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (
        status,
        Json(serde_json::json!({
            "status": if erasure.is_ready() { "ready" } else { "not_ready" },
            "service": "embedding-engine-rs",
            "checks": {
                "gdpr_erasure_consumer": erasure.as_str(),
                "contextual_retrieval": contextual.as_str()
            },
            "contextual_retrieval": contextual
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;

    fn on() -> ContextualRetrievalState {
        ContextualRetrievalState::On {
            model: "gpt-4o-mini".to_owned(),
        }
    }

    fn thrown() -> ContextualRetrievalState {
        ContextualRetrievalState::KillSwitch {
            reason: "provider incident 2026-10-07".to_owned(),
        }
    }

    async fn body_of(response: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("the body");
        serde_json::from_slice(&bytes).expect("json")
    }

    #[test]
    fn readiness_reports_contextual_retrieval_on_with_its_model() {
        let (status, Json(body)) = readiness(ErasureReadiness::Disabled, &on());
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "ready");
        assert_eq!(body["checks"]["contextual_retrieval"], "on");
        assert_eq!(
            body["contextual_retrieval"],
            serde_json::json!({"state": "on", "model": "gpt-4o-mini"})
        );
        // What was reported before is still there.
        assert_eq!(body["service"], "embedding-engine-rs");
        assert_eq!(body["checks"]["gdpr_erasure_consumer"], "disabled");
    }

    /// The kill switch is reported with its reason, and the service stays ready: it is a
    /// deliberate state, not a fault.
    #[test]
    fn readiness_reports_the_kill_switch_and_its_reason_and_stays_ready() {
        let (status, Json(body)) = readiness(ErasureReadiness::Connected, &thrown());
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "ready");
        assert_eq!(body["checks"]["contextual_retrieval"], "kill_switch");
        assert_eq!(
            body["contextual_retrieval"],
            serde_json::json!({"state": "kill_switch", "reason": "provider incident 2026-10-07"})
        );
    }

    /// A stalled erasure consumer still makes the service not ready, whichever state
    /// contextual retrieval is in, and the report of contextual retrieval is still there.
    #[test]
    fn a_stalled_erasure_consumer_is_still_not_ready() {
        for contextual in [on(), thrown()] {
            let (status, Json(body)) =
                readiness(ErasureReadiness::Stalled { seconds_down: 0 }, &contextual);
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
            assert_eq!(body["status"], "not_ready");
            assert_eq!(body["checks"]["gdpr_erasure_consumer"], "stalled");
            assert_eq!(body["checks"]["contextual_retrieval"], contextual.as_str());
        }
    }

    /// The handler, through the extractor the router uses, answers from the state the router was
    /// built with. (Its status also depends on the process-global erasure health, which is not
    /// this test's to set, so only the report of contextual retrieval is read.)
    #[tokio::test]
    async fn the_readyz_handler_answers_from_the_state_the_router_was_built_with() {
        let response = readyz(State(thrown())).await.into_response();
        let body = body_of(response).await;
        assert_eq!(body["contextual_retrieval"]["state"], "kill_switch");
        assert_eq!(
            body["contextual_retrieval"]["reason"],
            "provider incident 2026-10-07"
        );
    }

    /// `/metrics` is served by the router the service runs (not only by the handler): the
    /// budget counter in the Prometheus text format, a count by budget mode and nothing of any
    /// organization, next to the routes that were there. A real request over a real socket, so
    /// that the extension the handler reads is the one the router carries.
    #[tokio::test]
    async fn the_router_serves_the_budget_counter_in_the_prometheus_text_format() {
        let (gate, _spend) = crate::budget::testing::idle_gate();
        let app = router(on(), Arc::new(gate));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback");
        let address = listener.local_addr().expect("local address");
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let client = reqwest::Client::new();
        let metrics = client
            .get(format!("http://{address}/metrics"))
            .send()
            .await
            .expect("GET /metrics");
        assert_eq!(metrics.status(), 200);
        let content_type = metrics
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        assert!(
            content_type.starts_with("text/plain; version=0.0.4"),
            "{content_type}"
        );
        let text = metrics.text().await.expect("the body");
        assert!(
            text.contains("# TYPE ctx_budget_exceeded_total counter"),
            "{text}"
        );
        for mode in ["alert_only", "throttle", "stop"] {
            assert!(
                text.contains(&format!("ctx_budget_exceeded_total{{mode=\"{mode}\"}} 0")),
                "{text}"
            );
        }
        // The routes that were there still are.
        let health = client
            .get(format!("http://{address}/health"))
            .send()
            .await
            .expect("GET /health");
        assert_eq!(health.status(), 200);
        server.abort();
    }
}
