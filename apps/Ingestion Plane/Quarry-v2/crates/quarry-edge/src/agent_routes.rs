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
    use std::convert::Infallible;
    use std::fmt::Display;
    use std::str::FromStr;
    use std::sync::{Arc, Mutex as StdMutex};
    use std::time::Duration;

    use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
    use axum::extract::{Path, Query, State};
    use axum::http::StatusCode;
    use axum::response::sse::{Event, KeepAlive};
    use axum::response::{IntoResponse, Sse};
    use axum::routing::{delete, get, post};
    use axum::{Extension, Json, Router};
    use serde::de::Error as _;
    use serde::{Deserialize, Deserializer, Serialize};
    use tokio::sync::Mutex as TokioMutex;

    use quarry_browser::{
        BrowserDevtoolsEvent, BrowserSession, BrowserTab, LiveFrameFormat, LiveFrameOptions,
    };
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
    use quarry_core::QuarryResult;
    use quarry_runtime::observation::{ObservationContext, ObservationRunner};
    use quarry_security::{Decision, SecurityEngine};
    use url::Url;

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

    async fn validate_navigation(
        action: &AgentAction,
        security: &dyn SecurityEngine,
    ) -> QuarryResult<()> {
        let AgentAction::Navigate { url } = action else {
            return Ok(());
        };
        let parsed = Url::parse(url).map_err(|error| {
            QuarryError::new(
                ErrorCode::BadRequest,
                format!("invalid navigation URL: {error}"),
            )
        })?;
        let verdict = security.preflight(&parsed).await;
        if verdict.decision == Decision::Block {
            return Err(QuarryError::new(
                ErrorCode::SecurityBlocked,
                verdict.reasons.join("; "),
            ));
        }
        if !security.allow_private_hosts() {
            quarry_runtime::dns_guard::guard_url(&parsed).await?;
        }
        Ok(())
    }

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

    /// Defense-in-depth ZDR guard for `POST /v1/agent/runs`, independent of
    /// (and not reliant on) the Velion gateway's own `effective_profile_scope`
    /// check. The gateway already rejects a client-supplied
    /// `{zdr: true, profileId: "<real>"}` combination before ever proxying to
    /// Quarry-edge (`fix(gateway): close ZDR bypass via explicit ephemeral
    /// scope claim`), but Quarry-edge is the layer that actually launches the
    /// browser and calls `ProfileStore::save` on release — any other direct
    /// caller of this route (a different consumer, a future service, a bug
    /// upstream) must not be able to make a ZDR run's cookies/storage durable
    /// just by supplying a `profile_id` or `persist_profile: true`. Without
    /// this check, `persist_profile = body.persist_profile ||
    /// body.profile_id.is_some()` ignored `zdr` entirely, so `close_run` →
    /// `agent_driver.release()` → `persist_current_page()` (gated only on
    /// `lease.persist_profile`, which has no notion of ZDR at all — see
    /// `quarry_core::lease::BrowserLease`) would happily write a ZDR
    /// session's cookies into a named profile.
    fn zdr_forbids_persistent_profile(
        zdr: ZdrMode,
        persist_profile_flag: bool,
        has_profile_id: bool,
    ) -> bool {
        zdr.is_active() && (persist_profile_flag || has_profile_id)
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

    #[derive(Debug, Clone, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct FrameQuery {
        #[serde(default)]
        pub format: Option<String>,
        #[serde(default, deserialize_with = "deserialize_optional_query_u8")]
        pub quality: Option<u8>,
        #[serde(default, deserialize_with = "deserialize_optional_query_u32")]
        pub max_width: Option<u32>,
        #[serde(default, deserialize_with = "deserialize_optional_query_u32")]
        pub max_height: Option<u32>,
        #[serde(default, deserialize_with = "deserialize_optional_query_u32")]
        pub every_nth_frame: Option<u32>,
        #[serde(default, deserialize_with = "deserialize_optional_query_u64")]
        pub timeout_ms: Option<u64>,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct FrameStreamQuery {
        #[serde(flatten)]
        pub frame: FrameQuery,
        #[serde(default, deserialize_with = "deserialize_optional_query_u64")]
        pub interval_ms: Option<u64>,
        #[serde(default, deserialize_with = "deserialize_optional_query_u64")]
        pub max_frames: Option<u64>,
    }

    #[derive(Debug, Deserialize)]
    #[serde(untagged)]
    enum QueryScalar<T> {
        Number(T),
        String(String),
    }

    fn deserialize_optional_query_u8<'de, D>(deserializer: D) -> Result<Option<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserialize_optional_query_number(deserializer)
    }

    fn deserialize_optional_query_u32<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserialize_optional_query_number(deserializer)
    }

    fn deserialize_optional_query_u64<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserialize_optional_query_number(deserializer)
    }

    fn deserialize_optional_query_number<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
    where
        D: Deserializer<'de>,
        T: Deserialize<'de> + FromStr,
        T::Err: Display,
    {
        match Option::<QueryScalar<T>>::deserialize(deserializer)? {
            Some(QueryScalar::Number(value)) => Ok(Some(value)),
            Some(QueryScalar::String(value)) => {
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    Ok(None)
                } else {
                    trimmed.parse::<T>().map(Some).map_err(D::Error::custom)
                }
            }
            None => Ok(None),
        }
    }

    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct LiveFrameData {
        pub mime_type: String,
        pub data_base64: String,
        pub zdr: bool,
    }

    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct LiveFrameStreamData {
        pub sequence: u64,
        pub mime_type: String,
        pub data_base64: String,
        pub zdr: bool,
    }

    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct BrowserTabsData {
        pub tabs: Vec<BrowserTab>,
    }

    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct BrowserTabData {
        pub tab: BrowserTab,
        pub tabs: Vec<BrowserTab>,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct DevtoolsQuery {
        #[serde(default)]
        pub after_sequence: Option<u64>,
        #[serde(default)]
        pub limit: Option<usize>,
    }

    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct DevtoolsEventsData {
        pub events: Vec<BrowserDevtoolsEvent>,
        pub zdr: bool,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct NewTabBody {
        #[serde(default)]
        pub url: Option<String>,
    }

    #[derive(Debug, Deserialize)]
    #[serde(tag = "type", rename_all = "snake_case")]
    pub enum BrowserWsClientMessage {
        Action {
            action: AgentAction,
            #[serde(default)]
            instruction: Option<String>,
        },
        Ping,
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

        if zdr_forbids_persistent_profile(zdr, body.persist_profile, body.profile_id.is_some()) {
            return Err(status_err(
                StatusCode::BAD_REQUEST,
                &request_id,
                "zdr_persistent_profile_forbidden: a ZDR run cannot request a persistent profile or profile_id",
            ));
        }

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
                previous_screenshot: None,
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
            .emit_for_zdr(
                zdr,
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

    fn default_stream_viewport() -> BrowserViewport {
        BrowserViewport {
            width: 1280,
            height: 800,
            device_scale_factor: 1.0,
            is_mobile: false,
        }
    }

    fn live_frame_format(value: Option<&str>) -> Result<LiveFrameFormat, &'static str> {
        match value.map(str::trim).unwrap_or("jpeg") {
            "jpeg" | "jpg" => Ok(LiveFrameFormat::Jpeg),
            "png" => Ok(LiveFrameFormat::Png),
            _ => Err("format must be jpeg or png"),
        }
    }

    fn live_frame_options(
        query: &FrameQuery,
        viewport: BrowserViewport,
    ) -> Result<LiveFrameOptions, &'static str> {
        Ok(LiveFrameOptions {
            format: live_frame_format(query.format.as_deref())?,
            quality: query.quality.unwrap_or(65).clamp(1, 100),
            max_width: query
                .max_width
                .unwrap_or(viewport.width)
                .clamp(320, viewport.width.max(320)),
            max_height: query
                .max_height
                .unwrap_or(viewport.height)
                .clamp(240, viewport.height.max(240)),
            every_nth_frame: query.every_nth_frame.unwrap_or(1).clamp(1, 10),
            timeout_ms: query.timeout_ms.unwrap_or(1_000).clamp(100, 5_000),
        })
    }

    fn stream_interval(query: &FrameStreamQuery) -> Duration {
        Duration::from_millis(query.interval_ms.unwrap_or(250).clamp(75, 2_000))
    }

    // reason: this test module is intentionally kept adjacent to `normalize_viewport`;
    // the sibling route handlers (`step`, `close_run`, `agent_router`) follow it within
    // the same `mod enabled`, so relocating the tests to the module end is not desirable.
    #[allow(clippy::items_after_test_module)]
    #[cfg(test)]
    mod tests {
        use super::*;

        #[tokio::test]
        async fn agent_navigation_blocks_private_targets_at_edge_boundary() {
            let action = AgentAction::Navigate {
                url: "http://169.254.169.254/latest/meta-data".to_owned(),
            };
            let security = quarry_security::preflight::DefaultEngine::new();

            let error = validate_navigation(&action, &security)
                .await
                .expect_err("metadata endpoint must be blocked");

            assert_eq!(error.code, ErrorCode::SecurityBlocked);
        }

        #[tokio::test]
        async fn non_navigation_actions_do_not_trigger_network_validation() {
            let action = AgentAction::Click {
                selector: "#continue".to_owned(),
            };
            let security = quarry_security::preflight::DefaultEngine::new();

            validate_navigation(&action, &security).await.unwrap();
        }

        #[test]
        fn zdr_forbids_persistent_profile_when_profile_id_supplied() {
            assert!(zdr_forbids_persistent_profile(ZdrMode::On, false, true));
        }

        #[test]
        fn zdr_forbids_persistent_profile_when_flag_set_without_id() {
            assert!(zdr_forbids_persistent_profile(ZdrMode::On, true, false));
        }

        #[test]
        fn zdr_forbids_persistent_profile_when_both_signals_present() {
            assert!(zdr_forbids_persistent_profile(ZdrMode::On, true, true));
        }

        #[test]
        fn zdr_run_with_no_persistence_signal_is_allowed() {
            assert!(!zdr_forbids_persistent_profile(ZdrMode::On, false, false));
        }

        #[test]
        fn non_zdr_run_may_request_a_persistent_profile() {
            assert!(!zdr_forbids_persistent_profile(ZdrMode::Off, true, true));
        }

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

        #[test]
        fn frame_stream_query_accepts_url_encoded_numbers() {
            let uri: axum::http::Uri =
                "/frames/ws?format=jpeg&quality=55&maxWidth=640&maxHeight=480&everyNthFrame=2&timeoutMs=750&intervalMs=150&maxFrames=8"
                    .parse()
                    .expect("valid uri");
            let Query(query) = Query::<FrameStreamQuery>::try_from_uri(&uri)
                .expect("browser query numbers should parse");

            assert_eq!(query.frame.format.as_deref(), Some("jpeg"));
            assert_eq!(query.frame.quality, Some(55));
            assert_eq!(query.frame.max_width, Some(640));
            assert_eq!(query.frame.max_height, Some(480));
            assert_eq!(query.frame.every_nth_frame, Some(2));
            assert_eq!(query.frame.timeout_ms, Some(750));
            assert_eq!(query.interval_ms, Some(150));
            assert_eq!(query.max_frames, Some(8));
        }

        #[test]
        fn live_frame_options_clamp_stream_parameters() {
            let options = live_frame_options(
                &FrameQuery {
                    format: Some("jpeg".to_owned()),
                    quality: Some(0),
                    max_width: Some(99_999),
                    max_height: Some(10),
                    every_nth_frame: Some(0),
                    timeout_ms: Some(99_999),
                },
                BrowserViewport {
                    width: 1280,
                    height: 800,
                    device_scale_factor: 1.0,
                    is_mobile: false,
                },
            )
            .expect("valid frame options");

            assert_eq!(options.quality, 1);
            assert_eq!(options.max_width, 1280);
            assert_eq!(options.max_height, 240);
            assert_eq!(options.every_nth_frame, 1);
            assert_eq!(options.timeout_ms, 5_000);
        }

        #[test]
        fn browser_ws_client_message_parses_action() {
            let message: BrowserWsClientMessage = serde_json::from_value(serde_json::json!({
                "type": "action",
                "action": { "type": "click_point", "x": 12.5, "y": 30.0 },
                "instruction": "human takeover click"
            }))
            .expect("valid websocket action");

            match message {
                BrowserWsClientMessage::Action {
                    action: AgentAction::ClickPoint { x, y },
                    instruction,
                } => {
                    assert_eq!(x, 12.5);
                    assert_eq!(y, 30.0);
                    assert_eq!(instruction.as_deref(), Some("human takeover click"));
                }
                other => panic!("unexpected message: {other:?}"),
            }
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

        validate_navigation(&body.action, state.security.as_ref())
            .await
            .map_err(|error| driver_err(&request_id, error))?;

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
            visual_processor: state.visual_processor.clone(),
        };

        // Disjoint borrows: &session (shared) + &mut ctx (exclusive).
        let entry_mut: &mut RunEntry = &mut entry;
        let obs = runner
            .execute(&req, &entry_mut.session, &mut entry_mut.ctx)
            .await
            .map_err(|e| driver_err(&request_id, e))?;

        Ok(Json(Envelope::ok(request_id, obs)))
    }

    /// `GET /v1/agent/runs/{run_id}/frame` — return one transient live frame.
    ///
    /// This never writes an artifact. It is for live preview/human takeover only;
    /// durable evidence still flows through ObservationRunner screenshots.
    pub async fn live_frame(
        State(state): State<AppState>,
        Extension(claims): Extension<crate::auth::Claims>,
        Path(run_id): Path<String>,
        Query(query): Query<FrameQuery>,
    ) -> Result<Json<Envelope<LiveFrameData>>, ApiErr> {
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

        let entry = entry_arc.lock().await;
        if entry.org_id != claims.org_id {
            return Err(status_err(
                StatusCode::FORBIDDEN,
                &request_id,
                "run belongs to another org",
            ));
        }

        let viewport = entry.lease.viewport.unwrap_or_else(default_stream_viewport);
        let options = live_frame_options(&query, viewport)
            .map_err(|msg| status_err(StatusCode::BAD_REQUEST, &request_id, msg))?;

        let frame = state
            .agent_driver
            .live_frame(&entry.session, options)
            .await
            .map_err(|e| driver_err(&request_id, e))?;

        Ok(Json(Envelope::ok(
            request_id,
            LiveFrameData {
                mime_type: frame.mime_type,
                data_base64: frame.data_base64,
                zdr: matches!(entry.zdr, ZdrMode::On),
            },
        )))
    }

    /// `GET /v1/agent/runs/{run_id}/frames/stream` — stream transient live frames.
    ///
    /// Frame events are never persisted. This is the read-only visual channel
    /// for live preview; browser actions still flow through `/step`.
    pub async fn live_frame_stream(
        State(state): State<AppState>,
        Extension(claims): Extension<crate::auth::Claims>,
        Path(run_id): Path<String>,
        Query(query): Query<FrameStreamQuery>,
    ) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>>, ApiErr> {
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

        let (session, viewport, zdr) = {
            let entry = entry_arc.lock().await;
            if entry.org_id != claims.org_id {
                return Err(status_err(
                    StatusCode::FORBIDDEN,
                    &request_id,
                    "run belongs to another org",
                ));
            }
            (
                entry.session.clone(),
                entry.lease.viewport.unwrap_or_else(default_stream_viewport),
                entry.zdr,
            )
        };
        let options = live_frame_options(&query.frame, viewport)
            .map_err(|msg| status_err(StatusCode::BAD_REQUEST, &request_id, msg))?;
        let interval = stream_interval(&query);
        let max_frames = query.max_frames.unwrap_or(0).min(7_200);
        let driver = state.agent_driver.clone();
        let zdr_active = matches!(zdr, ZdrMode::On);

        let stream = async_stream::stream! {
            let mut sequence = 0_u64;
            loop {
                if max_frames > 0 && sequence >= max_frames {
                    yield Ok(Event::default().event("done").data("max_frames"));
                    break;
                }

                match driver.live_frame(&session, options).await {
                    Ok(frame) => {
                        sequence = sequence.saturating_add(1);
                        let payload = LiveFrameStreamData {
                            sequence,
                            mime_type: frame.mime_type,
                            data_base64: frame.data_base64,
                            zdr: zdr_active,
                        };
                        match Event::default()
                            .event("frame")
                            .id(sequence.to_string())
                            .json_data(&payload)
                        {
                            Ok(event) => yield Ok(event),
                            Err(err) => {
                                yield Ok(Event::default().event("error").data(err.to_string()));
                                break;
                            }
                        }
                    }
                    Err(err) => {
                        yield Ok(Event::default().event("error").data(err.to_string()));
                        break;
                    }
                }

                tokio::time::sleep(interval).await;
            }
        };

        Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
    }

    /// `GET /v1/agent/runs/{run_id}/frames/ws` — bidirectional browser channel.
    ///
    /// Server sends transient `frame` events. Client may send
    /// `{ "type": "action", "action": ... }` to execute a normal Quarry
    /// browser action and receive an `observation` event. Frames are never
    /// persisted; action observations still use the existing artifact-backed
    /// `ObservationRunner` path so replay/debugging stays deterministic.
    pub async fn live_frame_ws(
        State(state): State<AppState>,
        Extension(claims): Extension<crate::auth::Claims>,
        Path(run_id): Path<String>,
        Query(query): Query<FrameStreamQuery>,
        ws: WebSocketUpgrade,
    ) -> Result<impl IntoResponse, ApiErr> {
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

        let (session, viewport, zdr) = {
            let entry = entry_arc.lock().await;
            if entry.org_id != claims.org_id {
                return Err(status_err(
                    StatusCode::FORBIDDEN,
                    &request_id,
                    "run belongs to another org",
                ));
            }
            (
                entry.session.clone(),
                entry.lease.viewport.unwrap_or_else(default_stream_viewport),
                entry.zdr,
            )
        };
        let options = live_frame_options(&query.frame, viewport)
            .map_err(|msg| status_err(StatusCode::BAD_REQUEST, &request_id, msg))?;
        let interval = stream_interval(&query);
        let max_frames = query.max_frames.unwrap_or(0).min(7_200);
        let zdr_active = matches!(zdr, ZdrMode::On);

        Ok(ws.on_upgrade(move |socket| {
            browser_ws_loop(
                socket, state, entry_arc, session, options, interval, max_frames, zdr_active,
            )
        }))
    }

    // reason: websocket handler glue — every argument is a distinct, already
    // clamped/authorized concern handed over from `live_frame_ws`; bundling
    // them into a one-off struct would only move the argument list.
    #[allow(clippy::too_many_arguments)]
    async fn browser_ws_loop(
        mut socket: WebSocket,
        state: AppState,
        entry_arc: Arc<TokioMutex<RunEntry>>,
        session: BrowserSession,
        options: LiveFrameOptions,
        interval: Duration,
        max_frames: u64,
        zdr_active: bool,
    ) {
        let mut ticker = tokio::time::interval(interval);
        let mut sequence = 0_u64;
        let mut devtools_sequence = 0_u64;

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    if max_frames > 0 && sequence >= max_frames {
                        let _ = send_ws_json(&mut socket, serde_json::json!({
                            "type": "done",
                            "reason": "max_frames"
                        })).await;
                        break;
                    }

                    match state.agent_driver.live_frame(&session, options).await {
                        Ok(frame) => {
                            sequence = sequence.saturating_add(1);
                            if send_ws_json(&mut socket, serde_json::json!({
                                "type": "frame",
                                "sequence": sequence,
                                "mimeType": frame.mime_type,
                                "dataBase64": frame.data_base64,
                                "zdr": zdr_active
                            })).await.is_err() {
                                break;
                            }
                        }
                        Err(err) => {
                            let _ = send_ws_json(&mut socket, serde_json::json!({
                                "type": "error",
                                "message": err.to_string()
                            })).await;
                            break;
                        }
                    }

                    if let Ok(events) = state
                        .agent_driver
                        .devtools_events(&session, devtools_sequence, 100)
                        .await
                    {
                        if let Some(last) = events.last() {
                            devtools_sequence = last.sequence;
                        }
                        if !events.is_empty()
                            && send_ws_json(&mut socket, serde_json::json!({
                                "type": "devtools",
                                "events": events,
                                "zdr": zdr_active
                            })).await.is_err()
                        {
                            break;
                        }
                    }
                }
                message = socket.recv() => {
                    match message {
                        Some(Ok(Message::Text(text))) => {
                            match serde_json::from_str::<BrowserWsClientMessage>(&text) {
                                Ok(BrowserWsClientMessage::Ping) => {
                                    if send_ws_json(&mut socket, serde_json::json!({ "type": "pong" })).await.is_err() {
                                        break;
                                    }
                                }
                                Ok(BrowserWsClientMessage::Action { action, instruction }) => {
                                    match execute_ws_action(&state, &entry_arc, action, instruction).await {
                                        Ok(observation) => {
                                            if send_ws_json(&mut socket, serde_json::json!({
                                                "type": "observation",
                                                "observation": observation
                                            })).await.is_err() {
                                                break;
                                            }
                                        }
                                        Err(err) => {
                                            let _ = send_ws_json(&mut socket, serde_json::json!({
                                                "type": "error",
                                                "message": err.to_string()
                                            })).await;
                                        }
                                    }
                                }
                                Err(err) => {
                                    let _ = send_ws_json(&mut socket, serde_json::json!({
                                        "type": "error",
                                        "message": format!("invalid websocket message: {err}")
                                    })).await;
                                }
                            }
                        }
                        Some(Ok(Message::Ping(payload))) => {
                            if socket.send(Message::Pong(payload)).await.is_err() {
                                break;
                            }
                        }
                        Some(Ok(Message::Close(_))) | None => break,
                        Some(Ok(_)) => {}
                        Some(Err(err)) => {
                            tracing::debug!(error = %err, "browser websocket receive failed");
                            break;
                        }
                    }
                }
            }
        }
    }

    async fn execute_ws_action(
        state: &AppState,
        entry_arc: &Arc<TokioMutex<RunEntry>>,
        action: AgentAction,
        instruction: Option<String>,
    ) -> QuarryResult<BrowserObservation> {
        validate_navigation(&action, state.security.as_ref()).await?;
        let mut entry = entry_arc.lock().await;
        let req = AgentActionRequest {
            run_id: entry.run_id.clone(),
            lease_id: entry.lease.lease_id.clone(),
            action,
            instruction,
            constraints: entry.constraints.clone(),
            zdr: entry.zdr,
        };
        let runner = ObservationRunner {
            browser: state.agent_driver.clone(),
            artifacts: Some(state.artifacts.clone()),
            events: Some(state.event_sink.clone()),
            visual_processor: state.visual_processor.clone(),
        };
        let entry_mut: &mut RunEntry = &mut entry;
        runner
            .execute(&req, &entry_mut.session, &mut entry_mut.ctx)
            .await
    }

    async fn send_ws_json(
        socket: &mut WebSocket,
        value: serde_json::Value,
    ) -> Result<(), axum::Error> {
        socket.send(Message::Text(value.to_string())).await
    }

    /// `GET /v1/agent/runs/{run_id}/tabs` — list live browser tabs.
    pub async fn list_tabs(
        State(state): State<AppState>,
        Extension(claims): Extension<crate::auth::Claims>,
        Path(run_id): Path<String>,
    ) -> Result<Json<Envelope<BrowserTabsData>>, ApiErr> {
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

        let entry = entry_arc.lock().await;
        if entry.org_id != claims.org_id {
            return Err(status_err(
                StatusCode::FORBIDDEN,
                &request_id,
                "run belongs to another org",
            ));
        }

        let tabs = state
            .agent_driver
            .list_tabs(&entry.session)
            .await
            .map_err(|e| driver_err(&request_id, e))?;

        Ok(Json(Envelope::ok(request_id, BrowserTabsData { tabs })))
    }

    /// `POST /v1/agent/runs/{run_id}/tabs` — open a new live browser tab.
    pub async fn new_tab(
        State(state): State<AppState>,
        Extension(claims): Extension<crate::auth::Claims>,
        Path(run_id): Path<String>,
        Json(body): Json<NewTabBody>,
    ) -> Result<Json<Envelope<BrowserTabData>>, ApiErr> {
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

        let entry = entry_arc.lock().await;
        if entry.org_id != claims.org_id {
            return Err(status_err(
                StatusCode::FORBIDDEN,
                &request_id,
                "run belongs to another org",
            ));
        }

        let tab = state
            .agent_driver
            .new_tab(&entry.session, body.url.as_deref())
            .await
            .map_err(|e| driver_err(&request_id, e))?;
        let tabs = state
            .agent_driver
            .list_tabs(&entry.session)
            .await
            .map_err(|e| driver_err(&request_id, e))?;

        Ok(Json(Envelope::ok(request_id, BrowserTabData { tab, tabs })))
    }

    /// `POST /v1/agent/runs/{run_id}/tabs/{tab_id}/select` — select active tab.
    pub async fn select_tab(
        State(state): State<AppState>,
        Extension(claims): Extension<crate::auth::Claims>,
        Path((run_id, tab_id)): Path<(String, String)>,
    ) -> Result<Json<Envelope<BrowserTabsData>>, ApiErr> {
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

        let entry = entry_arc.lock().await;
        if entry.org_id != claims.org_id {
            return Err(status_err(
                StatusCode::FORBIDDEN,
                &request_id,
                "run belongs to another org",
            ));
        }

        state
            .agent_driver
            .select_tab(&entry.session, &tab_id)
            .await
            .map_err(|e| driver_err(&request_id, e))?;
        let tabs = state
            .agent_driver
            .list_tabs(&entry.session)
            .await
            .map_err(|e| driver_err(&request_id, e))?;

        Ok(Json(Envelope::ok(request_id, BrowserTabsData { tabs })))
    }

    /// `DELETE /v1/agent/runs/{run_id}/tabs/{tab_id}` — close a live tab.
    pub async fn close_tab(
        State(state): State<AppState>,
        Extension(claims): Extension<crate::auth::Claims>,
        Path((run_id, tab_id)): Path<(String, String)>,
    ) -> Result<Json<Envelope<BrowserTabsData>>, ApiErr> {
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

        let entry = entry_arc.lock().await;
        if entry.org_id != claims.org_id {
            return Err(status_err(
                StatusCode::FORBIDDEN,
                &request_id,
                "run belongs to another org",
            ));
        }

        state
            .agent_driver
            .close_tab(&entry.session, &tab_id)
            .await
            .map_err(|e| driver_err(&request_id, e))?;
        let tabs = state
            .agent_driver
            .list_tabs(&entry.session)
            .await
            .map_err(|e| driver_err(&request_id, e))?;

        Ok(Json(Envelope::ok(request_id, BrowserTabsData { tabs })))
    }

    /// `GET /v1/agent/runs/{run_id}/devtools` — fetch transient DevTools events.
    pub async fn devtools_events(
        State(state): State<AppState>,
        Extension(claims): Extension<crate::auth::Claims>,
        Path(run_id): Path<String>,
        Query(query): Query<DevtoolsQuery>,
    ) -> Result<Json<Envelope<DevtoolsEventsData>>, ApiErr> {
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

        let entry = entry_arc.lock().await;
        if entry.org_id != claims.org_id {
            return Err(status_err(
                StatusCode::FORBIDDEN,
                &request_id,
                "run belongs to another org",
            ));
        }

        let events = state
            .agent_driver
            .devtools_events(
                &entry.session,
                query.after_sequence.unwrap_or(0),
                query.limit.unwrap_or(100).clamp(1, 512),
            )
            .await
            .map_err(|e| driver_err(&request_id, e))?;

        Ok(Json(Envelope::ok(
            request_id,
            DevtoolsEventsData {
                events,
                zdr: matches!(entry.zdr, ZdrMode::On),
            },
        )))
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
                let zdr = entry.zdr;
                let _ = state.agent_driver.release(entry.session).await;
                state
                    .event_sink
                    .emit_for_zdr(
                        zdr,
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
            .route("/v1/agent/runs/:run_id/tabs", get(list_tabs).post(new_tab))
            .route(
                "/v1/agent/runs/:run_id/tabs/:tab_id/select",
                post(select_tab),
            )
            .route("/v1/agent/runs/:run_id/tabs/:tab_id", delete(close_tab))
            .route("/v1/agent/runs/:run_id/devtools", get(devtools_events))
            .route("/v1/agent/runs/:run_id/frames/ws", get(live_frame_ws))
            .route(
                "/v1/agent/runs/:run_id/frames/stream",
                get(live_frame_stream),
            )
            .route("/v1/agent/runs/:run_id/frame", get(live_frame))
            .route("/v1/agent/runs/:run_id", delete(close_run))
    }
}

/// No-op router when the `browser-agent` feature is disabled, so `routes.rs`
/// can unconditionally `.merge(crate::agent_routes::agent_router())`.
#[cfg(not(feature = "browser-agent"))]
pub fn agent_router() -> axum::Router<crate::state::AppState> {
    axum::Router::new()
}
