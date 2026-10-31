//! On-demand sync controller.
//!
//! The Connected page can trigger an incremental sync or a full backfill. The
//! BFF spawns the `xero` CLI (same image, PKCE auth via the shared env) as a
//! background child and tracks a single in-flight run's status. Only one run at
//! a time; the SPA polls `GET /api/sync/status`.

use std::sync::{Arc, Mutex};

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use axum_extra::extract::cookie::PrivateCookieJar;
use chrono::{NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use crate::{handlers::SESSION_COOKIE, AppState};

/// Sync controls require an authenticated session (a connected user).
fn require_session(jar: &PrivateCookieJar) -> Option<Response> {
    if jar.get(SESSION_COOKIE).is_none() {
        return Some((StatusCode::UNAUTHORIZED, "not authenticated").into_response());
    }
    None
}

/// Path to the sync CLI binary (overridable for local dev).
fn cli_bin() -> String {
    std::env::var("XERO_CLI_BIN").unwrap_or_else(|_| "/app/xero".to_owned())
}

#[derive(Clone, Serialize, Default)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum SyncState {
    #[default]
    Idle,
    Running {
        kind: String,
        started_at: String,
    },
    Finished {
        kind: String,
        ok: bool,
        summary: String,
        finished_at: String,
    },
}

pub type SharedSyncState = Arc<Mutex<SyncState>>;

/// Current run status (SPA polls this).
pub async fn sync_status(State(st): State<AppState>, jar: PrivateCookieJar) -> Response {
    if let Some(resp) = require_session(&jar) {
        return resp;
    }
    let s = st
        .sync_state
        .lock()
        .map(|g| g.clone())
        .unwrap_or(SyncState::Idle);
    Json(s).into_response()
}

/// Start an incremental sync (last 30 days) for the connected orgs.
pub async fn start_sync(State(st): State<AppState>, jar: PrivateCookieJar) -> Response {
    if let Some(resp) = require_session(&jar) {
        return resp;
    }
    start(
        st,
        "incremental",
        vec!["sync".into(), "--window-days".into(), "30".into()],
    )
}

/// Optional `[from, to]` date range for a backfill, from the request query
/// (`?from=YYYY-MM-DD&to=YYYY-MM-DD`). Both are optional; see [`start_backfill`]
/// for the defaults.
#[derive(Deserialize)]
pub struct BackfillParams {
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
}

/// Start a backfill over an explicit `[from, to]` range in 3-month business-date
/// chunks. With no query params it defaults to the last 3 years → today.
pub async fn start_backfill(
    State(st): State<AppState>,
    jar: PrivateCookieJar,
    Query(params): Query<BackfillParams>,
) -> Response {
    if let Some(resp) = require_session(&jar) {
        return resp;
    }
    let (from, to) = match resolve_backfill_range(params.from, params.to, Utc::now().date_naive()) {
        Ok(range) => range,
        Err(msg) => return bad_range(msg),
    };

    start(
        st,
        "backfill",
        vec![
            "sync".into(),
            "--backfill".into(),
            format!("{from}:{to}"),
            "--chunk-months".into(),
            "3".into(),
        ],
    )
}

/// Resolve the effective `[from, to]` backfill range from optional request
/// params and today's date. Defaults: `from` = 3 years ago, `to` = today; `to`
/// is clamped to today so we never request the future. Returns an error message
/// for an absurd (`from` < 2000) or inverted (`from` > `to`) range.
///
/// Pure (no I/O) so it can be unit-tested; the handler passes `Utc::now()`.
fn resolve_backfill_range(
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
    today: NaiveDate,
) -> Result<(NaiveDate, NaiveDate), &'static str> {
    // 365*3 days is within a day of exact — a historical backfill's floor does
    // not need calendar precision.
    let from = from.unwrap_or_else(|| today - chrono::Duration::days(365 * 3));
    let to = to.unwrap_or(today).min(today);
    let floor = NaiveDate::from_ymd_opt(2000, 1, 1).expect("valid literal date");
    if from < floor {
        return Err("`from` must be on or after 2000-01-01");
    }
    if from > to {
        return Err("`from` must be on or before `to`");
    }
    Ok((from, to))
}

/// 400 response for an out-of-bounds / malformed backfill range.
fn bad_range(msg: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "error": "invalid_range", "message": msg })),
    )
        .into_response()
}

fn start(st: AppState, kind: &'static str, args: Vec<String>) -> Response {
    // Reject if a run is already in flight (one at a time).
    {
        let mut guard = match st.sync_state.lock() {
            Ok(g) => g,
            Err(_) => {
                return (StatusCode::INTERNAL_SERVER_ERROR, "sync state poisoned").into_response()
            }
        };
        if let SyncState::Running { kind, started_at } = &*guard {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "error": "already_running", "kind": kind, "started_at": started_at,
                })),
            )
                .into_response();
        }
        *guard = SyncState::Running {
            kind: kind.to_owned(),
            started_at: Utc::now().to_rfc3339(),
        };
    }

    let state = st.sync_state.clone();
    let kind_s = kind.to_owned();
    tokio::spawn(async move {
        // RUST_LOG=warn keeps the child output small (warnings + the println
        // summary line); NO_COLOR strips ANSI so the summary parses cleanly.
        let output = tokio::process::Command::new(cli_bin())
            .args(&args)
            .env("RUST_LOG", "warn")
            .env("NO_COLOR", "1")
            .output()
            .await;
        let (ok, summary) = match output {
            Ok(out) => {
                let combined = format!(
                    "{}{}",
                    String::from_utf8_lossy(&out.stdout),
                    String::from_utf8_lossy(&out.stderr)
                );
                (out.status.success(), summarize(&combined))
            }
            Err(e) => (false, format!("failed to launch sync: {e}")),
        };
        if let Ok(mut guard) = state.lock() {
            *guard = SyncState::Finished {
                kind: kind_s,
                ok,
                summary,
                finished_at: Utc::now().to_rfc3339(),
            };
        }
        tracing::info!(ok, "on-demand {kind} run finished");
    });

    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({ "state": "running", "kind": kind })),
    )
        .into_response()
}

/// Pull the last meaningful summary line out of the CLI output (the trailing
/// "N entities · N pages · N records · N error(s)").
fn summarize(output: &str) -> String {
    output
        .lines()
        .map(str::trim)
        .rev()
        .find(|l| l.contains("records") || l.contains("entities"))
        .or_else(|| output.lines().map(str::trim).rev().find(|l| !l.is_empty()))
        .unwrap_or("(no output)")
        .chars()
        .take(300)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::resolve_backfill_range;
    use chrono::NaiveDate;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).expect("valid test date")
    }

    #[test]
    fn defaults_to_last_three_years() {
        let today = d(2026, 7, 16);
        let (from, to) = resolve_backfill_range(None, None, today).unwrap();
        assert_eq!(to, today);
        assert_eq!(from, today - chrono::Duration::days(365 * 3));
    }

    #[test]
    fn honours_explicit_range() {
        let today = d(2026, 7, 16);
        let range =
            resolve_backfill_range(Some(d(2020, 1, 1)), Some(d(2021, 6, 30)), today).unwrap();
        assert_eq!(range, (d(2020, 1, 1), d(2021, 6, 30)));
    }

    #[test]
    fn clamps_to_upper_bound_to_today() {
        let today = d(2026, 7, 16);
        let (_, to) =
            resolve_backfill_range(Some(d(2024, 1, 1)), Some(d(2030, 1, 1)), today).unwrap();
        assert_eq!(to, today);
    }

    #[test]
    fn rejects_inverted_range() {
        let today = d(2026, 7, 16);
        assert!(resolve_backfill_range(Some(d(2025, 1, 1)), Some(d(2024, 1, 1)), today).is_err());
    }

    #[test]
    fn rejects_absurdly_old_from() {
        let today = d(2026, 7, 16);
        assert!(resolve_backfill_range(Some(d(1990, 1, 1)), None, today).is_err());
    }
}
