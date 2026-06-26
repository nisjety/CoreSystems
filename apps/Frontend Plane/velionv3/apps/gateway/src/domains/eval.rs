//! Continuous-evaluation / quality read surface (Phase 7 B6).
//!
//! Computes an Ops/Quality rollup — accuracy (run success rate) and drift
//! (recent-vs-prior trend) — from session-core's `RunService` run history. The
//! browser never picks the scope: the org/user is derived from the validated
//! session, and model-gateway re-derives it from the model-plane token's claims.
//!
//! RunService is thread-scoped (`ListRuns` requires a `thread_id`), so this fans
//! out over the caller's recent threads and aggregates their runs. Every figure
//! is real run metadata (status / tokens / timestamps) — an org with no runs
//! yields an honest-empty rollup (null accuracy, zero counts), never a fabricated
//! quality number.

use axum::{
    extract::{Extension, State},
    http::HeaderMap,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use futures_util::future::join_all;
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    config::AppState,
    domains::chat::shared::{model_token, proxy_model_json},
    middleware::{require_session, AuthenticatedUser},
    rate_limit::rate_limit_middleware,
    upstream::authorized_org_id,
};

/// Cap the fan-out so the rollup stays bounded and fast.
const MAX_THREADS: u32 = 25;
const MAX_RUNS_PER_THREAD: u32 = 50;

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/eval/quality", get(quality))
        .route_layer(axum::middleware::from_fn(rate_limit_middleware))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

fn ratio(num: usize, den: usize) -> Option<f64> {
    if den == 0 {
        return None;
    }
    let n = f64::from(u32::try_from(num).unwrap_or(u32::MAX));
    let d = f64::from(u32::try_from(den).unwrap_or(u32::MAX));
    Some(n / d)
}

fn avg(sum: u64, count: usize) -> f64 {
    if count == 0 {
        return 0.0;
    }
    // Sums/counts here are small (≤ 25×50 runs); the f64 path is lossless enough
    // for an average and avoids a precision-loss lint on a raw cast.
    let total = u32::try_from(sum.min(u64::from(u32::MAX))).unwrap_or(u32::MAX);
    let c = u32::try_from(count).unwrap_or(u32::MAX);
    f64::from(total) / f64::from(c)
}

/// One run reduced to the fields the drift calc needs (status outcome + time).
struct RunRow {
    created_at: String,
    completed: bool,
    terminal: bool,
}

fn classify(run: &Value) -> Option<RunRow> {
    let status = run.get("status").and_then(Value::as_str).unwrap_or_default().to_lowercase();
    if status.is_empty() {
        return None;
    }
    let terminal = matches!(status.as_str(), "completed" | "failed" | "cancelled");
    Some(RunRow {
        created_at: run.get("created_at").and_then(Value::as_str).unwrap_or_default().to_owned(),
        completed: status == "completed",
        terminal,
    })
}

/// Accuracy = completed / terminal over a slice of rows.
fn slice_accuracy(rows: &[&RunRow]) -> Option<f64> {
    let terminal = rows.iter().filter(|r| r.terminal).count();
    let completed = rows.iter().filter(|r| r.completed).count();
    ratio(completed, terminal)
}

async fn quality(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let _org_id = authorized_org_id(&state, &user).await;
    let token = model_token(&state, &user, &headers).await;

    // 1) List the caller's recent threads (model-gateway derives scope from the token).
    let threads_url = format!("{}/v1/threads?limit={}", state.model_gateway_url, MAX_THREADS);
    let (status, Json(threads_body)) =
        proxy_model_json(&state, Method::GET, &threads_url, None, token.as_deref(), &user).await;
    if !status.is_success() {
        return (status, Json(threads_body)).into_response();
    }
    let thread_ids: Vec<String> = threads_body
        .get("threads")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|t| t.get("thread_id").and_then(Value::as_str))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();

    if thread_ids.is_empty() {
        return Json(json!({ "data": empty_rollup(), "error": null })).into_response();
    }

    // 2) Fan out RunService listings per thread, in parallel.
    let fetches = thread_ids.iter().map(|tid| {
        let url = format!(
            "{}/v1/runs?thread_id={}&limit={}",
            state.model_gateway_url,
            urlencoding::encode(tid),
            MAX_RUNS_PER_THREAD
        );
        let state = &state;
        let user = &user;
        let token = token.clone();
        async move {
            let (st, Json(body)) =
                proxy_model_json(state, Method::GET, &url, None, token.as_deref(), user).await;
            if st.is_success() {
                body.get("runs").and_then(Value::as_array).cloned().unwrap_or_default()
            } else {
                Vec::new()
            }
        }
    });
    let per_thread: Vec<Vec<Value>> = join_all(fetches).await;
    let runs: Vec<Value> = per_thread.into_iter().flatten().collect();

    // 3) Aggregate.
    let mut completed = 0usize;
    let mut failed = 0usize;
    let mut cancelled = 0usize;
    let mut running = 0usize;
    let mut other = 0usize;
    let mut input_sum = 0u64;
    let mut output_sum = 0u64;
    let mut rows: Vec<RunRow> = Vec::with_capacity(runs.len());

    for run in &runs {
        let status = run.get("status").and_then(Value::as_str).unwrap_or_default().to_lowercase();
        match status.as_str() {
            "completed" => completed += 1,
            "failed" => failed += 1,
            "cancelled" => cancelled += 1,
            "running" | "queued" => running += 1,
            _ => other += 1,
        }
        input_sum += run.get("input_tokens").and_then(Value::as_u64).unwrap_or(0);
        output_sum += run.get("output_tokens").and_then(Value::as_u64).unwrap_or(0);
        if let Some(row) = classify(run) {
            rows.push(row);
        }
    }

    let total = runs.len();
    let terminal = completed + failed + cancelled;

    // Drift: order terminal runs by time, split into prior vs recent halves.
    let mut terminal_rows: Vec<&RunRow> = rows.iter().filter(|r| r.terminal).collect();
    terminal_rows.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    let mid = terminal_rows.len() / 2;
    let (prior_acc, recent_acc) = if terminal_rows.len() >= 4 {
        (
            slice_accuracy(&terminal_rows[..mid]),
            slice_accuracy(&terminal_rows[mid..]),
        )
    } else {
        (None, None)
    };
    let accuracy_delta = match (recent_acc, prior_acc) {
        (Some(r), Some(p)) => Some(r - p),
        _ => None,
    };

    let data = json!({
        "accuracy": ratio(completed, terminal),
        "totalRuns": total,
        "terminalRuns": terminal,
        "byStatus": {
            "completed": completed,
            "failed": failed,
            "cancelled": cancelled,
            "running": running,
            "other": other,
        },
        "avgInputTokens": avg(input_sum, total),
        "avgOutputTokens": avg(output_sum, total),
        "threadsSampled": thread_ids.len(),
        "drift": {
            "recentAccuracy": recent_acc,
            "priorAccuracy": prior_acc,
            "accuracyDelta": accuracy_delta,
            "window": "recent vs prior half of terminal runs",
        },
    });
    Json(json!({ "data": data, "error": null })).into_response()
}

/// Honest-empty rollup: no runs recorded for this scope.
fn empty_rollup() -> Value {
    json!({
        "accuracy": Value::Null,
        "totalRuns": 0,
        "terminalRuns": 0,
        "byStatus": { "completed": 0, "failed": 0, "cancelled": 0, "running": 0, "other": 0 },
        "avgInputTokens": 0.0,
        "avgOutputTokens": 0.0,
        "threadsSampled": 0,
        "drift": { "recentAccuracy": Value::Null, "priorAccuracy": Value::Null, "accuracyDelta": Value::Null, "window": "recent vs prior half of terminal runs" },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(status: &str, created_at: &str, inp: u64, out: u64) -> Value {
        json!({ "status": status, "created_at": created_at, "input_tokens": inp, "output_tokens": out })
    }

    #[test]
    fn classify_marks_terminal_and_completed() {
        let r = classify(&run("completed", "2026-06-26T10:00:00Z", 10, 5)).unwrap();
        assert!(r.terminal && r.completed);
        let f = classify(&run("failed", "2026-06-26T10:00:00Z", 1, 1)).unwrap();
        assert!(f.terminal && !f.completed);
        let run_active = classify(&run("running", "2026-06-26T10:00:00Z", 0, 0)).unwrap();
        assert!(!run_active.terminal);
    }

    #[test]
    fn slice_accuracy_is_completed_over_terminal() {
        let rows = [
            classify(&run("completed", "a", 0, 0)).unwrap(),
            classify(&run("failed", "b", 0, 0)).unwrap(),
            classify(&run("running", "c", 0, 0)).unwrap(),
        ];
        let refs: Vec<&RunRow> = rows.iter().collect();
        // 1 completed / 2 terminal = 0.5 (running excluded).
        assert_eq!(slice_accuracy(&refs), Some(0.5));
    }

    #[test]
    fn avg_handles_empty() {
        assert_eq!(avg(0, 0), 0.0);
        assert_eq!(avg(100, 4), 25.0);
    }
}
