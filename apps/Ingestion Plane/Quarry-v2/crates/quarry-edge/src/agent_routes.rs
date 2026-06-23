//! P7 · Phase B — agentic browser endpoint.
//!
//! Exposes Quarry's existing per-action browser machinery (`ObservationRunner`
//! + a real `BrowserDriver`) to Model Plane, which owns the planning loop:
//!
//!   POST   /v1/agent/runs             → acquire a leased browser session
//!   POST   /v1/agent/runs/{run_id}/step → execute ONE `AgentAction`, return observation
//!   DELETE /v1/agent/runs/{run_id}    → release the session
//!
//! A live `BrowserSession` is held per run in an in-process map keyed by run_id.
//! Each run's entry is behind a `tokio::Mutex` so concurrent `/step` calls for
//! the same run serialize (a browser tab can't do two actions at once) while
//! different runs proceed in parallel. Tenant isolation: the verified JWT
//! `Claims.org_id` owns the run; cross-org access is refused.
//!
//! The whole feature is gated behind `browser-agent` (on by default), which
//! pulls in the real chromiumoxide CDP driver via `quarry-browser/chromiumoxide`.

#[cfg(feature = "browser-agent")]
pub use enabled::*;

#[cfg(feature = "browser-agent")]
mod enabled {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex as StdMutex};

    use axum::extract::{Path, State};
    use axum::http::StatusCode;
    use axum::routing::{delete, post};
    use axum::{Extension, Json, Router};
    use serde::{Deserialize, Serialize};
    use tokio::sync::Mutex as TokioMutex;

    use quarry_browser::BrowserSession;
    use quarry_core::contracts::{
        AgentAction, AgentActionRequest, AgentConstraints, BrowserObservation,
    };
    use quarry_core::envelope::Envelope;
    use quarry_core::error::{ErrorCode, QuarryError};
    use quarry_core::event::EventType;
    use quarry_core::ids::kinds::{LeaseKind, ProfileKind, RequestKind, RunKind};
    use quarry_core::ids::Id;
    use quarry_core::lease::{BrowserLease, BrowserViewport, Capability, ProxyAffinity};
    use quarry_core::zdr::ZdrMode;
    use quarry_runtime::observation::{ObservationContext, ObservationRunner};

    use crate::state::AppState;

    /// A live agent run: the leased browser session plus the per-run observation
    /// context (carries `step`/current_url across `/step` calls) and budget.
    pub struct RunEntry {
        pub run_id: RunKind,
        pub org_id: String,
        pub session: BrowserSession,
        pub ctx: ObservationContext,
        pub lease: BrowserLease,
        pub constraints: AgentConstraints,
        pub zdr: ZdrMode,
    }

    /// run_id → entry. Outer std-Mutex guards the map (held only for the O(1)
    /// get/insert/remove, never across an await); inner tokio-Mutex serializes
    /// steps within a single run across await points.
    pub type AgentRuns = Arc<StdMutex<HashMap<String, Arc<TokioMutex<RunEntry>>>>>;

    #[must_use]
    pub fn new_runs() -> AgentRuns {
        Arc::new(StdMutex::new(HashMap::new()))
    }

    #[derive(Debug, Deserialize)]
    pub struct StartRunBody {
        // quarry-core `AgentConstraints` has no `Default` impl, so supply one
        // via a serde default fn (unbounded budget when the caller omits it).
        #[serde(default = "default_constraints")]
        pub constraints: AgentConstraints,
        #[serde(default)]
        pub profile_id: Option<String>,
        #[serde(default)]
        pub persist_profile: bool,
        #[serde(default)]
        pub viewport: Option<BrowserViewport>,
        #[serde(default)]
        pub zdr: bool,
    }

    fn default_constraints() -> AgentConstraints {
        AgentConstraints {
            max_steps: 0,
            allowed_domains: Vec::new(),
            max_runtime_s: None,
            max_cost_usd: None,
        }
    }

    #[derive(Debug, Serialize)]
    pub struct StartRunData {
        pub run_id: String,
        pub lease_id: String,
        pub profile_id: String,
    }

    #[derive(Debug, Deserialize)]
    pub struct StepBody {
        pub action: AgentAction,
        #[serde(default)]
        pub instruction: Option<String>,
    }

    type ApiErr = (StatusCode, Json<Envelope<()>>);

    fn status_err(status: StatusCode, request_id: &str, msg: &str) -> ApiErr {
        (
            status,
            Json(Envelope::<()>::err(
                request_id,
                QuarryError::new(ErrorCode::BadRequest, msg),
            )),
        )
    }

    fn driver_err(request_id: &str, err: QuarryError) -> ApiErr {
        (
            StatusCode::from_u16(err.code.http_status())
                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            Json(Envelope::<()>::err(request_id, err)),
        )
    }

    /// `POST /v1/agent/runs` — acquire a leased browser session for a new run.
    pub async fn start_run(
        State(state): State<AppState>,
        Extension(claims): Extension<crate::auth::Claims>,
        Json(body): Json<StartRunBody>,
    ) -> Result<Json<Envelope<StartRunData>>, ApiErr> {
        let request_id = RequestKind::new().to_string();
        let org_id = claims.org_id.clone();

        let run_id: RunKind = Id::new();
        let lease_id: LeaseKind = Id::new();
        // Reuse a caller-supplied profile (cookie/session continuity across
        // runs) when provided & parseable; otherwise mint a fresh one.
        let profile_id: ProfileKind = body
            .profile_id
            .as_deref()
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(Id::new);
        let zdr = ZdrMode::from(body.zdr);
        let ttl_s = body.constraints.max_runtime_s.unwrap_or(120);
        let viewport = normalize_viewport(body.viewport)
            .map_err(|msg| status_err(StatusCode::BAD_REQUEST, &request_id, msg.as_str()))?;

        let persist_profile = body.persist_profile || body.profile_id.is_some();
        let lease = BrowserLease {
            lease_id: lease_id.clone(),
            profile_id,
            session_affinity_key: run_id.to_string(),
            proxy_affinity: ProxyAffinity {
                pool: String::new(),
                sticky_key: None,
            },
            ttl_s,
            capabilities: vec![Capability::Actions, Capability::Js, Capability::Screenshots],
            artifact_bucket: String::new(),
            persist_profile,
            viewport,
            org_id: org_id.clone(),
        };
        let profile_id = lease.profile_id.to_string();

        let session = state
            .agent_driver
            .acquire(&lease)
            .await
            .map_err(|e| driver_err(&request_id, e))?;

        let entry = RunEntry {
            run_id: run_id.clone(),
            org_id: org_id.clone(),
            session,
            ctx: ObservationContext {
                step: 0,
                current_url: String::new(),
                page_hash: String::new(),
            },
            lease,
            constraints: body.constraints,
            zdr,
        };

        state
            .agent_runs
            .lock()
            .expect("agent_runs mutex poisoned")
            .insert(run_id.to_string(), Arc::new(TokioMutex::new(entry)));

        state
            .event_sink
            .emit(
                run_id.clone(),
                EventType::AgentStarted,
                serde_json::json!({ "org_id": org_id }),
                format!("{run_id}:agent_started"),
            )
            .await;

        Ok(Json(Envelope::ok(
            request_id,
            StartRunData {
                run_id: run_id.to_string(),
                lease_id: lease_id.to_string(),
                profile_id,
            },
        )))
    }

    fn normalize_viewport(
        viewport: Option<BrowserViewport>,
    ) -> Result<Option<BrowserViewport>, String> {
        let Some(viewport) = viewport else {
            return Ok(None);
        };
        if !(320..=3840).contains(&viewport.width) || !(240..=2160).contains(&viewport.height) {
            return Err("viewport must be between 320x240 and 3840x2160".to_owned());
        }
        let device_scale_factor = if viewport.device_scale_factor > 0.0 {
            viewport.device_scale_factor.clamp(0.5, 4.0)
        } else {
            1.0
        };
        Ok(Some(BrowserViewport {
            device_scale_factor,
            ..viewport
        }))
    }

    // reason: this test module is intentionally kept adjacent to `normalize_viewport`;
    // the sibling route handlers (`step`, `close_run`, `agent_router`) follow it within
    // the same `mod enabled`, so relocating the tests to the module end is not desirable.
    #[allow(clippy::items_after_test_module)]
    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn normalize_viewport_accepts_reasonable_desktop_size() {
            let viewport = normalize_viewport(Some(BrowserViewport {
                width: 1280,
                height: 800,
                device_scale_factor: 0.0,
                is_mobile: false,
            }))
            .expect("valid viewport")
            .expect("viewport");

            assert_eq!(viewport.width, 1280);
            assert_eq!(viewport.height, 800);
            assert_eq!(viewport.device_scale_factor, 1.0);
            assert!(!viewport.is_mobile);
        }

        #[test]
        fn normalize_viewport_rejects_unbounded_sizes() {
            let err = normalize_viewport(Some(BrowserViewport {
                width: 10_000,
                height: 800,
                device_scale_factor: 1.0,
                is_mobile: false,
            }))
            .expect_err("oversized viewport should be rejected");

            assert!(err.contains("viewport"));
        }
    }

    /// `POST /v1/agent/runs/{run_id}/step` — execute one action, return the
    /// resulting observation. `ObservationRunner` advances `ctx.step` and emits
    /// `ActionStarted`/`ObservationReady` itself.
    pub async fn step(
        State(state): State<AppState>,
        Extension(claims): Extension<crate::auth::Claims>,
        Path(run_id): Path<String>,
        Json(body): Json<StepBody>,
    ) -> Result<Json<Envelope<BrowserObservation>>, ApiErr> {
        let request_id = RequestKind::new().to_string();

        let entry_arc = {
            let map = state.agent_runs.lock().expect("agent_runs mutex poisoned");
            map.get(&run_id).cloned()
        };
        let Some(entry_arc) = entry_arc else {
            return Err(status_err(
                StatusCode::NOT_FOUND,
                &request_id,
                "agent run not found",
            ));
        };

        let mut entry = entry_arc.lock().await;
        if entry.org_id != claims.org_id {
            return Err(status_err(
                StatusCode::FORBIDDEN,
                &request_id,
                "run belongs to another org",
            ));
        }

        let req = AgentActionRequest {
            run_id: entry.run_id.clone(),
            lease_id: entry.lease.lease_id.clone(),
            action: body.action,
            instruction: body.instruction,
            constraints: entry.constraints.clone(),
            zdr: entry.zdr,
        };

        let runner = ObservationRunner {
            browser: state.agent_driver.clone(),
            artifacts: Some(state.artifacts.clone()),
            events: Some(state.event_sink.clone()),
        };

        // Disjoint borrows: &session (shared) + &mut ctx (exclusive).
        let entry_mut: &mut RunEntry = &mut entry;
        let obs = runner
            .execute(&req, &entry_mut.session, &mut entry_mut.ctx)
            .await
            .map_err(|e| driver_err(&request_id, e))?;

        Ok(Json(Envelope::ok(request_id, obs)))
    }

    /// `DELETE /v1/agent/runs/{run_id}` — release the leased session.
    pub async fn close_run(
        State(state): State<AppState>,
        Extension(claims): Extension<crate::auth::Claims>,
        Path(run_id): Path<String>,
    ) -> Result<Json<Envelope<serde_json::Value>>, ApiErr> {
        let request_id = RequestKind::new().to_string();

        let entry_arc = {
            let map = state.agent_runs.lock().expect("agent_runs mutex poisoned");
            map.get(&run_id).cloned()
        };
        let Some(entry_arc) = entry_arc else {
            return Err(status_err(
                StatusCode::NOT_FOUND,
                &request_id,
                "agent run not found",
            ));
        };

        // Org check before mutating shared state.
        {
            let entry = entry_arc.lock().await;
            if entry.org_id != claims.org_id {
                return Err(status_err(
                    StatusCode::FORBIDDEN,
                    &request_id,
                    "run belongs to another org",
                ));
            }
        }

        state
            .agent_runs
            .lock()
            .expect("agent_runs mutex poisoned")
            .remove(&run_id);

        // Release only when we hold the sole reference (no in-flight step).
        match Arc::try_unwrap(entry_arc) {
            Ok(mutex) => {
                let entry = mutex.into_inner();
                let rid = entry.run_id.clone();
                let _ = state.agent_driver.release(entry.session).await;
                state
                    .event_sink
                    .emit(
                        rid.clone(),
                        EventType::AgentCompleted,
                        serde_json::json!({}),
                        format!("{rid}:agent_completed"),
                    )
                    .await;
            }
            Err(_still_in_use) => {
                tracing::warn!(run_id = %run_id, "close_run raced an in-flight step; session will drop on last ref");
            }
        }

        Ok(Json(Envelope::ok(
            request_id,
            serde_json::json!({ "closed": true }),
        )))
    }

    /// Agent routes, merged into the protected router so they inherit
    /// `require_auth` (JWT → `Claims`). State is applied by the parent router.
    pub fn agent_router() -> Router<AppState> {
        Router::new()
            .route("/v1/agent/runs", post(start_run))
            .route("/v1/agent/runs/:run_id/step", post(step))
            .route("/v1/agent/runs/:run_id", delete(close_run))
    }
}

/// No-op router when the `browser-agent` feature is disabled, so `routes.rs`
/// can unconditionally `.merge(crate::agent_routes::agent_router())`.
#[cfg(not(feature = "browser-agent"))]
pub fn agent_router() -> axum::Router<crate::state::AppState> {
    axum::Router::new()
}
