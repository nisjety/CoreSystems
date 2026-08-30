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
    use std::collections::{HashMap, HashSet};
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

    #[cfg(test)]
    use quarry_browser::SessionInner;
    use quarry_browser::{
        BrowserDevtoolsEvent, BrowserDriverCapabilities, BrowserEgressPolicy, BrowserSession,
        BrowserTab, LiveFrameFormat, LiveFrameOptions,
    };
    use quarry_core::contracts::{
        AgentAction, AgentActionRequest, AgentConstraints, BrowserEgressReceipt,
        BrowserObservation, BrowserTelemetry, ExtractionProfile,
    };
    use quarry_core::envelope::Envelope;
    use quarry_core::error::{ErrorCode, QuarryError};
    use quarry_core::event::EventType;
    use quarry_core::ids::kinds::{LeaseKind, ProfileKind, RequestKind, RunKind};
    use quarry_core::ids::Id;
    use quarry_core::lease::{BrowserLease, BrowserViewport, Capability};
    use quarry_core::privacy::PrivacyPolicy;
    use quarry_core::zdr::ZdrMode;
    use quarry_core::QuarryResult;
    use quarry_runtime::browser_procedure::{
        analyze_impact, assess_quality, compare_replay, compile_procedure, BrowserProcedure,
        ProcedureImpactReport, ProcedureQualityReport, ReplayDecision,
    };
    use quarry_runtime::observation::{ObservationContext, ObservationRunner, ObservationSnapshot};
    use quarry_runtime::step_receipts::{
        AgentRunCheckpoint, AgentRunStatus, BrowserControlMode, BrowserProfileScope,
        BrowserTabOperation, BrowserTimelineEvent, BrowserTimelineEventKind,
        BrowserTimelineInitiator, BrowserTimelineLifecycle, ReceiptBuilder, StepOutcome,
        StepReceipt,
    };
    use quarry_security::{Decision, SecurityEngine};
    use url::Url;

    use crate::state::AppState;

    /// A live agent run: the leased browser session plus the per-run observation
    /// context (carries `step`/current_url across `/step` calls) and budget.
    pub struct RunEntry {
        pub run_id: RunKind,
        pub org_id: String,
        /// Signed user/service actor that started this run. The browser edge
        /// scopes live-session reads and mutations to this actor as well as
        /// the tenant; an org membership alone is not browser-run authority.
        pub actor_id: String,
        pub session: BrowserSession,
        pub ctx: ObservationContext,
        pub lease: BrowserLease,
        pub constraints: AgentConstraints,
        pub zdr: ZdrMode,
        pub grant_id: Option<String>,
        pub control_mode: BrowserControlMode,
        pub profile_scope: BrowserProfileScope,
        pub execution_tier: BrowserExecutionTier,
        /// Current live observation. It is intentionally process-local for a
        /// ZDR run, while non-ZDR proof remains in immutable receipts.
        pub last_observation: Option<BrowserObservation>,
        /// A sensitive approval grant can authorize one irreversible action
        /// only. It is deliberately process-local: a resumed run must obtain
        /// fresh authority instead of replaying a prior approval.
        pub used_sensitive_approval_grants: HashSet<String>,
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

    struct SensitiveApproval {
        grant_id: String,
        parent_grant_id: String,
        action: &'static str,
        frame_id: Option<String>,
        dialog_id: Option<String>,
        artifact_id: Option<String>,
    }

    fn sensitive_approval_for_action(
        entry: &RunEntry,
        action: &AgentAction,
    ) -> QuarryResult<Option<SensitiveApproval>> {
        match action {
            AgentAction::RespondDialog {
                dialog_id,
                approval_grant_id,
                ..
            } => {
                let parent_grant_id = entry.grant_id.clone().ok_or_else(|| {
                    QuarryError::new(
                        ErrorCode::Forbidden,
                        "sensitive browser action requires a parent BrowserBroker run grant",
                    )
                })?;
                let dialog = entry
                    .last_observation
                    .as_ref()
                    .and_then(|observation| {
                        observation
                            .dialogs
                            .iter()
                            .find(|candidate| candidate.dialog_id == *dialog_id)
                    })
                    .ok_or_else(|| {
                        QuarryError::new(
                            ErrorCode::TargetRepairRequired,
                            "dialog is no longer observed; observe again before responding",
                        )
                    })?;
                Ok(Some(SensitiveApproval {
                    grant_id: approval_grant_id.clone(),
                    parent_grant_id: parent_grant_id.clone(),
                    action: "respond_dialog",
                    frame_id: dialog.frame_id.clone(),
                    dialog_id: Some(dialog_id.clone()),
                    artifact_id: None,
                }))
            }
            AgentAction::UploadRef {
                snapshot_id,
                generation,
                ref_id,
                artifact_id,
                approval_grant_id,
            }
            | AgentAction::FrameUploadRef {
                snapshot_id,
                generation,
                ref_id,
                artifact_id,
                approval_grant_id,
                ..
            } => {
                let parent_grant_id = entry.grant_id.clone().ok_or_else(|| {
                    QuarryError::new(
                        ErrorCode::Forbidden,
                        "sensitive browser action requires a parent BrowserBroker run grant",
                    )
                })?;
                let target = entry
                    .last_observation
                    .as_ref()
                    .and_then(|observation| observation.snapshot.as_ref())
                    .filter(|snapshot| {
                        snapshot.snapshot_id == *snapshot_id && snapshot.generation == *generation
                    })
                    .and_then(|snapshot| {
                        snapshot
                            .targets
                            .iter()
                            .find(|target| target.ref_id == *ref_id)
                    })
                    .ok_or_else(|| {
                        QuarryError::new(
                            ErrorCode::TargetRepairRequired,
                            "upload target is stale or absent; observe again before uploading",
                        )
                    })?;
                Ok(Some(SensitiveApproval {
                    grant_id: approval_grant_id.clone(),
                    parent_grant_id,
                    action: "upload_ref",
                    frame_id: target.frame_id.clone(),
                    dialog_id: None,
                    artifact_id: Some(artifact_id.to_string()),
                }))
            }
            AgentAction::DownloadRef {
                snapshot_id,
                generation,
                ref_id,
                approval_grant_id,
            }
            | AgentAction::FrameDownloadRef {
                snapshot_id,
                generation,
                ref_id,
                approval_grant_id,
                ..
            } => {
                let parent_grant_id = entry.grant_id.clone().ok_or_else(|| {
                    QuarryError::new(
                        ErrorCode::Forbidden,
                        "sensitive browser action requires a parent BrowserBroker run grant",
                    )
                })?;
                let target = entry
                    .last_observation
                    .as_ref()
                    .and_then(|observation| observation.snapshot.as_ref())
                    .filter(|snapshot| {
                        snapshot.snapshot_id == *snapshot_id && snapshot.generation == *generation
                    })
                    .and_then(|snapshot| {
                        snapshot
                            .targets
                            .iter()
                            .find(|target| target.ref_id == *ref_id)
                    })
                    .ok_or_else(|| {
                        QuarryError::new(
                            ErrorCode::TargetRepairRequired,
                            "download target is stale or absent; observe again before downloading",
                        )
                    })?;
                Ok(Some(SensitiveApproval {
                    grant_id: approval_grant_id.clone(),
                    parent_grant_id,
                    action: "download_ref",
                    frame_id: target.frame_id.clone(),
                    dialog_id: None,
                    artifact_id: None,
                }))
            }
            _ => Ok(None),
        }
    }

    async fn validate_sensitive_approval(
        state: &AppState,
        approval: &SensitiveApproval,
    ) -> QuarryResult<()> {
        let grant = state.grant_validator.validate(&approval.grant_id).await?;
        if !grant.is_usable() {
            Err(QuarryError::new(
                ErrorCode::SecurityBlocked,
                "sensitive browser approval grant is inactive or expired",
            ))
        } else if !grant.authorizes_exact_sensitive_action(
            approval.action,
            &approval.parent_grant_id,
            approval.frame_id.as_deref(),
            approval.dialog_id.as_deref(),
            approval.artifact_id.as_deref(),
        ) {
            Err(QuarryError::new(
                ErrorCode::SecurityBlocked,
                "sensitive browser approval grant does not match the observed action scope",
            ))
        } else {
            Ok(())
        }
    }

    /// Revalidate the per-run BrowserBroker grant and, in governed mode,
    /// prove that the live run is still using the broker's exact canonical
    /// host authority. This prevents a caller from presenting a valid grant
    /// while retaining a broader client-supplied `allowed_domains` list.
    async fn validate_run_grant(
        state: &AppState,
        grant_id: &str,
        run_domains: &[String],
    ) -> QuarryResult<()> {
        let grant = state.grant_validator.validate(grant_id).await?;
        if !grant.is_usable() {
            return Err(QuarryError::new(
                ErrorCode::SecurityBlocked,
                "browser grant is inactive or expired",
            ));
        }
        if state.require_browser_grants {
            let broker_domains =
                BrowserEgressPolicy::canonical_broker_domains(&grant.allowed_domains)
                    .map_err(|reason| QuarryError::new(ErrorCode::SecurityBlocked, reason))?;
            if broker_domains != run_domains {
                return Err(QuarryError::new(
                    ErrorCode::SecurityBlocked,
                    "browser run domain policy no longer matches its broker grant",
                ));
            }
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
        /// The effective scope is provided by the authenticated BFF after it
        /// validates the requested profile. It remains owner state so reads
        /// and resume paths never depend on a gateway-local session cache.
        #[serde(default)]
        pub profile_scope: BrowserProfileScope,
        #[serde(default)]
        pub viewport: Option<BrowserViewport>,
        #[serde(default)]
        pub zdr: bool,
        /// BrowserBroker's signed capability. Quarry revalidates it before
        /// every action, so an approval cannot be replayed after expiry.
        #[serde(default)]
        pub grant_id: Option<String>,
        /// Resume a previously checkpointed run after an edge restart. The
        /// checkpoint's tenant, constraints, profile, and current step remain
        /// authoritative; a new/renewed grant may be supplied explicitly.
        #[serde(default)]
        pub resume_run_id: Option<String>,
        /// Optional requirements for the selected execution driver. Quarry
        /// fails closed instead of silently beginning a run with a driver that
        /// cannot supply a capability the caller needs.
        #[serde(default)]
        pub driver_requirements: BrowserDriverRequirements,
        /// `ephemeral_evidence` is an opt-in experimental contract for public,
        /// disposable evidence acquisition. It is never an alias for a normal
        /// authenticated browser run.
        #[serde(default)]
        pub execution_tier: BrowserExecutionTier,
    }

    /// Browser execution contract selected at run creation. The renderer
    /// implementation is intentionally separate from the contract: the Lite
    /// experiment currently transparently falls back to Chromium while a
    /// lightweight renderer earns capability evidence on its own corpus.
    #[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
    #[serde(rename_all = "snake_case")]
    pub enum BrowserExecutionTier {
        #[default]
        Chromium,
        EphemeralEvidence,
    }

    impl BrowserExecutionTier {
        const fn is_ephemeral_evidence(self) -> bool {
            matches!(self, Self::EphemeralEvidence)
        }

        const fn engine_label(self) -> &'static str {
            match self {
                Self::Chromium => "chromium",
                // Do not imply a Boa/Blitz renderer exists merely because its
                // contract is exposed. This run is a constrained Chromium
                // fallback until a Lite engine supplies independent evidence.
                Self::EphemeralEvidence => "chromium_fallback",
            }
        }
    }

    #[derive(Debug, Default, Deserialize)]
    pub struct BrowserDriverRequirements {
        #[serde(default)]
        pub persistent_profile: bool,
        #[serde(default)]
        pub devtools_trace: bool,
        #[serde(default)]
        pub downloads_to_artifacts: bool,
        #[serde(default)]
        pub uploads_from_artifacts: bool,
        #[serde(default)]
        pub full_visual_fidelity: bool,
        #[serde(default)]
        pub isolated_egress: bool,
        #[serde(default)]
        pub security_evidence: bool,
        #[serde(default)]
        pub atomic_target_actions: bool,
    }

    fn default_constraints() -> AgentConstraints {
        AgentConstraints {
            max_steps: 0,
            allowed_domains: Vec::new(),
            max_runtime_s: None,
            max_cost_usd: None,
        }
    }

    fn domain_allowed(url: &str, allowed_domains: &[String]) -> bool {
        BrowserEgressPolicy::from_allowed_domains(allowed_domains).allows_url(url)
    }

    fn enforce_step_budget(entry: &RunEntry) -> Result<(), &'static str> {
        if entry.constraints.max_steps > 0 && entry.ctx.step >= entry.constraints.max_steps {
            return Err("agent step budget exhausted");
        }
        Ok(())
    }

    fn checkpoint_for_entry(entry: &RunEntry) -> AgentRunCheckpoint {
        AgentRunCheckpoint {
            org_id: entry.org_id.clone(),
            actor_id: entry.actor_id.clone(),
            run_id: entry.run_id.to_string(),
            lease_id: entry.lease.lease_id.to_string(),
            profile_id: entry.lease.profile_id.to_string(),
            step: entry.ctx.step,
            current_url: entry.ctx.current_url.clone(),
            page_hash: entry.ctx.page_hash.clone(),
            constraints: entry.constraints.clone(),
            persist_profile: entry.lease.persist_profile,
            profile_scope: entry.profile_scope,
            viewport: entry.lease.viewport,
            zdr: entry.zdr,
            grant_id: entry.grant_id.clone(),
            status: AgentRunStatus::Active,
            control_mode: entry.control_mode,
        }
    }

    fn caller_owns_browser_run(
        run_org_id: &str,
        run_actor_id: &str,
        claims: &crate::auth::Claims,
    ) -> bool {
        run_org_id == claims.org_id
            && !run_actor_id.trim().is_empty()
            && run_actor_id == claims.actor_id()
    }

    fn caller_owns_live_run(entry: &RunEntry, claims: &crate::auth::Claims) -> bool {
        caller_owns_browser_run(&entry.org_id, &entry.actor_id, claims)
    }

    async fn record_browser_timeline_event(
        state: &AppState,
        entry: &RunEntry,
        event: BrowserTimelineEventKind,
    ) -> QuarryResult<()> {
        if entry.zdr.is_active() {
            return Ok(());
        }
        state
            .receipts
            .append_browser_timeline_event(BrowserTimelineEvent::new(
                entry.org_id.clone(),
                entry.actor_id.clone(),
                entry.run_id.to_string(),
                event,
            ))
            .await?;
        state
            .receipts
            .save_run_checkpoint(checkpoint_for_entry(entry))
            .await
    }

    /// Defense-in-depth ZDR guard for `POST /v1/agent/runs`, independent of
    /// (and not reliant on) the Verevon gateway's own `effective_profile_scope`
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
        /// What the selected driver can actually supply for this lease. This
        /// is evidence-backed routing metadata, not a promise inferred from a
        /// provider name.
        pub driver_capabilities: BrowserDriverCapabilities,
        pub execution_tier: BrowserExecutionTier,
        /// Actual engine used for this run. `chromium_fallback` is explicit so
        /// callers never infer Lite throughput/fidelity from a request alone.
        pub execution_engine: &'static str,
    }

    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct BrowserSessionProjectionData {
        pub run_id: String,
        pub lease_id: String,
        pub profile_id: String,
        pub profile_storage: &'static str,
        pub profile_scope: BrowserProfileScope,
        pub viewport: Option<BrowserViewport>,
        pub current_url: String,
        pub step: u32,
        pub status: AgentRunStatus,
        pub live: bool,
        pub zdr: bool,
        pub control_mode: BrowserControlMode,
        pub execution_tier: BrowserExecutionTier,
        pub tabs: Vec<BrowserSessionTab>,
        pub last_observation: Option<BrowserObservation>,
    }

    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct BrowserSessionTab {
        pub tab_id: String,
        pub title: Option<String>,
        pub url: Option<String>,
        pub active: bool,
    }

    fn safe_display_url(raw: &str) -> String {
        let Ok(mut parsed) = Url::parse(raw) else {
            return String::new();
        };
        let _ = parsed.set_username("");
        let _ = parsed.set_password(None);
        parsed.set_query(None);
        parsed.set_fragment(None);
        parsed.to_string()
    }

    fn browser_session_tab(tab: BrowserTab) -> BrowserSessionTab {
        BrowserSessionTab {
            tab_id: tab.tab_id,
            title: tab.title,
            url: tab
                .url
                .as_deref()
                .map(safe_display_url)
                .filter(|value| !value.is_empty()),
            active: tab.active,
        }
    }

    fn active_browser_tab_id(tabs: &[BrowserTab]) -> Option<String> {
        tabs.iter()
            .find(|tab| tab.active)
            .map(|tab| tab.tab_id.clone())
    }

    fn live_browser_session_projection(
        entry: &RunEntry,
        tabs: Vec<BrowserTab>,
    ) -> BrowserSessionProjectionData {
        BrowserSessionProjectionData {
            run_id: entry.run_id.to_string(),
            lease_id: entry.lease.lease_id.to_string(),
            profile_id: entry.lease.profile_id.to_string(),
            profile_storage: if entry.lease.persist_profile {
                "persistent"
            } else {
                "ephemeral"
            },
            profile_scope: entry.profile_scope,
            viewport: entry.lease.viewport,
            current_url: safe_display_url(&entry.ctx.current_url),
            step: entry.ctx.step,
            status: AgentRunStatus::Active,
            live: true,
            zdr: entry.zdr.is_active(),
            control_mode: entry.control_mode,
            execution_tier: entry.execution_tier,
            tabs: tabs.into_iter().map(browser_session_tab).collect(),
            last_observation: entry.last_observation.clone(),
        }
    }

    fn checkpoint_browser_session_projection(
        checkpoint: AgentRunCheckpoint,
    ) -> BrowserSessionProjectionData {
        BrowserSessionProjectionData {
            run_id: checkpoint.run_id,
            lease_id: checkpoint.lease_id,
            profile_id: checkpoint.profile_id,
            profile_storage: if checkpoint.persist_profile {
                "persistent"
            } else {
                "ephemeral"
            },
            profile_scope: checkpoint.profile_scope,
            viewport: checkpoint.viewport,
            current_url: safe_display_url(&checkpoint.current_url),
            step: checkpoint.step,
            status: checkpoint.status,
            live: false,
            zdr: checkpoint.zdr.is_active(),
            control_mode: checkpoint.control_mode,
            // Ephemeral evidence runs are ZDR and never checkpointed.
            execution_tier: BrowserExecutionTier::Chromium,
            tabs: Vec::new(),
            last_observation: None,
        }
    }

    fn browser_action_name(action: &AgentAction) -> &'static str {
        match action {
            AgentAction::Navigate { .. } => "navigate",
            AgentAction::Click { .. } => "click",
            AgentAction::ClickRef { .. } => "click_ref",
            AgentAction::FrameClickRef { .. } => "frame_click_ref",
            AgentAction::ClickSemantic { .. } => "click_semantic",
            AgentAction::ClickPoint { .. } => "click_point",
            AgentAction::Type { .. } => "type",
            AgentAction::TypeRef { .. } => "type_ref",
            AgentAction::FrameTypeRef { .. } => "frame_type_ref",
            AgentAction::TypeSemantic { .. } => "type_semantic",
            AgentAction::Press { .. } => "press",
            AgentAction::Scroll { .. } => "scroll",
            AgentAction::MouseWheel { .. } => "mouse_wheel",
            AgentAction::Select { .. } => "select",
            AgentAction::SelectRef { .. } => "select_ref",
            AgentAction::FrameSelectRef { .. } => "frame_select_ref",
            AgentAction::SelectSemantic { .. } => "select_semantic",
            AgentAction::Wait { .. } => "wait",
            AgentAction::WaitFor { .. } => "wait_for",
            AgentAction::WaitForRef { .. } => "wait_for_ref",
            AgentAction::FrameWaitForRef { .. } => "frame_wait_for_ref",
            AgentAction::WaitForSemantic { .. } => "wait_for_semantic",
            AgentAction::RespondDialog { .. } => "respond_dialog",
            AgentAction::UploadRef { .. } => "upload_ref",
            AgentAction::FrameUploadRef { .. } => "frame_upload_ref",
            AgentAction::DownloadRef { .. } => "download_ref",
            AgentAction::FrameDownloadRef { .. } => "frame_download_ref",
            AgentAction::Screenshot { .. } => "screenshot",
            AgentAction::Pdf => "pdf",
            AgentAction::Evaluate { .. } => "evaluate",
            AgentAction::Back => "back",
            AgentAction::Forward => "forward",
            AgentAction::GetContent => "get_content",
        }
    }

    /// Step receipts retain a provider-authoritative action cost whenever the
    /// driver supplied one. W4: when the driver omits a cost, fall back to
    /// the flat action-cost table so `max_cost_usd` budgets are enforceable
    /// even without provider-billed metering. Callers can distinguish
    /// estimated vs provider-billed via the action_cost source field on
    /// the receipt (flat_table vs provider_billed) once W4's receipt
    /// `cost` field lands; for now the micro_usd value itself is the
    /// budget signal.
    fn observation_receipt_cost_usd(observation: &BrowserObservation) -> f64 {
        telemetry_receipt_cost_usd(&observation.telemetry)
    }

    fn telemetry_receipt_cost_usd(telemetry: &BrowserTelemetry) -> f64 {
        telemetry
            .verified_action_cost_micro_usd
            .map(|micro_usd| micro_usd as f64 / 1_000_000.0)
            .unwrap_or(0.0)
    }

    /// W4: estimate a cost for an action when no provider-billed cost is
    /// available. Used as the fallback inside `observation_receipt_cost_usd`
    /// callers and by the AgentLoop budget check. The estimate is
    /// intentionally conservative (flat table) so budgets fail closed
    /// rather than silently under-billing.
    fn estimated_action_cost_usd(action: &AgentAction) -> f64 {
        let name = match action {
            AgentAction::Navigate { .. } => "navigate",
            AgentAction::Click { .. } => "click",
            AgentAction::ClickRef { .. } => "click",
            AgentAction::FrameClickRef { .. } => "click",
            AgentAction::Type { .. } => "type",
            AgentAction::TypeRef { .. } => "type",
            AgentAction::FrameTypeRef { .. } => "type",
            AgentAction::Press { .. } => "press",
            AgentAction::Scroll { .. } => "scroll",
            AgentAction::Select { .. } => "select",
            AgentAction::SelectRef { .. } => "select",
            AgentAction::FrameSelectRef { .. } => "select",
            AgentAction::Wait { .. } => "wait",
            AgentAction::WaitFor { .. } => "wait",
            AgentAction::WaitForRef { .. } => "wait",
            AgentAction::FrameWaitForRef { .. } => "wait",
            AgentAction::WaitForSemantic { .. } => "wait",
            AgentAction::RespondDialog { .. } => "press",
            AgentAction::UploadRef { .. } => "type",
            AgentAction::FrameUploadRef { .. } => "type",
            AgentAction::DownloadRef { .. } => "download_ref",
            AgentAction::FrameDownloadRef { .. } => "frame_download_ref",
            AgentAction::Screenshot { .. } => "screenshot",
            AgentAction::Pdf => "pdf",
            AgentAction::Evaluate { .. } => "evaluate",
            AgentAction::Back => "back",
            AgentAction::Forward => "forward",
            AgentAction::GetContent => "get_content",
            AgentAction::MouseWheel { .. } => "mouse_wheel",
            AgentAction::SelectSemantic { .. } => "select",
            AgentAction::ClickSemantic { .. }
            | AgentAction::ClickPoint { .. }
            | AgentAction::TypeSemantic { .. } => "click",
        };
        quarry_runtime::action_cost::estimate_action_cost(name, 0).total_usd
    }

    fn browser_timeline_action(receipt: StepReceipt) -> BrowserTimelineItem {
        let (outcome, error_code) = match receipt.outcome {
            StepOutcome::Completed { .. } => ("completed", None),
            StepOutcome::Failed { error_code, .. } => ("failed", Some(error_code)),
            StepOutcome::Skipped { .. } => ("skipped", None),
        };
        BrowserTimelineItem {
            id: receipt.receipt_id,
            occurred_at: receipt.finished_at.to_rfc3339(),
            detail: BrowserTimelineItemDetail::Action {
                action: browser_action_name(&receipt.action),
                outcome,
                error_code,
            },
        }
    }

    fn browser_timeline_event(event: BrowserTimelineEvent) -> BrowserTimelineItem {
        let detail = match event.event {
            BrowserTimelineEventKind::Control { mode, initiated_by } => {
                BrowserTimelineItemDetail::Control { mode, initiated_by }
            }
            BrowserTimelineEventKind::Tab {
                operation,
                tab_id,
                active_tab_id,
            } => BrowserTimelineItemDetail::Tab {
                operation,
                tab_id,
                active_tab_id,
            },
            BrowserTimelineEventKind::Devtools {
                event_count,
                last_sequence,
            } => BrowserTimelineItemDetail::Devtools {
                event_count,
                last_sequence,
            },
            BrowserTimelineEventKind::Lifecycle { state } => {
                BrowserTimelineItemDetail::Lifecycle { state }
            }
        };
        BrowserTimelineItem {
            id: event.event_id,
            occurred_at: event.occurred_at.to_rfc3339(),
            detail,
        }
    }

    fn timeline_sort_key(item: &BrowserTimelineItem) -> (&str, &str) {
        (&item.occurred_at, &item.id)
    }

    #[derive(Debug, Deserialize)]
    pub struct StepBody {
        pub action: AgentAction,
        #[serde(default)]
        pub instruction: Option<String>,
        #[serde(default)]
        pub extraction_profile: Option<ExtractionProfile>,
    }

    #[derive(Debug, Deserialize)]
    pub struct BrowserSessionControlBody {
        pub mode: BrowserControlMode,
    }

    #[derive(Debug, Deserialize, Default)]
    pub struct BrowserTimelineQuery {
        #[serde(default)]
        pub cursor: Option<String>,
        #[serde(default)]
        pub limit: Option<usize>,
    }

    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct BrowserTimelineData {
        pub items: Vec<BrowserTimelineItem>,
        pub next_cursor: Option<String>,
    }

    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct BrowserTimelineItem {
        pub id: String,
        pub occurred_at: String,
        #[serde(flatten)]
        pub detail: BrowserTimelineItemDetail,
    }

    #[derive(Debug, Serialize)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum BrowserTimelineItemDetail {
        Action {
            action: &'static str,
            outcome: &'static str,
            error_code: Option<String>,
        },
        Control {
            mode: BrowserControlMode,
            initiated_by: BrowserTimelineInitiator,
        },
        Tab {
            operation: BrowserTabOperation,
            tab_id: Option<String>,
            active_tab_id: Option<String>,
        },
        Devtools {
            event_count: u32,
            last_sequence: u64,
        },
        Lifecycle {
            state: BrowserTimelineLifecycle,
        },
    }

    #[derive(Debug, Deserialize)]
    pub struct CompileProcedureBody {
        /// Stable caller-owned identifier for the compiled candidate. The
        /// procedure is still only a deterministic replay candidate; it is
        /// never silently promoted to an autonomous action policy.
        #[serde(default)]
        pub procedure_id: Option<String>,
    }

    #[derive(Debug, Deserialize)]
    pub struct ReplayCheckBody {
        pub procedure: BrowserProcedure,
        #[serde(default)]
        pub actions: Vec<AgentAction>,
    }

    #[derive(Debug, Deserialize)]
    pub struct ProcedureImpactBody {
        pub procedure: BrowserProcedure,
        #[serde(default)]
        pub changed_urls: Vec<String>,
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

    /// W3 — query for `GET /v1/agent/runs/{id}/events/stream`.
    /// `from_seq` triggers a durable-history replay before the live
    /// tail; `max_events` caps the total emission count for the
    /// connection.
    #[derive(Debug, Deserialize)]
    pub struct AgentEventsQuery {
        #[serde(default, deserialize_with = "deserialize_optional_query_u64")]
        pub from_seq: Option<u64>,
        #[serde(default, deserialize_with = "deserialize_optional_query_u64")]
        pub max_events: Option<u64>,
    }

    /// Decide whether a typed event should reach the App Shell. The
    /// agent loop today emits only `agent.*` / `action.*` /
    /// `observation.ready` events; the explicit allow-list keeps
    /// future internal/telemetry events out of the user-facing
    /// stream without changing the wire contract.
    fn is_user_facing_event(event_type: quarry_core::event::EventType) -> bool {
        use quarry_core::event::EventType;
        matches!(
            event_type,
            EventType::AgentStarted
                | EventType::ActionStarted
                | EventType::ActionCompleted
                | EventType::ActionFailed
                | EventType::ObservationReady
                | EventType::AgentCompleted
                | EventType::AgentFailed
        )
    }

    /// Map a typed event to its SSE event name. The App Shell
    /// dispatches on this string.
    fn event_name(event_type: quarry_core::event::EventType) -> &'static str {
        use quarry_core::event::EventType;
        match event_type {
            EventType::AgentStarted => "agent.started",
            EventType::ActionStarted => "action.started",
            EventType::ActionCompleted => "action.completed",
            EventType::ActionFailed => "action.failed",
            EventType::ObservationReady => "observation.ready",
            EventType::AgentCompleted => "agent.completed",
            EventType::AgentFailed => "agent.failed",
            // Defensive: every other event type is filtered before
            // this is called, but never panic on a future type.
            _ => "internal",
        }
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

    /// Bounded, redacted decisions made by the browser egress boundary. This
    /// contains neither resolved IPs nor request secrets; it is safe to read
    /// during a live ZDR session but is never a substitute for the immutable
    /// step receipt retained under the run's ZDR policy.
    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct EgressReceiptsData {
        pub receipts: Vec<BrowserEgressReceipt>,
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

    /// An interactive run gives a remote page the ability to initiate further
    /// network activity after the initial URL has been admitted. Entry-point
    /// URL checks are not sufficient for that authority: the selected driver
    /// must prove that every redirect and subresource request remains behind
    /// Quarry's isolated egress boundary. Do this before acquiring a session
    /// so an unsafe provider cannot create durable browser state merely by
    /// starting an agent run.
    fn require_isolated_agent_egress(state: &AppState) -> QuarryResult<()> {
        let capabilities = state.agent_driver.capabilities();
        if capabilities.isolated_egress && capabilities.security_evidence {
            return Ok(());
        }

        Err(QuarryError::new(
            ErrorCode::Unsupported,
            "agent browser execution requires verified isolated egress and current security evidence",
        ))
    }

    fn experimental_lite_enabled() -> bool {
        std::env::var("QUARRY_LITE_EXPERIMENTAL")
            .map(|value| matches!(value.trim(), "1" | "true" | "TRUE" | "yes"))
            .unwrap_or(false)
    }

    /// Enforce the Lite contract independently of whichever renderer is used.
    /// It admits anonymous evidence acquisition and bounded snapshot-based DOM
    /// interaction only; it cannot carry auth state, transfer files, resolve a
    /// host path, or expose a raw page scripting channel.
    fn require_ephemeral_evidence_action(action: &AgentAction) -> QuarryResult<()> {
        if matches!(
            action,
            AgentAction::Navigate { .. }
                | AgentAction::ClickRef { .. }
                | AgentAction::FrameClickRef { .. }
                | AgentAction::TypeRef { .. }
                | AgentAction::FrameTypeRef { .. }
                | AgentAction::SelectRef { .. }
                | AgentAction::FrameSelectRef { .. }
                | AgentAction::Wait { .. }
                | AgentAction::WaitForRef { .. }
                | AgentAction::FrameWaitForRef { .. }
                | AgentAction::Scroll { .. }
                | AgentAction::Screenshot { .. }
                | AgentAction::Pdf
                | AgentAction::GetContent
        ) {
            Ok(())
        } else {
            Err(QuarryError::new(
                ErrorCode::Forbidden,
                "action is unavailable in the ephemeral_evidence execution tier",
            ))
        }
    }

    fn require_driver_capabilities(
        requirements: &BrowserDriverRequirements,
        capabilities: BrowserDriverCapabilities,
    ) -> QuarryResult<()> {
        let mut missing = Vec::new();
        if requirements.persistent_profile && !capabilities.persistent_profile {
            missing.push("persistent_profile");
        }
        if requirements.devtools_trace && !capabilities.devtools_trace {
            missing.push("devtools_trace");
        }
        if requirements.downloads_to_artifacts && !capabilities.downloads_to_artifacts {
            missing.push("downloads_to_artifacts");
        }
        if requirements.uploads_from_artifacts && !capabilities.uploads_from_artifacts {
            missing.push("uploads_from_artifacts");
        }
        if requirements.full_visual_fidelity && !capabilities.full_visual_fidelity {
            missing.push("full_visual_fidelity");
        }
        if requirements.isolated_egress && !capabilities.isolated_egress {
            missing.push("isolated_egress");
        }
        if requirements.security_evidence && !capabilities.security_evidence {
            missing.push("security_evidence");
        }
        if requirements.atomic_target_actions && !capabilities.atomic_target_actions {
            missing.push("atomic_target_actions");
        }
        if missing.is_empty() {
            Ok(())
        } else {
            Err(QuarryError::new(
                ErrorCode::Unsupported,
                "selected browser driver does not satisfy the requested capabilities",
            )
            .with_details(serde_json::json!({ "missing": missing })))
        }
    }

    fn require_action_capabilities(
        action: &AgentAction,
        capabilities: BrowserDriverCapabilities,
    ) -> QuarryResult<()> {
        let uses_snapshot_target = matches!(
            action,
            AgentAction::ClickRef { .. }
                | AgentAction::FrameClickRef { .. }
                | AgentAction::TypeRef { .. }
                | AgentAction::FrameTypeRef { .. }
                | AgentAction::SelectRef { .. }
                | AgentAction::FrameSelectRef { .. }
                | AgentAction::WaitForRef { .. }
                | AgentAction::FrameWaitForRef { .. }
                | AgentAction::ClickSemantic { .. }
                | AgentAction::TypeSemantic { .. }
                | AgentAction::SelectSemantic { .. }
                | AgentAction::WaitForSemantic { .. }
                | AgentAction::UploadRef { .. }
                | AgentAction::FrameUploadRef { .. }
                | AgentAction::DownloadRef { .. }
                | AgentAction::FrameDownloadRef { .. }
        );
        if uses_snapshot_target && !capabilities.atomic_target_actions {
            return Err(QuarryError::new(
                ErrorCode::Unsupported,
                "selected browser driver cannot execute snapshot targets atomically",
            ));
        }
        if matches!(
            action,
            AgentAction::DownloadRef { .. } | AgentAction::FrameDownloadRef { .. }
        ) && !capabilities.downloads_to_artifacts
        {
            return Err(QuarryError::new(
                ErrorCode::Unsupported,
                "selected browser driver cannot provide governed artifact-only downloads",
            ));
        }
        if matches!(
            action,
            AgentAction::UploadRef { .. } | AgentAction::FrameUploadRef { .. }
        ) && !capabilities.uploads_from_artifacts
        {
            return Err(QuarryError::new(
                ErrorCode::Unsupported,
                "selected browser driver cannot provide governed artifact-only uploads",
            ));
        }
        Ok(())
    }

    /// `POST /v1/agent/runs` — acquire a leased browser session for a new run.
    pub async fn start_run(
        State(state): State<AppState>,
        Extension(claims): Extension<crate::auth::Claims>,
        Json(body): Json<StartRunBody>,
    ) -> Result<Json<Envelope<StartRunData>>, ApiErr> {
        let request_id = RequestKind::new().to_string();
        let org_id = claims.org_id.clone();
        let actor_id = claims.actor_id().to_owned();
        let execution_tier = body.execution_tier;

        if execution_tier.is_ephemeral_evidence() && !experimental_lite_enabled() {
            return Err(status_err(
                StatusCode::NOT_FOUND,
                &request_id,
                "ephemeral_evidence is disabled; set QUARRY_LITE_EXPERIMENTAL=1 for the lab",
            ));
        }
        if execution_tier.is_ephemeral_evidence()
            && (body.persist_profile || body.profile_id.is_some() || body.resume_run_id.is_some())
        {
            return Err(status_err(
                StatusCode::BAD_REQUEST,
                &request_id,
                "ephemeral_evidence forbids profiles, persistence, and resume",
            ));
        }

        // Do not treat a provider's top-level navigation guard as proof that
        // page-controlled redirects, frames, fetch/XHR, or DNS rebinding are
        // contained. Until a driver can prove the stronger boundary, agent
        // execution remains unavailable rather than silently unsafe.
        require_isolated_agent_egress(&state).map_err(|error| driver_err(&request_id, error))?;
        require_driver_capabilities(&body.driver_requirements, state.agent_driver.capabilities())
            .map_err(|error| driver_err(&request_id, error))?;

        let resume_run_id = match body.resume_run_id.as_deref() {
            Some(raw) => match raw.parse::<RunKind>() {
                Ok(run_id) => Some(run_id),
                Err(_) => {
                    return Err(status_err(
                        StatusCode::BAD_REQUEST,
                        &request_id,
                        "invalid resume_run_id",
                    ));
                }
            },
            None => None,
        };
        let resume_checkpoint = if let Some(run_id) = resume_run_id.as_ref() {
            let checkpoint = state
                .receipts
                .load_run_checkpoint(&org_id, run_id)
                .await
                .map_err(|error| driver_err(&request_id, error))?;
            let Some(checkpoint) = checkpoint else {
                return Err(status_err(
                    StatusCode::NOT_FOUND,
                    &request_id,
                    "no durable checkpoint exists for resume_run_id",
                ));
            };
            if checkpoint.run_id != run_id.to_string()
                || !caller_owns_browser_run(&checkpoint.org_id, &checkpoint.actor_id, &claims)
            {
                return Err(status_err(
                    StatusCode::NOT_FOUND,
                    &request_id,
                    "agent run not found",
                ));
            }
            if checkpoint.status == AgentRunStatus::Closed {
                return Err(status_err(
                    StatusCode::CONFLICT,
                    &request_id,
                    "agent run is already closed",
                ));
            }
            if checkpoint.zdr.is_active() {
                return Err(status_err(
                    StatusCode::FORBIDDEN,
                    &request_id,
                    "ZDR runs cannot be resumed from durable state",
                ));
            }
            Some(checkpoint)
        } else {
            None
        };

        // A resumed run's persisted constraints are authoritative. This
        // prevents a caller from widening a budget or allowed-domain set after
        // an edge restart.
        let mut constraints = resume_checkpoint
            .as_ref()
            .map(|checkpoint| checkpoint.constraints.clone())
            .unwrap_or_else(|| body.constraints.clone());

        // W4: max_cost_usd is now supported via the flat action-cost table
        // and enforcement in AgentLoop. The blanket reject is removed.
        let grant_id = body.grant_id.clone().or_else(|| {
            resume_checkpoint
                .as_ref()
                .and_then(|cp| cp.grant_id.clone())
        });
        if state.require_browser_grants && grant_id.is_none() {
            return Err(status_err(
                StatusCode::FORBIDDEN,
                &request_id,
                "browser grant_id is required",
            ));
        }
        if let Some(grant_id) = grant_id.as_deref() {
            let grant = state
                .grant_validator
                .validate(grant_id)
                .await
                .map_err(|error| driver_err(&request_id, error))?;
            if !grant.is_usable() {
                return Err(status_err(
                    StatusCode::FORBIDDEN,
                    &request_id,
                    "browser grant is inactive or expired",
                ));
            }
            if state.require_browser_grants {
                let broker_domains = BrowserEgressPolicy::canonical_broker_domains(
                    &grant.allowed_domains,
                )
                .map_err(|reason| status_err(StatusCode::FORBIDDEN, &request_id, &reason))?;
                if resume_checkpoint.is_some() && constraints.allowed_domains != broker_domains {
                    return Err(status_err(
                        StatusCode::FORBIDDEN,
                        &request_id,
                        "resumed run domain policy no longer matches its broker grant",
                    ));
                }
                // At first acquisition, authority is strictly broker-owned:
                // never use domains carried in the Model/client request.
                constraints.allowed_domains = broker_domains;
            }
        }

        let run_id: RunKind = resume_run_id.unwrap_or_else(Id::new);
        let lease_id: LeaseKind = Id::new();
        // Reuse a caller-supplied profile (cookie/session continuity across
        // runs) when provided & parseable; otherwise mint a fresh one.
        let profile_id: ProfileKind = if let Some(checkpoint) = resume_checkpoint.as_ref() {
            checkpoint.profile_id.parse().map_err(|_| {
                status_err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    &request_id,
                    "durable checkpoint has an invalid profile id",
                )
            })?
        } else {
            body.profile_id
                .as_deref()
                .and_then(|s| s.parse().ok())
                .unwrap_or_else(Id::new)
        };
        let zdr = resume_checkpoint
            .as_ref()
            .map(|checkpoint| checkpoint.zdr)
            .unwrap_or_else(|| ZdrMode::from(body.zdr || execution_tier.is_ephemeral_evidence()));
        let ttl_s = constraints.max_runtime_s.unwrap_or(120);
        let viewport = normalize_viewport(
            resume_checkpoint
                .as_ref()
                .and_then(|checkpoint| checkpoint.viewport)
                .or(body.viewport),
        )
        .map_err(|msg| status_err(StatusCode::BAD_REQUEST, &request_id, msg.as_str()))?;

        let persist_profile = resume_checkpoint
            .as_ref()
            .map(|checkpoint| checkpoint.persist_profile)
            .unwrap_or(
                !execution_tier.is_ephemeral_evidence()
                    && (body.persist_profile || body.profile_id.is_some()),
            );
        let profile_scope = resume_checkpoint.as_ref().map_or(
            if execution_tier.is_ephemeral_evidence() {
                BrowserProfileScope::Ephemeral
            } else {
                body.profile_scope
            },
            |checkpoint| checkpoint.profile_scope,
        );
        if zdr_forbids_persistent_profile(zdr, persist_profile, body.profile_id.is_some()) {
            return Err(status_err(
                StatusCode::BAD_REQUEST,
                &request_id,
                "zdr_persistent_profile_forbidden: a ZDR run cannot request a persistent profile or profile_id",
            ));
        }
        if persist_profile && !state.agent_driver.capabilities().persistent_profile {
            return Err(driver_err(
                &request_id,
                QuarryError::new(
                    ErrorCode::Unsupported,
                    "selected browser driver cannot provide a persistent profile",
                ),
            ));
        }

        let lease = BrowserLease {
            lease_id: lease_id.clone(),
            profile_id,
            session_affinity_key: run_id.to_string(),
            // W1 — per-run dedicated proxy identity. The `host` for the
            // sticky derivation is the lease's `session_affinity_key`
            // (the run id), so the same agent run on a different target
            // host keeps the same egress identity for the run's lifetime.
            proxy_affinity: quarry_runtime::proxy_affinity::derive(
                &org_id,
                &run_id.to_string(),
                &run_id.to_string(),
                &PrivacyPolicy::default(),
                &state.proxy_pool_name,
            ),
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

        // Recovery navigation happens before the first ObservationRunner
        // action. Install the broker-derived boundary immediately after lease
        // acquisition so a resumed checkpoint cannot create its first CDP
        // context with the proxy's default, unconstrained policy.
        if let Err(error) = state
            .agent_driver
            .configure_egress_policy(
                &session,
                BrowserEgressPolicy::from_allowed_domains(&constraints.allowed_domains),
            )
            .await
        {
            let _ = state.agent_driver.release(session).await;
            return Err(driver_err(&request_id, error));
        }

        // Rehydrate the last known location when resuming. This is a recovery
        // navigation, not a claimed business action; the next explicit step
        // still receives the first new receipt and observation.
        if let Some(checkpoint) = resume_checkpoint.as_ref() {
            if !checkpoint.current_url.is_empty() {
                let recovery_action = AgentAction::Navigate {
                    url: checkpoint.current_url.clone(),
                };
                if !domain_allowed(&checkpoint.current_url, &constraints.allowed_domains) {
                    let _ = state.agent_driver.release(session).await;
                    return Err(status_err(
                        StatusCode::FORBIDDEN,
                        &request_id,
                        "durable checkpoint URL is outside the persisted allowed domains",
                    ));
                }
                if let Err(error) =
                    validate_navigation(&recovery_action, state.security.as_ref()).await
                {
                    let _ = state.agent_driver.release(session).await;
                    return Err(driver_err(&request_id, error));
                }
                if let Err(error) = state
                    .agent_driver
                    .goto(&session, &checkpoint.current_url)
                    .await
                {
                    let _ = state.agent_driver.release(session).await;
                    return Err(driver_err(&request_id, error));
                }
            }
        }

        let entry = RunEntry {
            run_id: run_id.clone(),
            org_id: org_id.clone(),
            actor_id,
            session,
            ctx: ObservationContext {
                step: resume_checkpoint.as_ref().map_or(0, |cp| cp.step),
                current_url: resume_checkpoint
                    .as_ref()
                    .map_or_else(String::new, |cp| cp.current_url.clone()),
                page_hash: resume_checkpoint
                    .as_ref()
                    .map_or_else(String::new, |cp| cp.page_hash.clone()),
                previous_screenshot: None,
                // A resumed run continues diffing against the checkpointed page
                // identity instead of pretending the recovery navigation is a
                // fresh baseline.
                previous_observation: resume_checkpoint
                    .as_ref()
                    .filter(|cp| !cp.current_url.is_empty() || !cp.page_hash.is_empty())
                    .map(|cp| {
                        ObservationSnapshot::capture(
                            &cp.current_url,
                            None,
                            None,
                            (!cp.page_hash.is_empty()).then_some(cp.page_hash.as_str()),
                        )
                    }),
                previous_network_keys: vec![],
                last_egress_sequence: 0,
                active_snapshot: None,
                observed_action_count: 0,
                challenge_observation_count: 0,
            },
            lease,
            constraints,
            zdr,
            grant_id,
            control_mode: resume_checkpoint
                .as_ref()
                .map_or(BrowserControlMode::AgentControl, |checkpoint| {
                    checkpoint.control_mode
                }),
            profile_scope,
            execution_tier,
            last_observation: None,
            used_sensitive_approval_grants: HashSet::new(),
        };

        if !zdr.is_active() {
            let checkpoint = checkpoint_for_entry(&entry);
            if let Err(error) = state.receipts.save_run_checkpoint(checkpoint).await {
                let _ = state.agent_driver.release(entry.session).await;
                return Err(driver_err(&request_id, error));
            }
            let event = BrowserTimelineEvent::new(
                entry.org_id.clone(),
                entry.actor_id.clone(),
                entry.run_id.to_string(),
                BrowserTimelineEventKind::Lifecycle {
                    state: BrowserTimelineLifecycle::Started,
                },
            );
            if let Err(error) = state.receipts.append_browser_timeline_event(event).await {
                let _ = state.agent_driver.release(entry.session).await;
                return Err(driver_err(&request_id, error));
            }
        }

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
                driver_capabilities: state.agent_driver.capabilities(),
                execution_tier,
                execution_engine: execution_tier.engine_label(),
            },
        )))
    }

    /// `GET /v1/agent/runs/{run_id}/browser-session` — the browser-session
    /// projection owned by Quarry. Live Chromium state is read from the run;
    /// after a restart only the non-ZDR checkpoint projection is available.
    /// Both paths require the verified tenant and the signed initiating actor.
    pub async fn browser_session(
        State(state): State<AppState>,
        Extension(claims): Extension<crate::auth::Claims>,
        Path(run_id): Path<String>,
    ) -> Result<Json<Envelope<BrowserSessionProjectionData>>, ApiErr> {
        let request_id = RequestKind::new().to_string();
        let parsed_run_id = run_id.parse::<RunKind>().map_err(|_| {
            status_err(StatusCode::BAD_REQUEST, &request_id, "invalid agent run id")
        })?;

        let entry_arc = {
            let map = state.agent_runs.lock().expect("agent_runs mutex poisoned");
            map.get(&run_id).cloned()
        };
        if let Some(entry_arc) = entry_arc {
            let entry = entry_arc.lock().await;
            if !caller_owns_live_run(&entry, &claims) {
                return Err(status_err(
                    StatusCode::NOT_FOUND,
                    &request_id,
                    "agent run not found",
                ));
            }
            let tabs = state
                .agent_driver
                .list_tabs(&entry.session)
                .await
                .map_err(|error| driver_err(&request_id, error))?;
            return Ok(Json(Envelope::ok(
                request_id,
                live_browser_session_projection(&entry, tabs),
            )));
        }

        let checkpoint = state
            .receipts
            .load_run_checkpoint(&claims.org_id, &parsed_run_id)
            .await
            .map_err(|error| driver_err(&request_id, error))?;
        let Some(checkpoint) = checkpoint else {
            return Err(status_err(
                StatusCode::NOT_FOUND,
                &request_id,
                "agent run not found",
            ));
        };
        if !caller_owns_browser_run(&checkpoint.org_id, &checkpoint.actor_id, &claims) {
            return Err(status_err(
                StatusCode::NOT_FOUND,
                &request_id,
                "agent run not found",
            ));
        }

        Ok(Json(Envelope::ok(
            request_id,
            checkpoint_browser_session_projection(checkpoint),
        )))
    }

    /// `POST /v1/agent/runs/{run_id}/browser-session/control` — transfer
    /// browser input authority. Quarry owns this transition, stamps it with
    /// the verified actor, and appends a privacy-bounded audit event. ZDR
    /// runs may be controlled live but never receive durable history.
    pub async fn set_browser_session_control(
        State(state): State<AppState>,
        Extension(claims): Extension<crate::auth::Claims>,
        Path(run_id): Path<String>,
        Json(body): Json<BrowserSessionControlBody>,
    ) -> Result<Json<Envelope<BrowserSessionProjectionData>>, ApiErr> {
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
        if !caller_owns_live_run(&entry, &claims) {
            return Err(status_err(
                StatusCode::NOT_FOUND,
                &request_id,
                "agent run not found",
            ));
        }

        entry.control_mode = body.mode;
        record_browser_timeline_event(
            &state,
            &entry,
            BrowserTimelineEventKind::Control {
                mode: body.mode,
                initiated_by: BrowserTimelineInitiator::Human,
            },
        )
        .await
        .map_err(|error| driver_err(&request_id, error))?;

        let tabs = state
            .agent_driver
            .list_tabs(&entry.session)
            .await
            .map_err(|error| driver_err(&request_id, error))?;
        Ok(Json(Envelope::ok(
            request_id,
            live_browser_session_projection(&entry, tabs),
        )))
    }

    /// `GET /v1/agent/runs/{run_id}/browser-session/timeline` — a bounded,
    /// cursor-paginated owner timeline. It combines actor-bound action
    /// receipts with Quarry's compact control/tab/devtools/lifecycle audit
    /// records; neither stream contains raw browser contents.
    pub async fn browser_session_timeline(
        State(state): State<AppState>,
        Extension(claims): Extension<crate::auth::Claims>,
        Path(run_id): Path<String>,
        Query(query): Query<BrowserTimelineQuery>,
    ) -> Result<Json<Envelope<BrowserTimelineData>>, ApiErr> {
        let request_id = RequestKind::new().to_string();
        let parsed_run_id = run_id.parse::<RunKind>().map_err(|_| {
            status_err(StatusCode::BAD_REQUEST, &request_id, "invalid agent run id")
        })?;
        let entry_arc = {
            let map = state.agent_runs.lock().expect("agent_runs mutex poisoned");
            map.get(&run_id).cloned()
        };
        let (org_id, actor_id, zdr) = if let Some(entry_arc) = entry_arc {
            let entry = entry_arc.lock().await;
            if !caller_owns_live_run(&entry, &claims) {
                return Err(status_err(
                    StatusCode::NOT_FOUND,
                    &request_id,
                    "agent run not found",
                ));
            }
            (entry.org_id.clone(), entry.actor_id.clone(), entry.zdr)
        } else {
            let checkpoint = state
                .receipts
                .load_run_checkpoint(&claims.org_id, &parsed_run_id)
                .await
                .map_err(|error| driver_err(&request_id, error))?;
            let Some(checkpoint) = checkpoint else {
                return Err(status_err(
                    StatusCode::NOT_FOUND,
                    &request_id,
                    "agent run not found",
                ));
            };
            if !caller_owns_browser_run(&checkpoint.org_id, &checkpoint.actor_id, &claims) {
                return Err(status_err(
                    StatusCode::NOT_FOUND,
                    &request_id,
                    "agent run not found",
                ));
            }
            (checkpoint.org_id, checkpoint.actor_id, checkpoint.zdr)
        };
        if zdr.is_active() {
            return Err(status_err(
                StatusCode::CONFLICT,
                &request_id,
                "browser timeline is unavailable for a Zero Data Retention run",
            ));
        }

        let receipts = state
            .receipts
            .list_for_actor(&org_id, &actor_id, &parsed_run_id)
            .await
            .map_err(|error| driver_err(&request_id, error))?;
        let events = state
            .receipts
            .list_browser_timeline_events(&org_id, &actor_id, &parsed_run_id)
            .await
            .map_err(|error| driver_err(&request_id, error))?;
        let mut items = receipts
            .into_iter()
            .map(browser_timeline_action)
            .chain(events.into_iter().map(browser_timeline_event))
            .collect::<Vec<_>>();
        items.sort_by(|left, right| timeline_sort_key(left).cmp(&timeline_sort_key(right)));

        let start = if let Some(cursor) = query.cursor.as_deref() {
            let Some(index) = items.iter().position(|item| item.id == cursor) else {
                return Err(status_err(
                    StatusCode::BAD_REQUEST,
                    &request_id,
                    "browser timeline cursor is invalid",
                ));
            };
            index.saturating_add(1)
        } else {
            0
        };
        let limit = query.limit.unwrap_or(50).clamp(1, 100);
        let page = items
            .into_iter()
            .skip(start)
            .take(limit.saturating_add(1))
            .collect::<Vec<_>>();
        let has_more = page.len() > limit;
        let mut page = page;
        if has_more {
            let _ = page.pop();
        }
        let next_cursor = has_more.then(|| {
            page.last()
                .expect("non-empty browser timeline page when has_more")
                .id
                .clone()
        });

        Ok(Json(Envelope::ok(
            request_id,
            BrowserTimelineData {
                items: page,
                next_cursor,
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
        fn provider_meter_cost_is_copied_exactly_and_unknown_remains_unknown_in_telemetry() {
            let unknown = BrowserTelemetry::default();
            assert_eq!(unknown.verified_action_cost_micro_usd, None);
            assert_eq!(telemetry_receipt_cost_usd(&unknown), 0.0);

            let metered = BrowserTelemetry {
                verified_action_cost_micro_usd: Some(12_345),
                ..BrowserTelemetry::default()
            };
            assert!((telemetry_receipt_cost_usd(&metered) - 0.012_345).abs() < f64::EPSILON);
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
        fn browser_session_projection_requires_exact_tenant_and_actor() {
            let claims = crate::auth::Claims {
                sub: "user-owner".to_owned(),
                iss: "https://auth.example.test".to_owned(),
                exp: i64::MAX,
                org_id: "org-owner".to_owned(),
                user_id: "user-owner".to_owned(),
                principal_type: Some("user".to_owned()),
                service_id: None,
                nbf: None,
                aud: Some("quarry".to_owned()),
                scopes: Vec::new(),
            };

            assert!(caller_owns_browser_run("org-owner", "user-owner", &claims));
            assert!(!caller_owns_browser_run("org-owner", "user-other", &claims));
            assert!(!caller_owns_browser_run("org-other", "user-owner", &claims));
            assert!(!caller_owns_browser_run("org-owner", "", &claims));
        }

        #[test]
        fn allowed_domains_accept_exact_and_subdomains_only() {
            let allowed = vec!["example.com".to_owned()];
            assert!(domain_allowed("https://example.com/path", &allowed));
            assert!(domain_allowed("https://docs.example.com/path", &allowed));
            assert!(!domain_allowed(
                "https://example.com.evil.test/path",
                &allowed
            ));
            assert!(!domain_allowed("https://other.test/path", &allowed));
        }

        #[test]
        fn step_budget_is_fail_closed_once_limit_is_reached() {
            let mut entry = RunEntry {
                run_id: Id::new(),
                org_id: "org".into(),
                actor_id: "user".into(),
                    session: BrowserSession {
                        lease: BrowserLease {
                            lease_id: Id::new(),
                            profile_id: Id::new(),
                            session_affinity_key: "test-session".into(),
                            proxy_affinity: Default::default(),
                            ttl_s: 60,
                            capabilities: vec![],
                            artifact_bucket: String::new(),
                            persist_profile: false,
                            viewport: None,
                        org_id: "org".into(),
                    },
                    inner: Arc::new(TokioMutex::new(SessionInner::default())),
                },
                ctx: ObservationContext {
                    step: 2,
                    current_url: String::new(),
                    page_hash: String::new(),
                    previous_screenshot: None,
                    previous_observation: None,
                    previous_network_keys: vec![],
                    last_egress_sequence: 0,
                    active_snapshot: None,
                    observed_action_count: 0,
                    challenge_observation_count: 0,
                },
                lease: BrowserLease {
                    lease_id: Id::new(),
                    profile_id: Id::new(),
                    session_affinity_key: "test".into(),
                    proxy_affinity: Default::default(),
                    ttl_s: 60,
                    capabilities: vec![],
                    artifact_bucket: String::new(),
                    persist_profile: false,
                    viewport: None,
                    org_id: "org".into(),
                },
                constraints: AgentConstraints {
                    max_steps: 2,
                    allowed_domains: vec![],
                    max_runtime_s: None,
                    max_cost_usd: None,
                },
                zdr: ZdrMode::Off,
                grant_id: None,
                control_mode: BrowserControlMode::AgentControl,
                profile_scope: BrowserProfileScope::Ephemeral,
                execution_tier: BrowserExecutionTier::Chromium,
                last_observation: None,
                used_sensitive_approval_grants: HashSet::new(),
            };
            assert!(enforce_step_budget(&entry).is_err());
            entry.constraints.max_steps = 0;
            assert!(enforce_step_budget(&entry).is_ok());
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
        if !caller_owns_live_run(&entry, &claims) {
            return Err(status_err(
                StatusCode::NOT_FOUND,
                &request_id,
                "agent run not found",
            ));
        }

        if let Some(grant_id) = entry.grant_id.as_deref() {
            validate_run_grant(&state, grant_id, &entry.constraints.allowed_domains)
                .await
                .map_err(|error| driver_err(&request_id, error))?;
        } else if state.require_browser_grants {
            return Err(status_err(
                StatusCode::FORBIDDEN,
                &request_id,
                "browser grant is required for every action",
            ));
        }

        require_action_capabilities(&body.action, state.agent_driver.capabilities())
            .map_err(|error| driver_err(&request_id, error))?;
        if entry.execution_tier.is_ephemeral_evidence() {
            require_ephemeral_evidence_action(&body.action)
                .map_err(|error| driver_err(&request_id, error))?;
        }

        if let Some(approval) = sensitive_approval_for_action(&entry, &body.action)
            .map_err(|error| driver_err(&request_id, error))?
        {
            validate_sensitive_approval(&state, &approval)
                .await
                .map_err(|error| driver_err(&request_id, error))?;
            if !entry
                .used_sensitive_approval_grants
                .insert(approval.grant_id)
            {
                return Err(status_err(
                    StatusCode::CONFLICT,
                    &request_id,
                    "sensitive browser approval grant has already been used by this run",
                ));
            }
        }

        enforce_step_budget(&entry)
            .map_err(|message| status_err(StatusCode::TOO_MANY_REQUESTS, &request_id, message))?;

        if let AgentAction::Navigate { url } = &body.action {
            if !domain_allowed(url, &entry.constraints.allowed_domains) {
                return Err(status_err(
                    StatusCode::FORBIDDEN,
                    &request_id,
                    "browser navigation is outside the run's allowed domains",
                ));
            }
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
            extraction_profile: body.extraction_profile,
        };
        let receipt_step = entry.ctx.step;
        let receipt_run_id = entry.run_id.to_string();
        let receipt_action = req.action.clone();

        let runner = ObservationRunner {
            browser: state.agent_driver.clone(),
            artifacts: Some(state.artifacts.clone()),
            events: Some(state.event_sink.clone()),
            visual_processor: state.visual_processor.clone(),
            // The run's org (already checked against `claims.org_id` above) is
            // the tenant every observation artifact is stamped with.
            org_id: entry.org_id.clone(),
        };

        // Disjoint borrows: &session (shared) + &mut ctx (exclusive).
        let entry_mut: &mut RunEntry = &mut entry;
        // A completed browser call is not automatically a verified business
        // effect; the observation carries that distinction. Persist an
        // immutable receipt for both success and failure so a planner or
        // reviewer can replay the exact action sequence.
        let obs = match runner
            .execute(&req, &entry_mut.session, &mut entry_mut.ctx)
            .await
        {
            Ok(obs) => {
                // W4: when the driver omits a provider-billed cost, fall
                // back to the flat action-cost table so max_cost_usd
                // budgets are enforceable.
                let mut cost_usd = observation_receipt_cost_usd(&obs);
                if cost_usd == 0.0 {
                    cost_usd = estimated_action_cost_usd(&receipt_action);
                }
                let receipt = ReceiptBuilder::start(receipt_run_id.clone(), receipt_step)
                    .org_id(entry.org_id.clone())
                    .actor_id(entry.actor_id.clone())
                    .complete(
                        receipt_action,
                        Some(obs.clone()),
                        cost_usd,
                    );
                if !entry.zdr.is_active() {
                    state
                        .receipts
                        .append(receipt)
                        .await
                        .map_err(|e| driver_err(&request_id, e))?;
                }
                obs
            }
            Err(error) => {
                let receipt = ReceiptBuilder::start(receipt_run_id, receipt_step)
                    .org_id(entry.org_id.clone())
                    .actor_id(entry.actor_id.clone())
                    .fail(receipt_action, &error);
                if !entry.zdr.is_active() {
                    state
                        .receipts
                        .append(receipt)
                        .await
                        .map_err(|e| driver_err(&request_id, e))?;
                }
                return Err(driver_err(&request_id, error));
            }
        };

        entry.last_observation = Some(obs.clone());

        // Persist the continuation only after the immutable outcome receipt is
        // durable. If this write fails, surface an internal error rather than
        // letting the caller continue with a browser state that cannot be
        // recovered after a crash.
        if !entry.zdr.is_active() {
            let checkpoint = checkpoint_for_entry(&entry);
            state
                .receipts
                .save_run_checkpoint(checkpoint)
                .await
                .map_err(|error| driver_err(&request_id, error))?;
        }

        if !domain_allowed(&obs.url, &entry.constraints.allowed_domains) {
            return Err(status_err(
                StatusCode::FORBIDDEN,
                &request_id,
                "browser action reached a domain outside the run's allowed domains",
            ));
        }

        Ok(Json(Envelope::ok(request_id, obs)))
    }

    /// `GET /v1/agent/runs/{run_id}/receipts` — immutable action history for
    /// replay, audit, and human review. The run's verified tenant owns the
    /// receipt stream; no caller-supplied org id is accepted.
    pub async fn list_receipts(
        State(state): State<AppState>,
        Extension(claims): Extension<crate::auth::Claims>,
        Path(run_id): Path<String>,
    ) -> Result<Json<Envelope<Vec<StepReceipt>>>, ApiErr> {
        let request_id = RequestKind::new().to_string();
        let entry_arc = {
            let map = state.agent_runs.lock().expect("agent_runs mutex poisoned");
            map.get(&run_id).cloned()
        };
        let receipts = if let Some(entry_arc) = entry_arc {
            let entry = entry_arc.lock().await;
            if !caller_owns_live_run(&entry, &claims) {
                return Err(status_err(
                    StatusCode::NOT_FOUND,
                    &request_id,
                    "agent run not found",
                ));
            }
            state
                .receipts
                .list_for_actor(&entry.org_id, &entry.actor_id, &entry.run_id)
                .await
                .map_err(|e| driver_err(&request_id, e))?
        } else {
            // Receipts are durable independently of the live Chromium
            // session. An actor-scoped predicate is required even after an
            // edge restart; legacy rows without this binding remain hidden.
            let parsed_run_id = run_id.parse::<RunKind>().map_err(|_| {
                status_err(StatusCode::NOT_FOUND, &request_id, "agent run not found")
            })?;
            let receipts = state
                .receipts
                .list_for_actor(&claims.org_id, claims.actor_id(), &parsed_run_id)
                .await
                .map_err(|e| driver_err(&request_id, e))?;
            if receipts.is_empty() {
                return Err(status_err(
                    StatusCode::NOT_FOUND,
                    &request_id,
                    "agent run not found",
                ));
            }
            receipts
        };
        Ok(Json(Envelope::ok(request_id, receipts)))
    }

    /// `POST /v1/agent/runs/{run_id}/procedure` — compile a deterministic
    /// replay candidate from a tenant-owned verified receipt stream. Unknown,
    /// failed, or unobserved effects are rejected by the runtime compiler;
    /// Model Plane remains the planner and policy owner.
    pub async fn compile_run_procedure(
        State(state): State<AppState>,
        Extension(claims): Extension<crate::auth::Claims>,
        Path(run_id): Path<String>,
        Json(body): Json<CompileProcedureBody>,
    ) -> Result<Json<Envelope<BrowserProcedure>>, ApiErr> {
        let request_id = RequestKind::new().to_string();
        let parsed_run_id = run_id
            .parse::<RunKind>()
            .map_err(|_| status_err(StatusCode::NOT_FOUND, &request_id, "agent run not found"))?;
        let receipts = state
            .receipts
            .list_for_actor(&claims.org_id, claims.actor_id(), &parsed_run_id)
            .await
            .map_err(|error| driver_err(&request_id, error))?;
        if receipts.is_empty() {
            return Err(status_err(
                StatusCode::NOT_FOUND,
                &request_id,
                "agent run has no durable receipts",
            ));
        }
        let procedure_id = body
            .procedure_id
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| format!("proc_{run_id}"));
        let procedure = compile_procedure(procedure_id, &receipts)
            .map_err(|error| driver_err(&request_id, error))?;
        Ok(Json(Envelope::ok(request_id, procedure)))
    }

    /// `POST /v1/agent/procedures/replay-check` — compare a proposed action
    /// sequence with a compiled procedure without executing browser effects.
    /// Any mismatch is an explicit repair/refusal decision, never a silent
    /// selector or business-action substitution.
    pub async fn replay_check(
        Extension(_claims): Extension<crate::auth::Claims>,
        Json(body): Json<ReplayCheckBody>,
    ) -> Json<Envelope<ReplayDecision>> {
        let request_id = RequestKind::new().to_string();
        Json(Envelope::ok(
            request_id,
            compare_replay(&body.procedure, &body.actions),
        ))
    }

    /// `POST /v1/agent/procedures/quality-check` — deterministic promotion
    /// report. This never changes rollout state; it supplies evidence for the
    /// owning release/Model workflow.
    pub async fn procedure_quality_check(
        Extension(_claims): Extension<crate::auth::Claims>,
        Json(procedure): Json<BrowserProcedure>,
    ) -> Json<Envelope<ProcedureQualityReport>> {
        let request_id = RequestKind::new().to_string();
        Json(Envelope::ok(request_id, assess_quality(&procedure)))
    }

    /// `POST /v1/agent/procedures/impact-check` — conservative change impact
    /// analysis. A changed source URL quarantines affected navigation steps;
    /// Quarry does not silently replay or self-heal effectful procedures.
    pub async fn procedure_impact_check(
        Extension(_claims): Extension<crate::auth::Claims>,
        Json(body): Json<ProcedureImpactBody>,
    ) -> Json<Envelope<ProcedureImpactReport>> {
        let request_id = RequestKind::new().to_string();
        Json(Envelope::ok(
            request_id,
            analyze_impact(&body.procedure, &body.changed_urls),
        ))
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
        if !state.agent_driver.capabilities().full_visual_fidelity {
            return Err(driver_err(
                &request_id,
                QuarryError::new(
                    ErrorCode::Unsupported,
                    "selected browser driver cannot produce governed full-fidelity live frames",
                ),
            ));
        }
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
        if !caller_owns_live_run(&entry, &claims) {
            return Err(status_err(
                StatusCode::NOT_FOUND,
                &request_id,
                "agent run not found",
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

    /// `GET /v1/agent/runs/{run_id}/live-view` — single-shot live-view
    /// reference. W2 — the App Shell calls this on connect and
    /// again whenever the `live_view` SSE event's `expires_at`
    /// elapses, so the iframe URL rotates without polling the
    /// frame stream.
    ///
    /// Returns 200 with the `LiveViewRef` (or `null` when the driver
    /// has no live-view URL — the local Chromiumoxide fallback).
    /// Returns 404 when the run is unknown or not owned by the
    /// caller. The success shape is the same `LiveViewRef` the SSE
    /// `live_view` event emits, byte-for-byte.
    pub async fn live_view(
        State(state): State<AppState>,
        Extension(claims): Extension<crate::auth::Claims>,
        Path(run_id): Path<String>,
    ) -> Result<Json<Envelope<Option<quarry_core::driver_meta::LiveViewRef>>>, ApiErr> {
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
        if !caller_owns_live_run(&entry, &claims) {
            return Err(status_err(
                StatusCode::NOT_FOUND,
                &request_id,
                "agent run not found",
            ));
        }
        let lv = state
            .agent_driver
            .live_view(&entry.session)
            .await
            .map_err(|e| driver_err(&request_id, e))?;
        Ok(Json(Envelope::ok(request_id, lv)))
    }

    /// `GET /v1/agent/runs/{run_id}/events/stream` — W3 — typed
    /// agent-event SSE. Sends the same `quarry_core::event::Event`
    /// envelope the `EventSink` already broadcasts, in the order the
    /// agent loop emitted them, until the run closes. The App Shell
    /// uses this to render the action timeline next to the frame
    /// stream.
    ///
    /// Filtering: `internal.*` and `telemetry.*` event types are
    /// dropped. The agent loop only emits a small set of
    /// human-facing event types (`agent.*`, `action.*`,
    /// `observation.ready`) so the filter is currently a no-op,
    /// but it documents the wire contract for future telemetry
    /// events that the agent loop may add without surfacing.
    ///
    /// Reconnect: `?from_seq=N` replays from the durable event
    /// history first (Postgres `quarry_event_history` when
    /// configured, no-op otherwise), then streams the live tail.
    pub async fn agent_events_stream(
        State(state): State<AppState>,
        Extension(claims): Extension<crate::auth::Claims>,
        Path(run_id): Path<String>,
        Query(query): Query<AgentEventsQuery>,
    ) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>>, ApiErr> {
        use quarry_core::event::EventType;

        let request_id = RequestKind::new().to_string();
        let parsed_run: RunKind = run_id
            .parse()
            .map_err(|e: quarry_core::error::QuarryError| {
                status_err(StatusCode::BAD_REQUEST, &request_id, &e.message)
            })?;
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
        if !caller_owns_live_run(&entry, &claims) {
            return Err(status_err(
                StatusCode::NOT_FOUND,
                &request_id,
                "agent run not found",
            ));
        }
        drop(entry);

        // Replay the durable tail (best-effort: if the postgres
        // history backend is not configured, the replay emits no
        // events and the live tail still starts immediately).
        // NOTE: postgres-queue JobHistoryEvent replay is wired
        // separately via `GET /v1/runs/:id/events?after_seq=`; the
        // agent live tail here is intentionally live-only so we
        // don't conflate JobHistoryEvent<->Event types during the
        // fleet promotion. `?from_seq=` remains accepted for
        // forward-compat but is currently a no-op live tail.
        let mut history_rx: Option<
            std::pin::Pin<Box<dyn futures_util::Stream<Item = quarry_core::event::Event> + Send>>,
        > = None;
        let _ = &query.from_seq;

        let mut live_rx = state.event_sink.subscribe(&parsed_run);
        let max_events = query.max_events.unwrap_or(0);
        let mut emitted: u64 = 0;

        let stream = async_stream::stream! {
            // Drain the replay tail first (if any). The replay
            // stream yields events already in seq order.
            if let Some(replay) = history_rx.as_mut() {
                use futures_util::StreamExt;
                while let Some(evt) = replay.next().await {
                    if is_user_facing_event(evt.event_type) {
                        if max_events > 0 && emitted >= max_events {
                            yield Ok(Event::default().event("done").data("max_events"));
                            return;
                        }
                        emitted += 1;
                        if let Ok(event) = Event::default()
                            .event(event_name(evt.event_type))
                            .id(evt.seq.to_string())
                            .json_data(&evt)
                        {
                            yield Ok(event);
                        }
                    }
                }
            }
            // Then stream the live tail.
            loop {
                if max_events > 0 && emitted >= max_events {
                    yield Ok(Event::default().event("done").data("max_events"));
                    break;
                }
                match live_rx.recv().await {
                    Ok(evt) => {
                        if !is_user_facing_event(evt.event_type) {
                            continue;
                        }
                        emitted += 1;
                        let name = event_name(evt.event_type);
                        let evt_for_log = evt.clone();
                        match Event::default()
                            .event(name)
                            .id(evt.seq.to_string())
                            .json_data(&evt)
                        {
                            Ok(event) => yield Ok(event),
                            Err(err) => {
                                tracing::warn!(
                                    error = %err,
                                    ?evt_for_log,
                                    "agent events stream: failed to serialize event; skipping"
                                );
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        // Slow consumer: tell the App Shell to
                        // reconnect with `?from_seq=N` to backfill
                        // the gap. We keep streaming from the next
                        // available event so a real-time-only
                        // consumer still sees fresh data.
                        yield Ok(Event::default()
                            .event("lagged")
                            .data(format!("{{\"skipped\":{skipped}}}")));
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        // Run closed. The terminal `agent.completed` /
                        // `agent.failed` event is already in the
                        // history; emit `done` so the App Shell
                        // closes its EventSource.
                        yield Ok(Event::default().event("done").data("run_closed"));
                        break;
                    }
                }
            }
        };

        Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
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
        if !state.agent_driver.capabilities().full_visual_fidelity {
            return Err(driver_err(
                &request_id,
                QuarryError::new(
                    ErrorCode::Unsupported,
                    "selected browser driver cannot produce governed full-fidelity live frames",
                ),
            ));
        }
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
            if !caller_owns_live_run(&entry, &claims) {
                return Err(status_err(
                    StatusCode::NOT_FOUND,
                    &request_id,
                    "agent run not found",
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
            // W2 — emit the live-view reference as the first event so
            // the App Shell can decide whether to render the cloud
            // session (Browserbase/Browserless/Kernel) or fall back
            // to the local frame stream (Chromiumoxide). Cloud
            // providers return a URL with an `expires_at`; the
            // App Shell triggers a `/v1/agent/runs/{id}/live-view`
            // refresh when that deadline elapses.
            if let Ok(Some(ref lv)) = driver.live_view(&session).await {
                match Event::default().event("live_view").json_data(lv) {
                    Ok(event) => yield Ok(event),
                    Err(_) => {
                        // Live-view is best-effort; missing it must
                        // not stop the frame stream. The App Shell
                        // just falls back to the screenshot frames.
                    }
                }
            }
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
        if !state.agent_driver.capabilities().full_visual_fidelity {
            return Err(driver_err(
                &request_id,
                QuarryError::new(
                    ErrorCode::Unsupported,
                    "selected browser driver cannot produce governed full-fidelity live frames",
                ),
            ));
        }
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
            if !caller_owns_live_run(&entry, &claims) {
                return Err(status_err(
                    StatusCode::NOT_FOUND,
                    &request_id,
                    "agent run not found",
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
        if let Some(grant_id) = entry.grant_id.as_deref() {
            validate_run_grant(state, grant_id, &entry.constraints.allowed_domains).await?;
        } else if state.require_browser_grants {
            return Err(QuarryError::new(
                ErrorCode::Forbidden,
                "browser grant is required for every action",
            ));
        }
        require_action_capabilities(&action, state.agent_driver.capabilities())?;
        if entry.execution_tier.is_ephemeral_evidence() {
            require_ephemeral_evidence_action(&action)?;
        }
        if let Some(approval) = sensitive_approval_for_action(&entry, &action)? {
            validate_sensitive_approval(state, &approval).await?;
            if !entry
                .used_sensitive_approval_grants
                .insert(approval.grant_id)
            {
                return Err(QuarryError::new(
                    ErrorCode::Conflict,
                    "sensitive browser approval grant has already been used by this run",
                ));
            }
        }
        enforce_step_budget(&entry)
            .map_err(|message| QuarryError::new(ErrorCode::RateLimited, message))?;
        if let AgentAction::Navigate { url } = &action {
            if !domain_allowed(url, &entry.constraints.allowed_domains) {
                return Err(QuarryError::new(
                    ErrorCode::SecurityBlocked,
                    "browser navigation is outside the run's allowed domains",
                ));
            }
        }
        let req = AgentActionRequest {
            run_id: entry.run_id.clone(),
            lease_id: entry.lease.lease_id.clone(),
            action,
            instruction,
            constraints: entry.constraints.clone(),
            zdr: entry.zdr,
            extraction_profile: None,
        };
        let receipt_step = entry.ctx.step;
        let receipt_action = req.action.clone();
        let receipt_run_id = entry.run_id.to_string();
        let runner = ObservationRunner {
            browser: state.agent_driver.clone(),
            artifacts: Some(state.artifacts.clone()),
            events: Some(state.event_sink.clone()),
            visual_processor: state.visual_processor.clone(),
            // The run's org (already checked against `claims.org_id` above) is
            // the tenant every observation artifact is stamped with.
            org_id: entry.org_id.clone(),
        };
        let entry_mut: &mut RunEntry = &mut entry;
        let observation = match runner
            .execute(&req, &entry_mut.session, &mut entry_mut.ctx)
            .await
        {
            Ok(observation) => {
                if !entry.zdr.is_active() {
                    let receipt = ReceiptBuilder::start(receipt_run_id.clone(), receipt_step)
                        .org_id(entry.org_id.clone())
                        .actor_id(entry.actor_id.clone())
                        .complete(
                            receipt_action.clone(),
                            Some(observation.clone()),
                            observation_receipt_cost_usd(&observation),
                        );
                    state.receipts.append(receipt).await?;
                }
                observation
            }
            Err(error) => {
                if !entry.zdr.is_active() {
                    let receipt = ReceiptBuilder::start(receipt_run_id, receipt_step)
                        .org_id(entry.org_id.clone())
                        .actor_id(entry.actor_id.clone())
                        .fail(receipt_action, &error);
                    state.receipts.append(receipt).await?;
                }
                return Err(error);
            }
        };
        entry.last_observation = Some(observation.clone());
        if !entry.zdr.is_active() {
            state
                .receipts
                .save_run_checkpoint(checkpoint_for_entry(&entry))
                .await?;
        }
        if !domain_allowed(&observation.url, &entry.constraints.allowed_domains) {
            return Err(QuarryError::new(
                ErrorCode::SecurityBlocked,
                "browser action reached a domain outside the run's allowed domains",
            ));
        }
        Ok(observation)
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
        if !caller_owns_live_run(&entry, &claims) {
            return Err(status_err(
                StatusCode::NOT_FOUND,
                &request_id,
                "agent run not found",
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
        if !caller_owns_live_run(&entry, &claims) {
            return Err(status_err(
                StatusCode::NOT_FOUND,
                &request_id,
                "agent run not found",
            ));
        }

        if entry.execution_tier.is_ephemeral_evidence() {
            return Err(status_err(
                StatusCode::FORBIDDEN,
                &request_id,
                "the ephemeral_evidence execution tier does not permit creating browser tabs",
            ));
        }

        // Opening a tab is an effectful browser operation in its own right;
        // do not let it bypass the per-action broker revalidation that normal
        // REST/WebSocket steps receive.
        if let Some(grant_id) = entry.grant_id.as_deref() {
            validate_run_grant(&state, grant_id, &entry.constraints.allowed_domains)
                .await
                .map_err(|error| driver_err(&request_id, error))?;
        } else if state.require_browser_grants {
            return Err(status_err(
                StatusCode::FORBIDDEN,
                &request_id,
                "browser grant is required for tab creation",
            ));
        }

        if let Some(url) = body.url.as_deref() {
            if !domain_allowed(url, &entry.constraints.allowed_domains) {
                return Err(status_err(
                    StatusCode::FORBIDDEN,
                    &request_id,
                    "browser tab navigation is outside the run's allowed domains",
                ));
            }
            validate_navigation(
                &AgentAction::Navigate {
                    url: url.to_owned(),
                },
                state.security.as_ref(),
            )
            .await
            .map_err(|error| driver_err(&request_id, error))?;
        }

        // Tab creation bypasses ObservationRunner, so explicitly apply the
        // same run-scoped policy before this browser-owned navigation. The
        // Chromium Fetch listener then enforces it for redirects and every
        // request the new page initiates.
        state
            .agent_driver
            .configure_egress_policy(
                &entry.session,
                BrowserEgressPolicy::from_allowed_domains(&entry.constraints.allowed_domains),
            )
            .await
            .map_err(|error| driver_err(&request_id, error))?;

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
        record_browser_timeline_event(
            &state,
            &entry,
            BrowserTimelineEventKind::Tab {
                operation: BrowserTabOperation::Opened,
                tab_id: Some(tab.tab_id.clone()),
                active_tab_id: active_browser_tab_id(&tabs),
            },
        )
        .await
        .map_err(|error| driver_err(&request_id, error))?;

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
        if !caller_owns_live_run(&entry, &claims) {
            return Err(status_err(
                StatusCode::NOT_FOUND,
                &request_id,
                "agent run not found",
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
        record_browser_timeline_event(
            &state,
            &entry,
            BrowserTimelineEventKind::Tab {
                operation: BrowserTabOperation::Selected,
                tab_id: Some(tab_id),
                active_tab_id: active_browser_tab_id(&tabs),
            },
        )
        .await
        .map_err(|error| driver_err(&request_id, error))?;

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
        if !caller_owns_live_run(&entry, &claims) {
            return Err(status_err(
                StatusCode::NOT_FOUND,
                &request_id,
                "agent run not found",
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
        record_browser_timeline_event(
            &state,
            &entry,
            BrowserTimelineEventKind::Tab {
                operation: BrowserTabOperation::Closed,
                tab_id: Some(tab_id),
                active_tab_id: active_browser_tab_id(&tabs),
            },
        )
        .await
        .map_err(|error| driver_err(&request_id, error))?;

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
        if !caller_owns_live_run(&entry, &claims) {
            return Err(status_err(
                StatusCode::NOT_FOUND,
                &request_id,
                "agent run not found",
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
        if let Some(last) = events.last() {
            record_browser_timeline_event(
                &state,
                &entry,
                BrowserTimelineEventKind::Devtools {
                    event_count: events.len().min(u32::MAX as usize) as u32,
                    last_sequence: last.sequence,
                },
            )
            .await
            .map_err(|error| driver_err(&request_id, error))?;
        }

        Ok(Json(Envelope::ok(
            request_id,
            DevtoolsEventsData {
                events,
                zdr: matches!(entry.zdr, ZdrMode::On),
            },
        )))
    }

    /// `GET /v1/agent/runs/{run_id}/egress-receipts` — retrieve the
    /// driver-authored, redacted request decisions for a live browser run.
    pub async fn egress_receipts(
        State(state): State<AppState>,
        Extension(claims): Extension<crate::auth::Claims>,
        Path(run_id): Path<String>,
        Query(query): Query<DevtoolsQuery>,
    ) -> Result<Json<Envelope<EgressReceiptsData>>, ApiErr> {
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
        if !caller_owns_live_run(&entry, &claims) {
            return Err(status_err(
                StatusCode::NOT_FOUND,
                &request_id,
                "agent run not found",
            ));
        }
        let receipts = state
            .agent_driver
            .egress_receipts(
                &entry.session,
                query.after_sequence.unwrap_or(0),
                query.limit.unwrap_or(100).clamp(1, 512),
            )
            .await
            .map_err(|error| driver_err(&request_id, error))?;

        Ok(Json(Envelope::ok(
            request_id,
            EgressReceiptsData {
                receipts,
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
            if !caller_owns_live_run(&entry, &claims) {
                return Err(status_err(
                    StatusCode::NOT_FOUND,
                    &request_id,
                    "agent run not found",
                ));
            }
        }

        let close_event = {
            let entry = entry_arc.lock().await;
            (!entry.zdr.is_active()).then(|| {
                BrowserTimelineEvent::new(
                    entry.org_id.clone(),
                    entry.actor_id.clone(),
                    entry.run_id.to_string(),
                    BrowserTimelineEventKind::Lifecycle {
                        state: BrowserTimelineLifecycle::Closed,
                    },
                )
            })
        };
        if let Some(event) = close_event {
            let parsed_run_id = run_id.parse::<RunKind>().map_err(|_| {
                status_err(StatusCode::NOT_FOUND, &request_id, "agent run not found")
            })?;
            state
                .receipts
                .append_browser_timeline_event(event)
                .await
                .map_err(|error| driver_err(&request_id, error))?;
            state
                .receipts
                .close_run_checkpoint(&claims.org_id, &parsed_run_id)
                .await
                .map_err(|error| driver_err(&request_id, error))?;
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
            // `interact` is an alias for one governed action, preserving Quarry's one-step
            // receipt and approval semantics instead of adding an unbounded
            // macro executor.
            .route("/v1/agent/runs/:run_id/interact", post(step))
            .route(
                "/v1/agent/runs/:run_id/browser-session",
                get(browser_session),
            )
            .route(
                "/v1/agent/runs/:run_id/browser-session/control",
                post(set_browser_session_control),
            )
            .route(
                "/v1/agent/runs/:run_id/browser-session/timeline",
                get(browser_session_timeline),
            )
            .route(
                "/v1/agent/runs/:run_id/egress-receipts",
                get(egress_receipts),
            )
            .route("/v1/agent/runs/:run_id/receipts", get(list_receipts))
            .route(
                "/v1/agent/runs/:run_id/procedure",
                post(compile_run_procedure),
            )
            .route("/v1/agent/procedures/replay-check", post(replay_check))
            .route(
                "/v1/agent/procedures/quality-check",
                post(procedure_quality_check),
            )
            .route(
                "/v1/agent/procedures/impact-check",
                post(procedure_impact_check),
            )
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
            // W2 — single-shot live-view reference (no SSE). The
            // App Shell calls this on connect and again whenever
            // the `live_view` SSE event's `expires_at` elapses.
            .route("/v1/agent/runs/:run_id/live-view", get(live_view))
            // W3 — typed agent-event SSE alongside the frame
            // stream. The App Shell renders the action timeline
            // (`action.started` / `action.completed` / `action.failed`),
            // planner decisions (`observation.ready`), and run
            // lifecycle (`agent.started` / `agent.completed` /
            // `agent.failed`) on the agent console. The frame
            // stream is the live-view channel; this stream is the
            // typed-events channel. Both are scoped to the same run
            // and owned by the same caller, so they share the
            // tenant-scoping helper.
            .route(
                "/v1/agent/runs/:run_id/events/stream",
                get(agent_events_stream),
            )
            .route("/v1/agent/runs/:run_id", delete(close_run))
    }
}

/// No-op router when the `browser-agent` feature is disabled, so `routes.rs`
/// can unconditionally `.merge(crate::agent_routes::agent_router())`.
#[cfg(not(feature = "browser-agent"))]
pub fn agent_router() -> axum::Router<crate::state::AppState> {
    axum::Router::new()
}
