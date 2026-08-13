//! Observation protocol — executes an AgentActionRequest, produces a BrowserObservation.

use std::{
    collections::{BTreeSet, HashMap},
    io::Write,
    sync::Arc,
    time::Instant,
};

use chrono::Utc;
use quarry_browser::actions::{Action, ScrollTarget};
use quarry_browser::{
    BrowserDevtoolsEvent, BrowserDownloadedFile, BrowserDriver, BrowserEgressPolicy,
    BrowserNativeProjection, BrowserNativeTarget, VerifiedTargetAction, VerifiedTargetOperation,
};
use quarry_core::contracts::{
    ActionOutcome, AgentAction, AgentActionRequest, BrowserObservation, BrowserSnapshot,
    BrowserTelemetry, ChallengeKind, ChallengeSignal, ConsoleLine, DomEvidenceDelta, DomSummary,
    ElementFingerprint, EvidenceDelta, ExtractionFieldResult, ExtractionProfile, ExtractionResult,
    ExtractionSource, InteractiveElement, NetworkEntry, NetworkEvidenceDelta, ObservationDelta,
    ProofBundle, ResolvedTargetProof, SemanticLocator, SnapshotTarget,
};
use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_core::event::EventType;
use quarry_core::ids::kinds::ArtifactKind;
use quarry_core::zdr::{self, WriteKind, ZdrMode};
use serde_json::{json, Value};

use crate::artifact_store::ArtifactStore;
use crate::events::EventSink;
use crate::vision::{VisualChangeArtifact, VisualObservationInput, VisualObservationProcessor};

pub struct ObservationRunner {
    pub browser: Arc<dyn BrowserDriver>,
    pub artifacts: Option<Arc<dyn ArtifactStore>>,
    pub events: Option<EventSink>,
    pub visual_processor: Option<Arc<dyn VisualObservationProcessor>>,
    /// Verified org of the run these observations belong to. Every artifact
    /// written here (screenshots, traces, visual diffs) is stamped with it so
    /// only this tenant can read the bytes back by id. Empty means the run has
    /// no tenant attribution — such artifacts stay write-only.
    pub org_id: String,
}

pub struct ObservationContext {
    pub step: u32,
    pub current_url: String,
    pub page_hash: String,
    /// Fingerprint from the last completed observation. This is separate from
    /// `page_hash`, which is used while an action is executing for artifact
    /// attribution.
    pub previous_page_hash: Option<String>,
    pub previous_screenshot: Option<Vec<u8>>,
    pub previous_url: Option<String>,
    pub previous_title: Option<String>,
    pub previous_dom_node_count: Option<u32>,
    /// Redacted request identity keys from the prior observation. Kept only in
    /// memory and never contains a query string or request body.
    pub previous_network_keys: Vec<String>,
    /// Highest egress receipt incorporated into an observation. Keeping this
    /// cursor run-local makes each receipt appear exactly once in the durable
    /// step trail while allowing bounded reads from a busy browser session.
    pub last_egress_sequence: u64,
    /// Kept in process only. Resuming a run starts with no active snapshot, so
    /// a caller must re-observe before attempting a snapshot-bound action.
    pub active_snapshot: Option<ActiveSnapshot>,
    /// Observation counters remain run-local. They deliberately reset on
    /// resume, matching the snapshot and prior-network lifecycle.
    pub observed_action_count: u32,
    pub challenge_observation_count: u32,
}

#[derive(Debug, Clone)]
pub struct ActiveSnapshot {
    snapshot: BrowserSnapshot,
    targets: HashMap<String, ActiveSnapshotTarget>,
}

#[derive(Debug, Clone)]
struct ActiveSnapshotTarget {
    /// A selector is a compatibility binding only. Chromium-native snapshots
    /// leave this empty and execute through `native_target` instead.
    selector: Option<String>,
    native_target: Option<BrowserNativeTarget>,
    target: SnapshotTarget,
}

impl ObservationRunner {
    /// Browser uploads are a sensitive data-disclosure operation. The cap is
    /// intentionally independent of the artifact store so a permissive
    /// backend cannot make a single action stage arbitrary amounts of data.
    const MAX_UPLOAD_ARTIFACT_BYTES: usize = 25 * 1024 * 1024;
    const MAX_DOWNLOAD_ARTIFACT_BYTES: usize = 25 * 1024 * 1024;

    /// Classify access/challenge pages without attempting to bypass them. The
    /// signal is deliberately conservative and evidence-bearing so Model
    /// Plane can decide whether to ask for a user handoff or use another
    /// source. It is not a success/failure verdict for the business action.
    pub fn classify_challenge(
        url: &str,
        title: Option<&str>,
        text: Option<&str>,
    ) -> Option<ChallengeSignal> {
        let haystack = format!(
            "{} {} {}",
            url,
            title.unwrap_or_default(),
            text.unwrap_or_default()
        )
        .to_ascii_lowercase();
        let (kind, evidence) = if ["captcha", "recaptcha", "hcaptcha", "verify you are human"]
            .iter()
            .any(|needle| haystack.contains(needle))
        {
            (ChallengeKind::Captcha, vec!["captcha_marker".to_owned()])
        } else if [
            "access denied",
            "forbidden",
            "blocked by",
            "status code 403",
        ]
        .iter()
        .any(|needle| haystack.contains(needle))
        {
            (
                ChallengeKind::AccessDenied,
                vec!["access_denied_marker".to_owned()],
            )
        } else if ["sign in", "log in", "login required", "/login"]
            .iter()
            .any(|needle| haystack.contains(needle))
        {
            (ChallengeKind::Login, vec!["login_marker".to_owned()])
        } else if ["cookie consent", "accept cookies", "consent required"]
            .iter()
            .any(|needle| haystack.contains(needle))
        {
            (ChallengeKind::Consent, vec!["consent_marker".to_owned()])
        } else if ["too many requests", "rate limit", "status code 429"]
            .iter()
            .any(|needle| haystack.contains(needle))
        {
            (
                ChallengeKind::RateLimit,
                vec!["rate_limit_marker".to_owned()],
            )
        } else {
            return None;
        };
        Some(ChallengeSignal {
            kind,
            confidence: 0.98,
            evidence,
            requires_escalation: true,
        })
    }

    fn outcome_for_action(action: &AgentAction, current_url: &str) -> ActionOutcome {
        match action {
            AgentAction::Navigate { .. } => {
                if current_url.is_empty() {
                    ActionOutcome::unknown(
                        "navigation_url_unavailable",
                        "the browser did not expose a final URL after navigation",
                    )
                } else {
                    ActionOutcome::verified("navigation_completed")
                }
            }
            AgentAction::Screenshot { .. } | AgentAction::Pdf | AgentAction::GetContent => {
                ActionOutcome::verified("artifact_or_content_captured")
            }
            AgentAction::WaitFor { .. }
            | AgentAction::WaitForRef { .. }
            | AgentAction::FrameWaitForRef { .. }
            | AgentAction::WaitForSemantic { .. } => ActionOutcome::verified("selector_observed"),
            AgentAction::Wait { .. } => ActionOutcome::verified("wait_completed"),
            AgentAction::RespondDialog { .. } => ActionOutcome::verified("dialog_response_sent"),
            AgentAction::UploadRef { .. } | AgentAction::FrameUploadRef { .. } => ActionOutcome::unknown(
                "postcondition_required",
                "the file was attached to the exact input, but the page has not confirmed submission",
            ),
            AgentAction::DownloadRef { .. } | AgentAction::FrameDownloadRef { .. } => ActionOutcome::verified("download_captured_as_artifact"),
            AgentAction::Click { .. }
            | AgentAction::ClickRef { .. }
            | AgentAction::FrameClickRef { .. }
            | AgentAction::ClickSemantic { .. }
            | AgentAction::ClickPoint { .. }
            | AgentAction::Type { .. }
            | AgentAction::TypeRef { .. }
            | AgentAction::FrameTypeRef { .. }
            | AgentAction::TypeSemantic { .. }
            | AgentAction::Press { .. }
            | AgentAction::Scroll { .. }
            | AgentAction::MouseWheel { .. }
            | AgentAction::Select { .. }
            | AgentAction::SelectRef { .. }
            | AgentAction::FrameSelectRef { .. }
            | AgentAction::SelectSemantic { .. }
            | AgentAction::Evaluate { .. }
            | AgentAction::Back
            | AgentAction::Forward => ActionOutcome::unknown(
                "postcondition_required",
                "the browser operation completed, but its business effect was not verified",
            ),
        }
    }

    pub fn agent_action_to_browser_action(action: &AgentAction) -> QuarryResult<Action> {
        Ok(match action {
            AgentAction::Navigate { url } => Action::Navigate { url: url.clone() },
            AgentAction::Click { selector } => Action::Click {
                selector: selector.clone(),
            },
            AgentAction::ClickRef { .. }
            | AgentAction::FrameClickRef { .. }
            | AgentAction::ClickSemantic { .. }
            | AgentAction::TypeRef { .. }
            | AgentAction::FrameTypeRef { .. }
            | AgentAction::TypeSemantic { .. }
            | AgentAction::SelectRef { .. }
            | AgentAction::FrameSelectRef { .. }
            | AgentAction::SelectSemantic { .. }
            | AgentAction::WaitForRef { .. }
            | AgentAction::FrameWaitForRef { .. }
            | AgentAction::WaitForSemantic { .. }
            | AgentAction::UploadRef { .. }
            | AgentAction::FrameUploadRef { .. }
            | AgentAction::DownloadRef { .. }
            | AgentAction::FrameDownloadRef { .. } => {
                return Err(QuarryError::new(
                    ErrorCode::TargetRepairRequired,
                    "snapshot actions require ObservationContext target resolution",
                ));
            }
            AgentAction::RespondDialog { .. } => {
                return Err(QuarryError::new(
                    ErrorCode::Unsupported,
                    "dialog responses are executed through the governed observation path",
                ));
            }
            AgentAction::ClickPoint { x, y } => Action::ClickPoint { x: *x, y: *y },
            AgentAction::Type { selector, text } => Action::Type {
                selector: selector.clone(),
                text: text.clone(),
            },
            AgentAction::Press { key } => Action::Press { key: key.clone() },
            AgentAction::Scroll { target } => Action::Scroll {
                to: ScrollTarget::Selector(target.clone()),
            },
            AgentAction::MouseWheel {
                x,
                y,
                delta_x,
                delta_y,
            } => Action::MouseWheel {
                x: *x,
                y: *y,
                delta_x: *delta_x,
                delta_y: *delta_y,
            },
            AgentAction::Select { selector, value } => Action::Select {
                selector: selector.clone(),
                value: value.clone(),
            },
            AgentAction::Wait { ms } => Action::Wait { ms: *ms },
            AgentAction::WaitFor {
                selector,
                timeout_ms,
            } => Action::WaitFor {
                selector: selector.clone(),
                timeout_ms: *timeout_ms,
            },
            AgentAction::Screenshot { full_page } => Action::Screenshot {
                full_page: *full_page,
            },
            AgentAction::Pdf => Action::Pdf,
            AgentAction::Evaluate { script } => Action::Evaluate {
                script: script.clone(),
            },
            AgentAction::Back => Action::Back,
            AgentAction::Forward => Action::Forward,
            AgentAction::GetContent => Action::Navigate { url: String::new() },
        })
    }

    pub async fn execute(
        &self,
        request: &AgentActionRequest,
        session: &quarry_browser::BrowserSession,
        ctx: &mut ObservationContext,
    ) -> QuarryResult<BrowserObservation> {
        let observation_started_at = Instant::now();
        let run_id = &request.run_id;
        // This must happen inside the shared execution path, not just the
        // REST handler: browser WebSocket actions and any future internal
        // caller use the same run-scoped policy before a page can make an
        // effectful request.
        self.browser
            .configure_egress_policy(
                session,
                BrowserEgressPolicy::from_allowed_domains(&request.constraints.allowed_domains),
            )
            .await?;
        if action_requires_atomic_target(&request.action)
            && !self.browser.capabilities().atomic_target_actions
        {
            return Err(QuarryError::new(
                ErrorCode::Unsupported,
                "selected browser driver cannot atomically verify and execute snapshot targets",
            ));
        }
        if matches!(
            request.action,
            AgentAction::Screenshot { .. } | AgentAction::Pdf
        ) && !self.browser.capabilities().full_visual_fidelity
        {
            return Err(QuarryError::new(
                ErrorCode::Unsupported,
                "selected browser driver cannot produce governed full-fidelity visual evidence",
            ));
        }
        let (effective_action, target_resolution, verified_target_action, transfer_target) = self
            .resolve_snapshot_action(&request.action, session, ctx)
            .await?;

        if let Some(events) = &self.events {
            events
                .emit_for_zdr(
                    request.zdr,
                    run_id.clone(),
                    EventType::ActionStarted,
                    json!({ "step": ctx.step, "action": serde_json::to_value(&request.action).ok() }),
                    format!("{}:action:{}:started", run_id, ctx.step),
                )
                .await;
        }

        let mut screenshot_artifact_id: Option<ArtifactKind> = None;
        let mut visual_observation_artifact_id: Option<ArtifactKind> = None;
        let mut download_artifact_id: Option<ArtifactKind> = None;
        let policy_denials: Vec<String> = Vec::new();
        let mut current_screenshot: Option<Vec<u8>> = None;

        if let Some(verified_target_action) = verified_target_action {
            self.browser
                .act_on_verified_target(session, verified_target_action)
                .await?;
        } else {
            match &effective_action {
                AgentAction::Navigate { url } => {
                    self.browser.goto(session, url).await?;
                    ctx.current_url = url.clone();
                }
                AgentAction::Click { selector } => {
                    self.browser.click(session, selector).await?;
                }
                AgentAction::ClickPoint { x, y } => {
                    self.browser.click_point(session, *x, *y).await?;
                }
                AgentAction::Type { selector, text } => {
                    self.browser.type_text(session, selector, text).await?;
                }
                AgentAction::Press { key } => {
                    self.browser.press(session, key).await?;
                }
                AgentAction::Scroll { target } => {
                    self.browser
                        .scroll(session, &ScrollTarget::Selector(target.clone()))
                        .await?;
                }
                AgentAction::MouseWheel {
                    x,
                    y,
                    delta_x,
                    delta_y,
                } => {
                    self.browser
                        .mouse_wheel(session, *x, *y, *delta_x, *delta_y)
                        .await?;
                }
                AgentAction::Select { selector, value } => {
                    self.browser.select(session, selector, value).await?;
                }
                AgentAction::Wait { ms } => {
                    tokio::time::sleep(std::time::Duration::from_millis(*ms as u64)).await;
                }
                AgentAction::WaitFor {
                    selector,
                    timeout_ms,
                } => {
                    self.browser
                        .wait_for(session, selector, *timeout_ms)
                        .await?;
                }
                AgentAction::RespondDialog {
                    dialog_id,
                    accept,
                    prompt_text,
                    ..
                } => {
                    self.browser
                        .respond_dialog(session, dialog_id, *accept, prompt_text.as_deref())
                        .await?;
                }
                AgentAction::UploadRef { artifact_id, .. } => {
                    let target = transfer_target.clone().ok_or_else(|| {
                        QuarryError::new(
                            ErrorCode::TargetRepairRequired,
                            "upload requires an executable native file-input target; observe again",
                        )
                    })?;
                    self.upload_artifact_to_target(request.zdr, artifact_id, session, target)
                        .await?;
                }
                AgentAction::DownloadRef { .. } => {
                    if request.zdr.is_active() {
                        return Err(QuarryError::new(
                            ErrorCode::SecurityBlocked,
                            "artifact-only downloads are unavailable in zero-data-retention mode",
                        ));
                    }
                    if self.artifacts.is_none() || self.org_id.is_empty() {
                        return Err(QuarryError::new(
                            ErrorCode::RuntimeNotReady,
                            "artifact storage and verified tenant identity are required for downloads",
                        ));
                    }
                    let target = transfer_target.clone().ok_or_else(|| {
                        QuarryError::new(
                            ErrorCode::TargetRepairRequired,
                            "download requires an executable native target; observe again",
                        )
                    })?;
                    let download = self
                        .browser
                        .download_from_verified_target(session, target)
                        .await?;
                    self.validate_download_admission(&download)?;
                    download_artifact_id = Some(
                        self.put_artifact_if_allowed(
                            request.zdr,
                            run_id,
                            &ctx.page_hash,
                            "browser_download",
                            download.bytes.to_vec(),
                        )
                        .await?
                        .ok_or_else(|| {
                            QuarryError::new(
                                ErrorCode::RuntimeNotReady,
                                "browser download could not be promoted to artifact storage",
                            )
                        })?,
                    );
                }
                AgentAction::Screenshot { full_page } => {
                    let bytes = self.browser.screenshot(session, *full_page).await?;
                    let payload = bytes.to_vec();
                    screenshot_artifact_id = self
                        .put_artifact_if_allowed(
                            request.zdr,
                            run_id,
                            &ctx.page_hash,
                            "screenshot",
                            payload.clone(),
                        )
                        .await?;
                    current_screenshot = Some(payload);
                }
                AgentAction::Pdf => {
                    let bytes = self.browser.pdf(session).await?;
                    let _ = self
                        .put_artifact_if_allowed(
                            request.zdr,
                            run_id,
                            &ctx.page_hash,
                            "pdf",
                            bytes.to_vec(),
                        )
                        .await?;
                }
                AgentAction::Evaluate { script } => {
                    self.browser.evaluate(session, script).await?;
                }
                AgentAction::Back => {
                    self.browser.back(session).await?;
                }
                AgentAction::Forward => {
                    self.browser.forward(session).await?;
                }
                AgentAction::GetContent => {
                    let _content = self.browser.content(session).await?;
                }
                AgentAction::ClickRef { .. }
                | AgentAction::FrameClickRef { .. }
                | AgentAction::ClickSemantic { .. }
                | AgentAction::TypeRef { .. }
                | AgentAction::FrameTypeRef { .. }
                | AgentAction::TypeSemantic { .. }
                | AgentAction::SelectRef { .. }
                | AgentAction::FrameSelectRef { .. }
                | AgentAction::SelectSemantic { .. }
                | AgentAction::WaitForRef { .. }
                | AgentAction::FrameWaitForRef { .. }
                | AgentAction::WaitForSemantic { .. }
                | AgentAction::FrameUploadRef { .. }
                | AgentAction::FrameDownloadRef { .. } => {
                    return Err(QuarryError::new(
                        ErrorCode::TargetRepairRequired,
                        "snapshot target was not resolved before browser execution",
                    ));
                }
            }
        }

        let page_state = self.page_state(session).await;
        if let Some(url) = page_state.url.as_deref().filter(|url| !url.is_empty()) {
            ctx.current_url = url.to_owned();
        }
        let devtools_events = self
            .browser
            .devtools_events(session, 0, 512)
            .await
            .unwrap_or_default();
        // Drivers that expose a central egress boundary provide bounded,
        // redacted receipts here. Unsupported drivers produce an empty list;
        // the agent-run admission gate refuses them until they can prove
        // isolated egress, so an empty list is never treated as proof.
        let egress_receipts = match self
            .browser
            .egress_receipts(session, ctx.last_egress_sequence, 512)
            .await
        {
            Ok(receipts) => receipts,
            // A driver without a receipt authority remains source-compatible
            // for non-agent use, but cannot be admitted for governed agent
            // runs. Do not turn an actual receipt-authority failure into an
            // apparently clean, empty audit trail.
            Err(error) if error.code == ErrorCode::Unsupported => Vec::new(),
            Err(error) => return Err(error),
        };
        if let Some(last_receipt) = egress_receipts.last() {
            ctx.last_egress_sequence = ctx.last_egress_sequence.max(last_receipt.sequence);
        }
        let dialogs = self.browser.dialogs(session).await.unwrap_or_default();

        let html_bytes = self.browser.content(session).await.ok();
        if let Some(bytes) = html_bytes.as_ref() {
            // Proof must identify the bytes actually observed, not the
            // synthetic run hash used before the first page is loaded.
            ctx.page_hash = format!("blake3:{}", blake3::hash(bytes).to_hex());
        }

        let dom_summary = html_bytes.as_ref().map(|bytes| {
            let html_str = String::from_utf8_lossy(bytes);
            build_dom_summary(&html_str)
        });
        let native_projection = match self.browser.native_page_projection(session).await {
            Ok(projection) => Some(projection),
            // Older/read-only drivers can continue to expose the explicitly
            // marked compatibility snapshot. A driver that advertises native
            // browser operation must not silently downgrade because its AX
            // query failed mid-run.
            Err(error) if error.code == ErrorCode::Unsupported => None,
            Err(error) => return Err(error),
        };
        let active_snapshot = (dom_summary.is_some() || native_projection.is_some()).then(|| {
            build_active_snapshot(
                run_id,
                ctx.step,
                &ctx.page_hash,
                dom_summary.as_ref(),
                native_projection,
            )
        });
        let snapshot = active_snapshot.as_ref().map(|state| state.snapshot.clone());
        ctx.active_snapshot = active_snapshot;

        let title = page_state.title.or_else(|| {
            html_bytes.as_ref().and_then(|bytes| {
                let html_str = String::from_utf8_lossy(bytes);
                extract_title(&html_str)
            })
        });

        let extraction_profile = request.extraction_profile.clone();
        let extraction_result = extraction_profile.as_ref().and_then(|profile| {
            html_bytes
                .as_ref()
                .and_then(|bytes| extract_profile(bytes, profile))
        });

        if screenshot_artifact_id.is_none() {
            if let Ok(bytes) = self.browser.screenshot(session, false).await {
                let payload = bytes.to_vec();
                if let Ok(id) = self
                    .put_artifact_if_allowed(
                        request.zdr,
                        run_id,
                        &ctx.page_hash,
                        "screenshot",
                        payload.clone(),
                    )
                    .await
                {
                    screenshot_artifact_id = id;
                }
                current_screenshot = Some(payload);
            }
        }

        if let Some(current) = current_screenshot.clone() {
            if matches!(request.zdr, ZdrMode::Off) {
                if let Some(processor) = &self.visual_processor {
                    match processor
                        .observe(VisualObservationInput {
                            run_id: run_id.to_string(),
                            page_hash: ctx.page_hash.clone(),
                            step: ctx.step,
                            previous_png: ctx.previous_screenshot.clone(),
                            current_png: current.clone(),
                        })
                        .await
                    {
                        Ok(mut result) => {
                            if let Some(annotated_png) = result.annotated_png {
                                if let Ok(Some(id)) = self
                                    .put_artifact_if_allowed(
                                        request.zdr,
                                        run_id,
                                        &ctx.page_hash,
                                        "screenshot_annotated",
                                        annotated_png,
                                    )
                                    .await
                                {
                                    result.artifact.annotated_artifact_id = Some(id.to_string());
                                    attach_related(
                                        &mut result.artifact,
                                        "screenshot_annotated",
                                        &id,
                                    );
                                }
                            }
                            if let Some(clean_png) = result.clean_png {
                                if let Ok(Some(id)) = self
                                    .put_artifact_if_allowed(
                                        request.zdr,
                                        run_id,
                                        &ctx.page_hash,
                                        "page_image_clean",
                                        clean_png,
                                    )
                                    .await
                                {
                                    attach_related(&mut result.artifact, "page_image_clean", &id);
                                }
                            }
                            if let Some(thumbnail_png) = result.thumbnail_png {
                                if let Ok(Some(id)) = self
                                    .put_artifact_if_allowed(
                                        request.zdr,
                                        run_id,
                                        &ctx.page_hash,
                                        "thumbnail",
                                        thumbnail_png,
                                    )
                                    .await
                                {
                                    attach_related(&mut result.artifact, "thumbnail", &id);
                                }
                            }
                            if let Some(tiles) = result.tiles {
                                if let Ok(Some(id)) = self
                                    .put_json_artifact_if_allowed(
                                        request.zdr,
                                        run_id,
                                        &ctx.page_hash,
                                        "tiles",
                                        &tiles,
                                    )
                                    .await
                                {
                                    attach_related(&mut result.artifact, "tiles", &id);
                                }
                            }
                            if let Some(ocr_png) = result.ocr_preprocessed_png {
                                if let Ok(Some(id)) = self
                                    .put_artifact_if_allowed(
                                        request.zdr,
                                        run_id,
                                        &ctx.page_hash,
                                        "ocr_preprocessed",
                                        ocr_png,
                                    )
                                    .await
                                {
                                    attach_related(&mut result.artifact, "ocr_preprocessed", &id);
                                }
                            }
                            if let Some(logo_png) = result.logo_candidate_png {
                                if let Ok(Some(id)) = self
                                    .put_artifact_if_allowed(
                                        request.zdr,
                                        run_id,
                                        &ctx.page_hash,
                                        "logo_candidate",
                                        logo_png,
                                    )
                                    .await
                                {
                                    attach_related(&mut result.artifact, "logo_candidate", &id);
                                }
                            }
                            if let Some(palette) = result.rendered_palette {
                                if let Ok(Some(id)) = self
                                    .put_json_artifact_if_allowed(
                                        request.zdr,
                                        run_id,
                                        &ctx.page_hash,
                                        "rendered_palette",
                                        &palette,
                                    )
                                    .await
                                {
                                    attach_related(&mut result.artifact, "rendered_palette", &id);
                                }
                            }

                            let visual_change = VisualChangeArtifact {
                                version: result.artifact.version,
                                backend: result.artifact.backend.clone(),
                                step: result.artifact.step,
                                previous_available: result.artifact.previous_available,
                                changed: result.artifact.changed,
                                change_ratio: result.artifact.change_ratio,
                                regions: result.artifact.regions.clone(),
                                metrics: result.artifact.metrics.clone(),
                                annotated_artifact_id: result
                                    .artifact
                                    .annotated_artifact_id
                                    .clone(),
                            };
                            if let Ok(Some(id)) = self
                                .put_json_artifact_if_allowed(
                                    request.zdr,
                                    run_id,
                                    &ctx.page_hash,
                                    "visual_change",
                                    &visual_change,
                                )
                                .await
                            {
                                result.artifact.change_artifact_id = Some(id.to_string());
                                attach_related(&mut result.artifact, "visual_change", &id);
                            }

                            match self
                                .put_json_artifact_if_allowed(
                                    request.zdr,
                                    run_id,
                                    &ctx.page_hash,
                                    "visual_observation",
                                    &result.artifact,
                                )
                                .await
                            {
                                Ok(id) => visual_observation_artifact_id = id,
                                Err(e) => tracing::warn!(
                                    error = %e,
                                    "visual observation artifact write failed"
                                ),
                            }
                        }
                        Err(e) => tracing::warn!(
                            error = %e,
                            "visual observation processor failed"
                        ),
                    }
                }
            }

            ctx.previous_screenshot = if matches!(request.zdr, ZdrMode::Off) {
                Some(current)
            } else {
                None
            };
        }

        let delta = observation_delta(
            ctx.previous_url.as_deref(),
            ctx.previous_title.as_deref(),
            ctx.previous_dom_node_count,
            ctx.previous_page_hash.as_deref(),
            &ctx.current_url,
            title.as_deref(),
            dom_summary.as_ref().map(|summary| summary.node_count),
            (!ctx.page_hash.is_empty()).then_some(ctx.page_hash.as_str()),
        );
        let network_summary = network_summary_from_devtools(&devtools_events);
        let network_delta = network_evidence_delta(&ctx.previous_network_keys, &network_summary);
        let evidence_delta = EvidenceDelta {
            version: 1,
            step: ctx.step,
            dom: DomEvidenceDelta {
                previous_node_count: ctx.previous_dom_node_count,
                current_node_count: dom_summary.as_ref().map(|summary| summary.node_count),
                changed: delta.dom_changed || delta.content_changed,
                snapshot_target_count: snapshot
                    .as_ref()
                    .map_or(0, |snapshot| snapshot.targets.len() as u32),
            },
            network: network_delta,
            visual_observation_artifact_id: visual_observation_artifact_id
                .as_ref()
                .map(ToString::to_string),
        };
        let evidence_delta_artifact_id = self
            .put_json_artifact_if_allowed(
                request.zdr,
                run_id,
                &ctx.page_hash,
                "evidence_delta",
                &evidence_delta,
            )
            .await
            .unwrap_or_else(|error| {
                tracing::warn!(error = %error, "evidence delta artifact write failed");
                None
            });
        let action_outcome = Self::outcome_for_action(&request.action, &ctx.current_url);
        let challenge = Self::classify_challenge(
            &ctx.current_url,
            title.as_deref(),
            dom_summary
                .as_ref()
                .and_then(|summary| summary.text_snippet.as_deref()),
        );
        ctx.observed_action_count = ctx.observed_action_count.saturating_add(1);
        if challenge.is_some() {
            ctx.challenge_observation_count = ctx.challenge_observation_count.saturating_add(1);
        }
        let driver_telemetry = match self.browser.telemetry(session).await {
            Ok(telemetry) => telemetry,
            Err(error) => {
                // A telemetry sample must not turn a completed browser action
                // into a retryable effect. Keep the unknown/default fields and
                // retain the real execution outcome separately.
                tracing::warn!(error = %error, "browser telemetry sample failed");
                BrowserTelemetry::default()
            }
        };
        let challenge_rate_per_mille = if ctx.observed_action_count == 0 {
            0
        } else {
            let rate = u64::from(ctx.challenge_observation_count).saturating_mul(1_000)
                / u64::from(ctx.observed_action_count);
            rate.min(u64::from(u16::MAX)) as u16
        };
        let telemetry = BrowserTelemetry {
            usable_observation_latency_ms: elapsed_ms(observation_started_at),
            estimated_snapshot_tokens: estimated_snapshot_tokens(snapshot.as_ref()),
            observed_action_count: ctx.observed_action_count,
            challenge_observation_count: ctx.challenge_observation_count,
            challenge_rate_per_mille,
            ..driver_telemetry
        };
        let mut artifact_ids = Vec::new();
        if let Some(id) = screenshot_artifact_id.clone() {
            artifact_ids.push(id);
        }
        if let Some(id) = visual_observation_artifact_id.clone() {
            artifact_ids.push(id);
        }
        if let Some(id) = evidence_delta_artifact_id.clone() {
            artifact_ids.push(id);
        }
        if let Some(id) = download_artifact_id.clone() {
            artifact_ids.push(id);
        }
        let proof_bundle = Some(ProofBundle {
            proof_id: format!("proof_{}_{}", run_id, ctx.step),
            source_url: ctx.current_url.clone(),
            run_id: run_id.clone(),
            step: ctx.step,
            action_outcome: action_outcome.clone(),
            artifact_ids,
            content_fingerprint: (!ctx.page_hash.is_empty()).then(|| ctx.page_hash.clone()),
            challenge: challenge.clone(),
            target_resolution: target_resolution.clone(),
            evidence_delta_artifact_id: evidence_delta_artifact_id.clone(),
            observed_at: Utc::now(),
        });
        let observation = BrowserObservation {
            run_id: run_id.clone(),
            step: ctx.step,
            url: ctx.current_url.clone(),
            title,
            snapshot,
            dom_summary,
            screenshot_artifact_id,
            visual_observation_artifact_id,
            evidence_delta_artifact_id,
            console_summary: console_summary_from_devtools(&devtools_events),
            network_summary,
            egress_receipts,
            dialogs,
            policy_denials,
            action_outcome,
            observation_delta: Some(delta),
            challenge,
            extraction_profile,
            extraction_result,
            proof_bundle,
            target_resolution,
            telemetry,
            observed_at: Utc::now(),
        };

        ctx.previous_url = Some(observation.url.clone());
        ctx.previous_title = observation.title.clone();
        ctx.previous_dom_node_count = observation
            .dom_summary
            .as_ref()
            .map(|summary| summary.node_count);
        ctx.previous_page_hash = (!ctx.page_hash.is_empty()).then(|| ctx.page_hash.clone());
        ctx.previous_network_keys = observation
            .network_summary
            .iter()
            .map(redacted_network_key)
            .collect();

        if let Some(events) = &self.events {
            events
                .emit_for_zdr(
                    request.zdr,
                    run_id.clone(),
                    EventType::ObservationReady,
                    json!({
                        "step": ctx.step,
                        "url": &observation.url,
                        "has_screenshot": observation.screenshot_artifact_id.is_some(),
                        "has_evidence_delta": observation.evidence_delta_artifact_id.is_some(),
                    }),
                    format!("{}:observation:{}:ready", run_id, ctx.step),
                )
                .await;
        }

        ctx.step += 1;
        Ok(observation)
    }

    async fn resolve_snapshot_action(
        &self,
        action: &AgentAction,
        session: &quarry_browser::BrowserSession,
        ctx: &ObservationContext,
    ) -> QuarryResult<(
        AgentAction,
        Option<ResolvedTargetProof>,
        Option<VerifiedTargetAction>,
        Option<BrowserNativeTarget>,
    )> {
        let (snapshot_id, generation, ref_id, locator) = match action {
            AgentAction::ClickRef {
                snapshot_id,
                generation,
                ref_id,
            }
            | AgentAction::FrameClickRef {
                snapshot_id,
                generation,
                ref_id,
                ..
            }
            | AgentAction::TypeRef {
                snapshot_id,
                generation,
                ref_id,
                ..
            }
            | AgentAction::FrameTypeRef {
                snapshot_id,
                generation,
                ref_id,
                ..
            }
            | AgentAction::SelectRef {
                snapshot_id,
                generation,
                ref_id,
                ..
            }
            | AgentAction::FrameSelectRef {
                snapshot_id,
                generation,
                ref_id,
                ..
            }
            | AgentAction::WaitForRef {
                snapshot_id,
                generation,
                ref_id,
                ..
            }
            | AgentAction::FrameWaitForRef {
                snapshot_id,
                generation,
                ref_id,
                ..
            }
            | AgentAction::UploadRef {
                snapshot_id,
                generation,
                ref_id,
                ..
            } => (snapshot_id, *generation, Some(ref_id.as_str()), None),
            AgentAction::FrameUploadRef {
                snapshot_id,
                generation,
                ref_id,
                ..
            } => (snapshot_id, *generation, Some(ref_id.as_str()), None),
            AgentAction::DownloadRef {
                snapshot_id,
                generation,
                ref_id,
                ..
            } => (snapshot_id, *generation, Some(ref_id.as_str()), None),
            AgentAction::FrameDownloadRef {
                snapshot_id,
                generation,
                ref_id,
                ..
            } => (snapshot_id, *generation, Some(ref_id.as_str()), None),
            AgentAction::ClickSemantic {
                snapshot_id,
                generation,
                locator,
            }
            | AgentAction::TypeSemantic {
                snapshot_id,
                generation,
                locator,
                ..
            }
            | AgentAction::SelectSemantic {
                snapshot_id,
                generation,
                locator,
                ..
            }
            | AgentAction::WaitForSemantic {
                snapshot_id,
                generation,
                locator,
                ..
            } => (snapshot_id, *generation, None, Some(locator)),
            _ => return Ok((action.clone(), None, None, None)),
        };
        let snapshot = ctx.active_snapshot.clone().ok_or_else(|| {
            target_repair(
                "no active snapshot; observe the page before acting",
                json!({}),
            )
        })?;
        if snapshot.snapshot.snapshot_id != *snapshot_id
            || snapshot.snapshot.generation != generation
        {
            return Err(target_repair(
                "snapshot reference is stale; re-observe the page before acting",
                json!({ "snapshot_id": snapshot_id, "generation": generation }),
            ));
        }

        let ref_id = match ref_id {
            Some(ref_id) => ref_id.to_owned(),
            None => resolve_semantic_ref(&snapshot, locator.expect("semantic locator is set"))?,
        };
        let target = snapshot.targets.get(&ref_id).cloned().ok_or_else(|| {
            target_repair(
                "snapshot target is unavailable; re-observe the page",
                json!({ "ref_id": ref_id }),
            )
        })?;
        validate_snapshot_frame_contract(action, &snapshot.snapshot, &target.target)?;
        let expected = target.target.fingerprint.clone().ok_or_else(|| {
            target_repair(
                "snapshot target lacks a fingerprint; re-observe the page",
                json!({ "ref_id": ref_id }),
            )
        })?;
        if matches!(
            action,
            AgentAction::UploadRef { .. }
                | AgentAction::FrameUploadRef { .. }
                | AgentAction::DownloadRef { .. }
                | AgentAction::FrameDownloadRef { .. }
        ) && target.native_target.is_none()
        {
            return Err(target_repair(
                "transfer target has no native browser binding; observe again",
                json!({ "ref_id": ref_id }),
            ));
        }
        if target.native_target.is_none() {
            let selector = target.selector.as_deref().ok_or_else(|| {
                target_repair(
                    "snapshot target has no executable browser binding; re-observe the page",
                    json!({ "ref_id": ref_id }),
                )
            })?;
            let current_html = self.browser.content(session).await.map_err(|_| {
                target_repair(
                    "could not revalidate target against the current page",
                    json!({ "ref_id": ref_id }),
                )
            })?;
            let current = build_dom_summary(&String::from_utf8_lossy(&current_html));
            let fresh = current.interactive_elements.iter().find(|candidate| {
                candidate
                    .fingerprint
                    .as_ref()
                    .map(|fingerprint| fingerprint.fingerprint_id.as_str())
                    == Some(expected.fingerprint_id.as_str())
                    && candidate.selector == selector
            });
            if fresh.is_none() {
                return Err(target_repair(
                    "snapshot target changed; re-observe and choose a current target",
                    json!({ "ref_id": ref_id, "fingerprint_id": expected.fingerprint_id }),
                ));
            }
        }
        let proof = ResolvedTargetProof {
            snapshot_id: snapshot.snapshot.snapshot_id,
            generation: snapshot.snapshot.generation,
            ref_id,
            fingerprint: expected.clone(),
            locator: locator.cloned(),
        };
        let selector = target.selector.clone().unwrap_or_default();
        let resolved_action = match action {
            AgentAction::ClickRef { .. }
            | AgentAction::FrameClickRef { .. }
            | AgentAction::ClickSemantic { .. } => AgentAction::Click {
                selector: selector.clone(),
            },
            AgentAction::TypeRef { text, .. }
            | AgentAction::FrameTypeRef { text, .. }
            | AgentAction::TypeSemantic { text, .. } => AgentAction::Type {
                selector: selector.clone(),
                text: text.clone(),
            },
            AgentAction::SelectRef { value, .. }
            | AgentAction::FrameSelectRef { value, .. }
            | AgentAction::SelectSemantic { value, .. } => AgentAction::Select {
                selector: selector.clone(),
                value: value.clone(),
            },
            AgentAction::WaitForRef { timeout_ms, .. }
            | AgentAction::FrameWaitForRef { timeout_ms, .. }
            | AgentAction::WaitForSemantic { timeout_ms, .. } => AgentAction::WaitFor {
                selector: selector.clone(),
                timeout_ms: *timeout_ms,
            },
            AgentAction::UploadRef { .. } => action.clone(),
            AgentAction::DownloadRef { .. } => action.clone(),
            AgentAction::FrameUploadRef {
                snapshot_id,
                generation,
                ref_id,
                artifact_id,
                approval_grant_id,
                ..
            } => AgentAction::UploadRef {
                snapshot_id: snapshot_id.clone(),
                generation: *generation,
                ref_id: ref_id.clone(),
                artifact_id: artifact_id.clone(),
                approval_grant_id: approval_grant_id.clone(),
            },
            AgentAction::FrameDownloadRef {
                snapshot_id,
                generation,
                ref_id,
                approval_grant_id,
                ..
            } => AgentAction::DownloadRef {
                snapshot_id: snapshot_id.clone(),
                generation: *generation,
                ref_id: ref_id.clone(),
                approval_grant_id: approval_grant_id.clone(),
            },
            _ => unreachable!("only snapshot actions reach target resolution"),
        };
        let verified_target_action = match action {
            AgentAction::ClickRef { .. }
            | AgentAction::FrameClickRef { .. }
            | AgentAction::ClickSemantic { .. } => Some(VerifiedTargetOperation::Click),
            AgentAction::TypeRef { text, .. }
            | AgentAction::FrameTypeRef { text, .. }
            | AgentAction::TypeSemantic { text, .. } => {
                Some(VerifiedTargetOperation::Type { text: text.clone() })
            }
            AgentAction::SelectRef { value, .. }
            | AgentAction::FrameSelectRef { value, .. }
            | AgentAction::SelectSemantic { value, .. } => Some(VerifiedTargetOperation::Select {
                value: value.clone(),
            }),
            AgentAction::WaitForRef { timeout_ms, .. }
            | AgentAction::FrameWaitForRef { timeout_ms, .. }
            | AgentAction::WaitForSemantic { timeout_ms, .. } => {
                Some(VerifiedTargetOperation::WaitFor {
                    timeout_ms: *timeout_ms,
                })
            }
            _ => unreachable!("only snapshot actions reach target resolution"),
        }
        .map(|operation| VerifiedTargetAction {
            native_target: target.native_target.clone(),
            selector,
            tag: expected.tag.clone(),
            attributes: expected.attributes.clone(),
            normalized_text: expected.normalized_text.clone(),
            operation,
        });
        let transfer_target = if matches!(
            action,
            AgentAction::UploadRef { .. }
                | AgentAction::FrameUploadRef { .. }
                | AgentAction::DownloadRef { .. }
                | AgentAction::FrameDownloadRef { .. }
        ) {
            target
                .native_target
                .clone()
                .ok_or_else(|| {
                    target_repair(
                        "upload target has no native browser binding; observe again",
                        json!({ "ref_id": proof.ref_id }),
                    )
                })?
                .into()
        } else {
            None
        };
        Ok((
            resolved_action,
            Some(proof),
            verified_target_action,
            transfer_target,
        ))
    }

    fn validate_download_admission(&self, download: &BrowserDownloadedFile) -> QuarryResult<()> {
        let bytes = &download.bytes;
        if bytes.len() > Self::MAX_DOWNLOAD_ARTIFACT_BYTES {
            return Err(QuarryError::new(
                ErrorCode::BadRequest,
                "browser download exceeds the governed artifact size limit",
            ));
        }
        // Refuse common executable containers independent of a page-provided
        // filename. This is an admission control, not a claim of malware
        // detection; production antivirus scanning remains a routing gate.
        let dangerous_magic = bytes.starts_with(b"MZ")
            || bytes.starts_with(b"\x7fELF")
            || bytes.starts_with(&[0xcf, 0xfa, 0xed, 0xfe])
            || bytes.starts_with(&[0xfe, 0xed, 0xfa, 0xcf]);
        if dangerous_magic {
            return Err(QuarryError::new(
                ErrorCode::SecurityBlocked,
                "browser download executable content is not admissible as evidence",
            ));
        }
        let extension = download
            .suggested_filename
            .rsplit_once('.')
            .map(|(_, extension)| extension.to_ascii_lowercase())
            .unwrap_or_default();
        // A suffix is only a hint from an untrusted page. Admit a small set of
        // evidence formats only when its bytes are compatible with that hint;
        // this prevents an arbitrary binary from entering the evidence store as
        // `report.pdf` or `notes.txt`.
        let admitted = match extension.as_str() {
            "pdf" => bytes.starts_with(b"%PDF-"),
            "png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
            "jpg" | "jpeg" => bytes.starts_with(&[0xff, 0xd8, 0xff]),
            "gif" => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
            "webp" => bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"),
            "txt" | "csv" | "tsv" | "json" | "xml" | "html" | "htm" | "md" => {
                std::str::from_utf8(bytes).is_ok()
            }
            _ => false,
        };
        if !admitted {
            return Err(QuarryError::new(
                ErrorCode::SecurityBlocked,
                "browser download bytes do not match an admitted evidence type",
            ));
        }
        Ok(())
    }

    async fn upload_artifact_to_target(
        &self,
        zdr_mode: ZdrMode,
        artifact_id: &ArtifactKind,
        session: &quarry_browser::BrowserSession,
        target: BrowserNativeTarget,
    ) -> QuarryResult<()> {
        if zdr_mode.is_active() {
            return Err(QuarryError::new(
                ErrorCode::SecurityBlocked,
                "artifact upload is unavailable in zero-data-retention mode",
            ));
        }
        if self.org_id.is_empty() {
            return Err(QuarryError::new(
                ErrorCode::Forbidden,
                "artifact upload requires a verified tenant identity",
            ));
        }
        let store = self.artifacts.as_ref().ok_or_else(|| {
            QuarryError::new(
                ErrorCode::RuntimeNotReady,
                "artifact storage is unavailable for governed upload",
            )
        })?;
        let bytes = store.get(&self.org_id, artifact_id).await?;
        if bytes.len() > Self::MAX_UPLOAD_ARTIFACT_BYTES {
            return Err(QuarryError::new(
                ErrorCode::BadRequest,
                "artifact exceeds the governed browser upload size limit",
            )
            .with_details(json!({
                "artifact_id": artifact_id,
                "max_bytes": Self::MAX_UPLOAD_ARTIFACT_BYTES,
            })));
        }
        let mut staged = tempfile::NamedTempFile::new().map_err(|error| {
            QuarryError::new(
                ErrorCode::Internal,
                "create private upload staging file failed",
            )
            .with_details(json!({ "error": error.to_string() }))
        })?;
        staged.write_all(&bytes).map_err(|error| {
            QuarryError::new(ErrorCode::Internal, "stage upload artifact failed")
                .with_details(json!({ "error": error.to_string() }))
        })?;
        staged.flush().map_err(|error| {
            QuarryError::new(ErrorCode::Internal, "flush upload staging file failed")
                .with_details(json!({ "error": error.to_string() }))
        })?;
        self.browser
            .upload_staged_file_to_target(session, target, staged.path())
            .await
    }

    async fn put_artifact_if_allowed(
        &self,
        zdr_mode: ZdrMode,
        run_id: &quarry_core::ids::kinds::RunKind,
        page_hash: &str,
        kind: &str,
        body: Vec<u8>,
    ) -> QuarryResult<Option<ArtifactKind>> {
        if zdr::guard(zdr_mode, WriteKind::Artifact).is_err() {
            return Ok(None);
        }
        let Some(store) = &self.artifacts else {
            return Ok(None);
        };
        let handle = store
            .put(&self.org_id, run_id, page_hash, kind, body)
            .await?;
        Ok(Some(handle.artifact_id))
    }

    async fn put_json_artifact_if_allowed<T: serde::Serialize + ?Sized>(
        &self,
        zdr_mode: ZdrMode,
        run_id: &quarry_core::ids::kinds::RunKind,
        page_hash: &str,
        kind: &str,
        value: &T,
    ) -> QuarryResult<Option<ArtifactKind>> {
        let body = serde_json::to_vec(value).map_err(|e| {
            QuarryError::new(
                ErrorCode::Internal,
                format!("serialize visual artifact {kind}: {e}"),
            )
        })?;
        self.put_artifact_if_allowed(zdr_mode, run_id, page_hash, kind, body)
            .await
    }

    async fn page_state(&self, session: &quarry_browser::BrowserSession) -> PageState {
        self.browser
            .evaluate(
                session,
                r#"(() => ({
                    url: window.location && window.location.href ? window.location.href : null,
                    title: document && document.title ? document.title : null
                }))()"#,
            )
            .await
            .ok()
            .map(|value| PageState {
                url: value
                    .get("url")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned),
                title: value
                    .get("title")
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|title| !title.is_empty())
                    .map(str::to_owned),
            })
            .unwrap_or_default()
    }
}

fn action_requires_atomic_target(action: &AgentAction) -> bool {
    matches!(
        action,
        AgentAction::ClickRef { .. }
            | AgentAction::FrameClickRef { .. }
            | AgentAction::ClickSemantic { .. }
            | AgentAction::TypeRef { .. }
            | AgentAction::FrameTypeRef { .. }
            | AgentAction::TypeSemantic { .. }
            | AgentAction::SelectRef { .. }
            | AgentAction::FrameSelectRef { .. }
            | AgentAction::SelectSemantic { .. }
            | AgentAction::WaitForRef { .. }
            | AgentAction::FrameWaitForRef { .. }
            | AgentAction::WaitForSemantic { .. }
            | AgentAction::UploadRef { .. }
            | AgentAction::FrameUploadRef { .. }
            | AgentAction::DownloadRef { .. }
            | AgentAction::FrameDownloadRef { .. }
    )
}

/// Child-frame browser actions have a deliberately separate public grammar.
/// A bare snapshot ref is therefore never enough to cause a cross-frame
/// effect: the caller must echo the opaque frame id observed with that target,
/// and Quarry checks both values against the current snapshot before it gives
/// the driver its private backend-node binding.
fn validate_snapshot_frame_contract(
    action: &AgentAction,
    snapshot: &BrowserSnapshot,
    target: &SnapshotTarget,
) -> QuarryResult<()> {
    let explicit_frame_id = match action {
        AgentAction::FrameClickRef { frame_id, .. }
        | AgentAction::FrameTypeRef { frame_id, .. }
        | AgentAction::FrameSelectRef { frame_id, .. }
        | AgentAction::FrameWaitForRef { frame_id, .. }
        | AgentAction::FrameUploadRef { frame_id, .. }
        | AgentAction::FrameDownloadRef { frame_id, .. } => Some(frame_id.as_str()),
        _ => None,
    };
    let root_frame_id = snapshot
        .frames
        .iter()
        .find(|frame| frame.parent_frame_id.is_none())
        .map(|frame| frame.frame_id.as_str());
    let target_frame_id = target.frame_id.as_deref();
    let target_is_child = match (root_frame_id, target_frame_id) {
        (Some(root), Some(frame)) => root != frame,
        // A native child-frame target always carries its frame id. When an
        // old renderer omits root identity, an explicit target frame is not
        // assumed safe: require the dedicated frame action grammar.
        (None, Some(_)) => true,
        (_, None) => false,
    };

    match (target_is_child, explicit_frame_id, target_frame_id) {
        (true, Some(requested), Some(actual)) if requested == actual => Ok(()),
        (true, _, Some(actual)) => Err(target_repair(
            "child-frame target requires an exact frame-bound action; observe again",
            json!({ "target_frame_id": actual }),
        )),
        (false, Some(_), _) => Err(target_repair(
            "frame-bound action may only target an observed child frame",
            json!({}),
        )),
        (false, None, _) => Ok(()),
        // The prior arms cover normal snapshot data. Preserve fail-closed
        // behaviour if a future driver violates that shape.
        _ => Err(target_repair(
            "frame contract is incomplete; observe again",
            json!({}),
        )),
    }
}

fn target_repair(message: impl Into<String>, details: Value) -> QuarryError {
    QuarryError::new(ErrorCode::TargetRepairRequired, message).with_details(details)
}

fn build_active_snapshot(
    run_id: &quarry_core::ids::kinds::RunKind,
    generation: u32,
    page_hash: &str,
    summary: Option<&DomSummary>,
    native_projection: Option<BrowserNativeProjection>,
) -> ActiveSnapshot {
    let snapshot_id = format!(
        "snap_{}",
        &blake3::hash(format!("{run_id}|{generation}|{page_hash}").as_bytes())
            .to_hex()
            .to_string()[..24]
    );
    let mut targets = HashMap::new();
    let mut snapshot_targets = Vec::new();
    let native_targets = native_projection
        .as_ref()
        .map(|projection| projection.targets.as_slice())
        .unwrap_or_default();
    if native_projection.is_some() {
        // Chromium's AX projection is authoritative even when it contains no
        // actionable targets. Falling back to a string-scanned HTML selector
        // in that case would silently reintroduce a second, less reliable
        // target authority. The scanner is compatibility-only for drivers
        // that cannot produce any native projection at all.
        for native in native_targets.iter().take(50) {
            let ref_id = format!("@e{}", snapshot_targets.len() + 1);
            let target = SnapshotTarget {
                ref_id: ref_id.clone(),
                tag: native
                    .role
                    .as_deref()
                    .map(|role| format!("ax:{role}"))
                    .unwrap_or_else(|| "ax:node".to_owned()),
                text: native.name.clone().or_else(|| native.value.clone()),
                role: native.role.clone(),
                name: native.name.clone(),
                placeholder: None,
                test_id: None,
                frame_id: native.frame_id.clone(),
                fingerprint: Some(native_target_fingerprint(native)),
            };
            targets.insert(
                ref_id,
                ActiveSnapshotTarget {
                    selector: None,
                    native_target: Some(native.clone()),
                    target: target.clone(),
                },
            );
            snapshot_targets.push(target);
        }
    } else {
        let selector_counts = summary
            .into_iter()
            .flat_map(|summary| summary.interactive_elements.iter())
            .fold(HashMap::<&str, usize>::new(), |mut counts, element| {
                *counts.entry(element.selector.as_str()).or_default() += 1;
                counts
            });
        for element in summary
            .into_iter()
            .flat_map(|summary| summary.interactive_elements.iter())
            .filter(|element| {
                element.selector != element.tag
                    && selector_counts.get(element.selector.as_str()) == Some(&1)
            })
            .take(50)
        {
            let ref_id = format!("@e{}", snapshot_targets.len() + 1);
            let target = SnapshotTarget {
                ref_id: ref_id.clone(),
                tag: element.tag.clone(),
                text: element.text.clone(),
                role: element.role.clone(),
                name: element
                    .accessible_name
                    .clone()
                    .or_else(|| element.aria_label.clone())
                    .or_else(|| element.text.clone()),
                placeholder: element.placeholder.clone(),
                test_id: element.test_id.clone(),
                frame_id: None,
                fingerprint: element.fingerprint.clone(),
            };
            targets.insert(
                ref_id,
                ActiveSnapshotTarget {
                    selector: Some(element.selector.clone()),
                    native_target: None,
                    target: target.clone(),
                },
            );
            snapshot_targets.push(target);
        }
    }
    ActiveSnapshot {
        snapshot: BrowserSnapshot {
            snapshot_id,
            generation,
            targets: snapshot_targets,
            accessibility: native_projection
                .as_ref()
                .and_then(|projection| projection.accessibility.clone()),
            frames: native_projection
                .map(|projection| projection.frames)
                .unwrap_or_default(),
        },
        targets,
    }
}

fn native_target_fingerprint(target: &BrowserNativeTarget) -> ElementFingerprint {
    let mut attributes = Vec::new();
    if let Some(role) = &target.role {
        attributes.push(("ax_role".to_owned(), role.clone()));
    }
    if let Some(name) = &target.name {
        attributes.push(("ax_name".to_owned(), name.clone()));
    }
    if let Some(frame_id) = &target.frame_id {
        attributes.push(("frame_id".to_owned(), frame_id.clone()));
    }
    ElementFingerprint {
        fingerprint_id: format!("ax:{}:{}", target.ax_node_id, target.backend_node_id),
        tag: target
            .role
            .as_deref()
            .map(|role| format!("ax:{role}"))
            .unwrap_or_else(|| "ax:node".to_owned()),
        normalized_text: target
            .name
            .as_deref()
            .or(target.value.as_deref())
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase(),
        attributes,
        structural_path: format!("ax:{}", target.ax_node_id),
        logical_id: Some(target.ax_node_id.clone()),
    }
}

fn resolve_semantic_ref(
    snapshot: &ActiveSnapshot,
    locator: &SemanticLocator,
) -> QuarryResult<String> {
    let mut matches: Vec<String> = match locator {
        SemanticLocator::Nth { selector, index } => {
            let mut candidates = snapshot
                .targets
                .iter()
                .filter(|(_, target)| {
                    target.target.tag == *selector
                        || target.target.role.as_deref() == Some(selector.as_str())
                })
                .map(|(ref_id, _)| ref_id.clone())
                .collect::<Vec<_>>();
            candidates.sort_by_key(|ref_id| {
                ref_id
                    .trim_start_matches("@e")
                    .parse::<u32>()
                    .unwrap_or(u32::MAX)
            });
            candidates
                .get(*index as usize)
                .cloned()
                .into_iter()
                .collect()
        }
        _ => snapshot
            .targets
            .iter()
            .filter_map(|(ref_id, target)| {
                semantic_matches(&target.target, locator).then_some(ref_id.clone())
            })
            .collect(),
    };
    matches.sort();
    match matches.as_slice() {
        [ref_id] => Ok(ref_id.clone()),
        [] => Err(target_repair(
            "semantic target was not found in the active snapshot",
            json!({ "locator": locator }),
        )),
        _ => Err(target_repair(
            "semantic target is ambiguous; choose an explicit snapshot ref",
            json!({ "match_count": matches.len() }),
        )),
    }
}

fn semantic_matches(target: &SnapshotTarget, locator: &SemanticLocator) -> bool {
    match locator {
        SemanticLocator::Role { role, name, exact } => {
            value_matches(target.role.as_deref(), role, *exact)
                && name
                    .as_deref()
                    .map(|name| value_matches(target.name.as_deref(), name, *exact))
                    .unwrap_or(true)
        }
        SemanticLocator::Text { text, exact } => {
            value_matches(target.text.as_deref(), text, *exact)
        }
        SemanticLocator::Label { label, exact } => {
            value_matches(target.name.as_deref(), label, *exact)
        }
        SemanticLocator::Placeholder { placeholder, exact } => {
            value_matches(target.placeholder.as_deref(), placeholder, *exact)
        }
        SemanticLocator::TestId { test_id, exact } => {
            value_matches(target.test_id.as_deref(), test_id, *exact)
        }
        SemanticLocator::Nth { .. } => false,
    }
}

fn value_matches(value: Option<&str>, needle: &str, exact: bool) -> bool {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return false;
    };
    if exact {
        value.eq_ignore_ascii_case(needle.trim())
    } else {
        value
            .to_ascii_lowercase()
            .contains(&needle.trim().to_ascii_lowercase())
    }
}

fn attach_related(
    artifact: &mut crate::vision::VisualObservationArtifact,
    kind: &str,
    id: &ArtifactKind,
) {
    artifact
        .related_artifacts
        .insert(kind.to_string(), id.to_string());
}

#[derive(Default)]
struct PageState {
    url: Option<String>,
    title: Option<String>,
}

fn console_summary_from_devtools(events: &[BrowserDevtoolsEvent]) -> Vec<ConsoleLine> {
    let mut summary = events
        .iter()
        .filter(|event| event.category == "console")
        .filter_map(|event| {
            let text = event.text.as_deref()?.trim();
            if text.is_empty() {
                return None;
            }
            Some(ConsoleLine {
                level: event.level.clone().unwrap_or_else(|| "info".to_owned()),
                text: text.chars().take(500).collect(),
            })
        })
        .rev()
        .take(20)
        .collect::<Vec<_>>();
    summary.reverse();
    summary
}

fn network_summary_from_devtools(events: &[BrowserDevtoolsEvent]) -> Vec<NetworkEntry> {
    let mut methods_by_url = HashMap::<String, String>::new();
    for event in events {
        if event.name == "Network.requestWillBeSent" {
            if let (Some(url), Some(method)) = (&event.url, &event.method) {
                methods_by_url.insert(url.clone(), method.clone());
            }
        }
    }

    let mut summary = events
        .iter()
        .filter(|event| event.name == "Network.responseReceived")
        .filter_map(|event| {
            let url = event.url.clone()?;
            Some(NetworkEntry {
                method: methods_by_url
                    .get(&url)
                    .cloned()
                    .unwrap_or_else(|| "GET".to_owned()),
                url,
                status: event.status.unwrap_or(0),
                content_type: event
                    .payload
                    .get("response")
                    .and_then(|response| response.get("mimeType"))
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned),
            })
        })
        .rev()
        .take(30)
        .collect::<Vec<_>>();
    summary.reverse();
    summary
}

fn network_evidence_delta(
    previous_keys: &[String],
    current_entries: &[NetworkEntry],
) -> NetworkEvidenceDelta {
    let previous = previous_keys.iter().cloned().collect::<BTreeSet<_>>();
    let current = current_entries
        .iter()
        .map(redacted_network_key)
        .collect::<BTreeSet<_>>();
    NetworkEvidenceDelta {
        added: current.difference(&previous).take(30).cloned().collect(),
        removed: previous.difference(&current).take(30).cloned().collect(),
        current_count: current.len().min(u32::MAX as usize) as u32,
    }
}

fn redacted_network_key(entry: &NetworkEntry) -> String {
    let location = url::Url::parse(&entry.url)
        .ok()
        .and_then(|url| {
            let host = url.host_str()?;
            let port = url
                .port()
                .map(|port| format!(":{port}"))
                .unwrap_or_default();
            Some(format!("{}://{}{}{}", url.scheme(), host, port, url.path()))
        })
        .unwrap_or_else(|| "unparseable_url".to_owned());
    let location = truncate_evidence_value(&location, 512);
    let content_type = entry
        .content_type
        .as_deref()
        .unwrap_or("unknown")
        .split(';')
        .next()
        .unwrap_or("unknown")
        .trim();
    let content_type = truncate_evidence_value(content_type, 120);
    format!(
        "{} {} {} {}",
        truncate_evidence_value(&entry.method, 16),
        entry.status,
        content_type,
        location
    )
}

fn truncate_evidence_value(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{truncated}…")
    } else {
        truncated
    }
}

fn elapsed_ms(started_at: Instant) -> u64 {
    started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

/// An intentionally coarse planning-budget estimate. It is computed from the
/// serialized public snapshot rather than raw DOM bytes; model-specific
/// tokenization and any private CDP bindings never enter this figure.
fn estimated_snapshot_tokens(snapshot: Option<&BrowserSnapshot>) -> u32 {
    let bytes = snapshot
        .and_then(|snapshot| serde_json::to_vec(snapshot).ok())
        .map_or(0, |serialized| serialized.len());
    bytes
        .saturating_add(3)
        .saturating_div(4)
        .min(u32::MAX as usize) as u32
}

fn extract_title(html: &str) -> Option<String> {
    let start = html.find("<title")?;
    let after_tag = html[start..].find('>')? + start + 1;
    let end = html[after_tag..].find("</title>")? + after_tag;
    let title = html[after_tag..end].trim();
    if title.is_empty() {
        None
    } else {
        Some(title.to_string())
    }
}

fn build_dom_summary(html: &str) -> DomSummary {
    let node_count = html.matches('<').count() as u32;
    let mut interactive_elements = Vec::new();

    for tag in &["a", "button", "input", "select", "textarea"] {
        for idx in tag_openings(html, tag) {
            let remaining = &html[idx..];
            let end = remaining.find('>').unwrap_or(remaining.len());
            let element_str = &remaining[..end];

            let text = extract_inner_text(remaining);
            let role = extract_attr(element_str, "role")
                .map(|s| s.to_string())
                .or_else(|| match *tag {
                    "a" => Some("link".to_owned()),
                    "button" => Some("button".to_owned()),
                    "select" => Some("combobox".to_owned()),
                    "textarea" => Some("textbox".to_owned()),
                    "input" => Some("textbox".to_owned()),
                    _ => None,
                });
            let aria_label = extract_attr(element_str, "aria-label").map(|s| s.to_string());
            let placeholder = extract_attr(element_str, "placeholder").map(str::to_owned);
            let test_id = extract_attr(element_str, "data-testid")
                .or_else(|| extract_attr(element_str, "data-test"));
            let id = extract_attr(element_str, "id");
            let name = extract_attr(element_str, "name");
            let associated_label = id.and_then(|id| extract_label_for(html, id));
            let mut selectors = Vec::new();
            if let Some(id) = id {
                selectors.push(attribute_selector("id", id));
            }
            if let Some(test_id) = test_id {
                selectors.push(attribute_selector("data-testid", test_id));
            }
            if let Some(name) = name {
                selectors.push(format!("{tag}{}", attribute_selector("name", name)));
            }
            if let Some(label) = aria_label.as_deref() {
                selectors.push(format!("{tag}{}", attribute_selector("aria-label", label)));
            }
            if let Some(role) = role.as_deref() {
                if extract_attr(element_str, "role").is_some() {
                    selectors.push(format!("{tag}{}", attribute_selector("role", role)));
                }
            }
            let selector = selectors
                .first()
                .cloned()
                .unwrap_or_else(|| tag.to_string());
            let selector_alternatives = selectors.into_iter().skip(1).collect();

            let normalized_text = text
                .as_deref()
                .unwrap_or_default()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_ascii_lowercase();
            let mut attributes = Vec::new();
            for attr in [
                "id",
                "data-testid",
                "data-test",
                "name",
                "role",
                "aria-label",
                "placeholder",
            ] {
                if let Some(value) = extract_attr(element_str, attr) {
                    attributes.push((attr.to_owned(), value.to_owned()));
                }
            }
            let logical_id = id
                .or(test_id)
                .or(name)
                .or(aria_label.as_deref())
                .map(str::to_owned);
            let structural_path = format!("{tag}[{idx}]");
            let fingerprint_seed =
                format!("{tag}|{normalized_text}|{attributes:?}|{structural_path}|{logical_id:?}");
            let fingerprint_id = format!(
                "efp_{}",
                &blake3::hash(fingerprint_seed.as_bytes())
                    .to_hex()
                    .to_string()[..24]
            );

            interactive_elements.push(InteractiveElement {
                tag: tag.to_string(),
                selector,
                selector_alternatives,
                text: text.clone(),
                role: role.clone(),
                aria_label: aria_label.clone(),
                accessible_name: aria_label
                    .clone()
                    .or(associated_label)
                    .or_else(|| text.clone()),
                placeholder,
                test_id: test_id.map(str::to_owned),
                fingerprint: Some(ElementFingerprint {
                    fingerprint_id,
                    tag: tag.to_string(),
                    normalized_text,
                    attributes,
                    structural_path,
                    logical_id,
                }),
            });

            if interactive_elements.len() >= 50 {
                break;
            }
        }
    }

    let text_snippet = html
        .find("<body")
        .and_then(|start| html[start..].find('>').map(|i| start + i + 1))
        .map(|body_start| {
            let body = &html[body_start..];
            let stripped = strip_tags(body);
            let trimmed = stripped.trim();
            if trimmed.len() > 500 {
                format!("{}...", &trimmed[..500])
            } else {
                trimmed.to_string()
            }
        })
        .filter(|s| !s.is_empty());

    DomSummary {
        node_count,
        interactive_elements,
        text_snippet,
    }
}

fn tag_openings(html: &str, tag: &str) -> Vec<usize> {
    let pattern = format!("<{tag}");
    html.match_indices(&pattern)
        .filter_map(|(idx, _)| {
            let after = html.as_bytes().get(idx + pattern.len()).copied();
            after
                .is_some_and(|byte| byte == b'>' || byte == b'/' || byte.is_ascii_whitespace())
                .then_some(idx)
        })
        .collect()
}

fn attribute_selector(name: &str, value: &str) -> String {
    format!(r#"[{name}="{}"]"#, escape_css_string(value))
}

fn escape_css_string(value: &str) -> String {
    value
        .chars()
        .flat_map(|character| match character {
            '\\' => "\\\\".chars().collect::<Vec<_>>(),
            '"' => "\\\"".chars().collect::<Vec<_>>(),
            '\n' => "\\A ".chars().collect::<Vec<_>>(),
            '\r' => "\\D ".chars().collect::<Vec<_>>(),
            '\u{000C}' => "\\C ".chars().collect::<Vec<_>>(),
            character if character.is_control() => {
                format!("\\{:X} ", character as u32).chars().collect()
            }
            character => vec![character],
        })
        .collect()
}

/// Execute a bounded, deterministic extraction profile over the captured
/// source. Network JSON and visual sources are represented in the profile
/// contract but require provider-specific evidence unavailable in this page
/// snapshot; they are skipped rather than guessed.
fn extract_profile(bytes: &[u8], profile: &ExtractionProfile) -> Option<ExtractionResult> {
    let max_bytes = profile.max_bytes.clamp(1, 5_000_000) as usize;
    let html = String::from_utf8_lossy(&bytes[..bytes.len().min(max_bytes)]);
    let mut fields = Vec::with_capacity(profile.fields.len());
    let mut used_bytes = 0usize;
    let mut truncated = bytes.len() > max_bytes;

    for field in &profile.fields {
        let mut extracted: Option<(Value, ExtractionSource, Option<String>)> = None;
        for source in &profile.source_order {
            match source {
                ExtractionSource::JsonLd => {
                    if let Some(value) = extract_json_ld_field(&html, &field.name) {
                        extracted = Some((
                            value,
                            ExtractionSource::JsonLd,
                            Some("script[type=application/ld+json]".into()),
                        ));
                    }
                }
                ExtractionSource::Dom | ExtractionSource::Accessibility => {
                    let selector = field.selector.as_deref().unwrap_or(&field.name);
                    if let Some(value) = extract_dom_field(&html, selector) {
                        extracted =
                            Some((Value::String(value), source.clone(), Some(selector.into())));
                    }
                }
                ExtractionSource::NetworkJson | ExtractionSource::Visual => {}
            }
            if extracted.is_some() {
                break;
            }
        }

        let (value, source, evidence_selector) = match extracted {
            Some((value, source, evidence_selector)) => {
                let encoded_len = serde_json::to_vec(&value).map(|v| v.len()).unwrap_or(0);
                if used_bytes.saturating_add(encoded_len) > max_bytes {
                    truncated = true;
                    (None, None, None)
                } else {
                    used_bytes = used_bytes.saturating_add(encoded_len);
                    (Some(value), Some(source), evidence_selector)
                }
            }
            None => (None, None, None),
        };

        if field.required && value.is_none() {
            truncated = true;
        }
        fields.push(ExtractionFieldResult {
            name: field.name.clone(),
            value,
            source,
            evidence_selector,
        });
    }

    Some(ExtractionResult {
        profile_id: profile.profile_id.clone(),
        fields,
        truncated,
    })
}

fn extract_dom_field(html: &str, selector: &str) -> Option<String> {
    let needle = selector.strip_prefix('#').map(|id| format!("id=\"{id}\""));
    let needle = needle.or_else(|| {
        selector
            .strip_prefix("[data-testid=\"")
            .and_then(|value| value.strip_suffix("\"]"))
            .map(|value| format!("data-testid=\"{value}\""))
    });
    let start = if let Some(needle) = needle {
        html.find(&needle)
            .and_then(|attribute_start| html[..attribute_start].rfind('<'))
    } else {
        let tag = selector.split('[').next().unwrap_or(selector).trim();
        (!tag.is_empty())
            .then(|| html.find(&format!("<{tag}")))
            .flatten()
    }?;
    let open_end = html[start..].find('>')? + start;
    let close_tag = html[start + 1..]
        .split(|ch: char| ch.is_ascii_whitespace() || ch == '>')
        .next()
        .filter(|tag| !tag.is_empty())?;
    let close = format!("</{close_tag}>");
    let body_start = open_end + 1;
    let body_end = html[body_start..].find(&close)? + body_start;
    let text = strip_tags(&html[body_start..body_end]);
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (!text.is_empty()).then_some(text)
}

fn extract_json_ld_field(html: &str, field: &str) -> Option<Value> {
    let mut offset = 0;
    while let Some(relative) = html[offset..].find("application/ld+json") {
        let script_start = offset + relative;
        let body_start = html[script_start..].find('>')? + script_start + 1;
        let body_end = html[body_start..].find("</script>")? + body_start;
        let body = html[body_start..body_end].trim();
        if let Ok(value) = serde_json::from_str::<Value>(body) {
            if let Some(found) = json_value_field(&value, field) {
                return Some(found.clone());
            }
        }
        offset = body_end + "</script>".len();
    }
    None
}

fn json_value_field<'a>(value: &'a Value, field: &str) -> Option<&'a Value> {
    match value {
        Value::Object(object) => object.get(field).or_else(|| {
            object
                .values()
                .find_map(|child| json_value_field(child, field))
        }),
        Value::Array(values) => values
            .iter()
            .find_map(|child| json_value_field(child, field)),
        _ => None,
    }
}

fn observation_delta(
    previous_url: Option<&str>,
    previous_title: Option<&str>,
    previous_dom_nodes: Option<u32>,
    previous_page_hash: Option<&str>,
    current_url: &str,
    current_title: Option<&str>,
    current_dom_nodes: Option<u32>,
    current_page_hash: Option<&str>,
) -> ObservationDelta {
    let url_changed = previous_url.is_some_and(|previous| previous != current_url);
    let title_changed = previous_title != current_title;
    let dom_changed = previous_dom_nodes
        .is_some_and(|previous| current_dom_nodes.is_some_and(|current| current != previous));
    let content_changed = previous_page_hash
        .is_some_and(|previous| current_page_hash.is_some_and(|current| current != previous));
    let mut changed_fields = Vec::new();
    if previous_url.is_none() {
        changed_fields.push("initial_observation".to_string());
    } else {
        if url_changed {
            changed_fields.push("url".to_string());
        }
        if title_changed {
            changed_fields.push("title".to_string());
        }
        if dom_changed {
            changed_fields.push("dom".to_string());
        }
        if content_changed {
            changed_fields.push("content".to_string());
        }
    }
    ObservationDelta {
        changed_fields,
        url_changed,
        title_changed,
        dom_changed,
        content_changed,
    }
}

fn extract_attr<'a>(element: &'a str, attr: &str) -> Option<&'a str> {
    for quote in ['"', '\''] {
        let pattern = format!("{attr}={quote}");
        let mut offset = 0;
        while let Some(relative) = element[offset..].find(&pattern) {
            let start = offset + relative;
            let is_attribute_boundary = start == 0
                || element
                    .as_bytes()
                    .get(start.saturating_sub(1))
                    .is_some_and(|byte| byte.is_ascii_whitespace() || *byte == b'<');
            if is_attribute_boundary {
                let val_start = start + pattern.len();
                if let Some(end) = element[val_start..].find(quote) {
                    return Some(&element[val_start..val_start + end]);
                }
            }
            offset = start + pattern.len();
        }
    }
    None
}

fn extract_inner_text(element_and_after: &str) -> Option<String> {
    let close_tag = element_and_after.find('>')?;
    let after = &element_and_after[close_tag + 1..];
    let end = after.find('<').unwrap_or(after.len());
    let text = after[..end].trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

fn extract_label_for(html: &str, control_id: &str) -> Option<String> {
    let mut offset = 0;
    while let Some(relative) = html[offset..].find("<label") {
        let start = offset + relative;
        let remaining = &html[start..];
        let end = remaining.find('>')?;
        let element = &remaining[..end];
        if extract_attr(element, "for") == Some(control_id) {
            return extract_inner_text(remaining);
        }
        offset = start + end + 1;
    }
    None
}

fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifact_store::{ArtifactStore, InMemoryStore};
    use crate::vision::{
        VisualObservationArtifact, VisualObservationResult, VisualPreprocessInput,
        VisualPreprocessResult, VisualRegion,
    };
    use bytes::Bytes;
    use quarry_browser::BrowserDriver;
    use quarry_core::contracts::AgentConstraints;
    use quarry_core::ids::kinds::RunKind;
    use quarry_core::ids::Id;
    use quarry_core::lease::{BrowserLease, ProxyAffinity};
    use std::sync::Arc;

    #[test]
    fn extract_title_basic() {
        let html = "<html><head><title>Hello World</title></head></html>";
        assert_eq!(extract_title(html), Some("Hello World".to_string()));
    }

    #[test]
    fn extract_title_missing() {
        let html = "<html><head></head></html>";
        assert_eq!(extract_title(html), None);
    }

    #[test]
    fn effectful_actions_are_unknown_without_a_postcondition() {
        let outcome = ObservationRunner::outcome_for_action(
            &AgentAction::Click {
                selector: "#submit".into(),
            },
            "https://example.com",
        );
        assert_eq!(
            outcome.status,
            quarry_core::contracts::ActionOutcomeStatus::Unknown
        );
        assert_eq!(
            outcome.reason_code.as_deref(),
            Some("postcondition_required")
        );
    }

    #[test]
    fn completed_navigation_has_an_explicit_verified_receipt() {
        let outcome = ObservationRunner::outcome_for_action(
            &AgentAction::Navigate {
                url: "https://example.com".into(),
            },
            "https://example.com",
        );
        assert_eq!(
            outcome.status,
            quarry_core::contracts::ActionOutcomeStatus::Verified
        );
        assert_eq!(outcome.reason_code.as_deref(), Some("navigation_completed"));
    }

    #[test]
    fn challenge_classifier_requires_escalation_and_preserves_evidence() {
        let signal = ObservationRunner::classify_challenge(
            "https://example.test/challenge",
            Some("Verify you are human"),
            Some("captcha required"),
        )
        .expect("captcha should be classified");
        assert_eq!(signal.kind, ChallengeKind::Captcha);
        assert!(signal.requires_escalation);
        assert!(signal.confidence > 0.9);
        assert!(!signal.evidence.is_empty());
    }

    #[test]
    fn ordinary_source_pages_do_not_receive_a_challenge_signal() {
        assert!(ObservationRunner::classify_challenge(
            "https://example.test/article",
            Some("Example article"),
            Some("A normal page with source text"),
        )
        .is_none());
    }

    #[test]
    fn extraction_profile_prefers_json_ld_and_keeps_evidence() {
        let html = r#"<script type="application/ld+json">{"name":"Widget","price":12}</script>
            <main><span id="name">DOM fallback</span></main>"#;
        let profile = ExtractionProfile {
            profile_id: "product".into(),
            fields: vec![
                quarry_core::contracts::ExtractionField {
                    name: "name".into(),
                    selector: Some("#name".into()),
                    required: true,
                },
                quarry_core::contracts::ExtractionField {
                    name: "price".into(),
                    selector: None,
                    required: true,
                },
            ],
            source_order: vec![ExtractionSource::JsonLd, ExtractionSource::Dom],
            max_bytes: 10_000,
        };
        let result = extract_profile(html.as_bytes(), &profile).expect("result");
        assert_eq!(result.profile_id, "product");
        assert_eq!(result.fields[0].value, Some(Value::String("Widget".into())));
        assert_eq!(result.fields[0].source, Some(ExtractionSource::JsonLd));
        assert!(result.fields[0].evidence_selector.is_some());
        assert_eq!(result.fields[1].value, Some(serde_json::json!(12)));
    }

    #[test]
    fn extraction_profile_is_bounded_when_source_exceeds_limit() {
        let profile = ExtractionProfile {
            profile_id: "tiny".into(),
            fields: vec![quarry_core::contracts::ExtractionField {
                name: "title".into(),
                selector: Some("#title".into()),
                required: false,
            }],
            source_order: vec![ExtractionSource::Dom],
            max_bytes: 10,
        };
        let result = extract_profile(b"<div id=\"title\">a very long value</div>", &profile)
            .expect("result");
        assert!(result.truncated);
    }

    #[test]
    fn dom_summary_counts_nodes() {
        let html = "<html><body><a href='#' id='link1' aria-label='Open'>Click</a><button data-testid='go'>Go</button><label for='email'>Email address</label><input id='email' placeholder='name@example.test'></body></html>";
        let summary = build_dom_summary(html);
        assert!(summary.node_count > 0);
        assert!(!summary.interactive_elements.is_empty());
        let link = summary
            .interactive_elements
            .iter()
            .find(|e| e.tag == "a")
            .unwrap();
        assert_eq!(link.selector, "[id=\"link1\"]");
        assert_eq!(link.aria_label.as_deref(), Some("Open"));
        assert!(link
            .selector_alternatives
            .iter()
            .any(|selector| selector.contains("aria-label")));
        assert_eq!(link.text.as_deref(), Some("Click"));
        let button = summary
            .interactive_elements
            .iter()
            .find(|e| e.tag == "button")
            .unwrap();
        assert_eq!(button.selector, "[data-testid=\"go\"]");
        assert_eq!(button.role.as_deref(), Some("button"));
        assert_eq!(button.test_id.as_deref(), Some("go"));
        let input = summary
            .interactive_elements
            .iter()
            .find(|e| e.tag == "input")
            .unwrap();
        assert_eq!(input.accessible_name.as_deref(), Some("Email address"));
        assert_eq!(input.placeholder.as_deref(), Some("name@example.test"));
    }

    #[test]
    fn snapshot_uses_opaque_refs_and_semantic_resolution_is_deterministic() {
        let summary = build_dom_summary(
            "<html><body><button data-testid='cancel'>Cancel</button><button data-testid='continue'>Continue</button></body></html>",
        );
        let run_id: RunKind = Id::new();
        let snapshot = build_active_snapshot(&run_id, 4, "blake3:page", Some(&summary), None);
        assert!(snapshot.snapshot.snapshot_id.starts_with("snap_"));
        assert_eq!(snapshot.snapshot.generation, 4);
        assert_eq!(snapshot.snapshot.targets.len(), 2);
        assert!(snapshot
            .snapshot
            .targets
            .iter()
            .all(|target| target.ref_id.starts_with("@e")));

        let ref_id = resolve_semantic_ref(
            &snapshot,
            &SemanticLocator::Role {
                role: "button".into(),
                name: Some("Continue".into()),
                exact: true,
            },
        )
        .unwrap();
        let target = snapshot.targets.get(&ref_id).unwrap();
        assert_eq!(target.target.name.as_deref(), Some("Continue"));
        assert!(target.target.fingerprint.is_some());
    }

    #[test]
    fn ambiguous_semantic_locator_requires_explicit_ref() {
        let summary = build_dom_summary(
            "<html><body><button data-testid='one'>Continue</button><button data-testid='two'>Continue</button></body></html>",
        );
        let run_id: RunKind = Id::new();
        let snapshot = build_active_snapshot(&run_id, 1, "blake3:page", Some(&summary), None);
        let error = resolve_semantic_ref(
            &snapshot,
            &SemanticLocator::Text {
                text: "Continue".into(),
                exact: true,
            },
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::TargetRepairRequired);
    }

    #[test]
    fn snapshot_omits_duplicate_driver_selectors() {
        let summary = build_dom_summary(
            "<button data-testid='continue'>First</button><button data-testid='continue'>Second</button>",
        );
        let run_id: RunKind = Id::new();
        let snapshot = build_active_snapshot(&run_id, 1, "blake3:page", Some(&summary), None);
        assert!(snapshot.snapshot.targets.is_empty());
    }

    #[test]
    fn snapshot_parser_does_not_treat_article_as_anchor_or_embed_css() {
        let summary = build_dom_summary(
            r#"<article><button data-testid='go"] , body'>Go</button></article><a id='real'>Real</a>"#,
        );
        assert_eq!(summary.interactive_elements.len(), 2);
        let button = summary
            .interactive_elements
            .iter()
            .find(|element| element.tag == "button")
            .unwrap();
        assert_eq!(button.selector, r#"[data-testid="go\"] , body"]"#);
        assert!(summary
            .interactive_elements
            .iter()
            .any(|element| element.tag == "a" && element.text.as_deref() == Some("Real")));
    }

    struct ContentDriver {
        html: &'static [u8],
    }

    #[async_trait::async_trait]
    impl BrowserDriver for ContentDriver {
        async fn configure_egress_policy(
            &self,
            _session: &quarry_browser::BrowserSession,
            _policy: BrowserEgressPolicy,
        ) -> QuarryResult<()> {
            Ok(())
        }
        async fn acquire(
            &self,
            lease: &BrowserLease,
        ) -> QuarryResult<quarry_browser::BrowserSession> {
            Ok(quarry_browser::BrowserSession {
                lease: lease.clone(),
                inner: Arc::new(tokio::sync::Mutex::new(
                    quarry_browser::SessionInner::default(),
                )),
            })
        }
        async fn release(&self, _session: quarry_browser::BrowserSession) -> QuarryResult<()> {
            Ok(())
        }
        async fn goto(
            &self,
            _session: &quarry_browser::BrowserSession,
            _url: &str,
        ) -> QuarryResult<()> {
            Ok(())
        }
        async fn content(&self, _session: &quarry_browser::BrowserSession) -> QuarryResult<Bytes> {
            Ok(Bytes::from_static(self.html))
        }
        async fn screenshot(
            &self,
            _session: &quarry_browser::BrowserSession,
            _full_page: bool,
        ) -> QuarryResult<Bytes> {
            Ok(Bytes::new())
        }
        async fn pdf(&self, _session: &quarry_browser::BrowserSession) -> QuarryResult<Bytes> {
            Ok(Bytes::new())
        }
    }

    #[tokio::test]
    async fn changed_target_fingerprint_blocks_snapshot_action_before_click() {
        let old_html = "<html><body><button data-testid='continue'>Continue</button></body></html>";
        let run_id: RunKind = Id::new();
        let summary = build_dom_summary(old_html);
        let active_snapshot = build_active_snapshot(&run_id, 3, "blake3:old", Some(&summary), None);
        let ref_id = active_snapshot.snapshot.targets[0].ref_id.clone();
        let snapshot_id = active_snapshot.snapshot.snapshot_id.clone();
        let driver = Arc::new(ContentDriver {
            html: b"<html><body><button data-testid='continue'>Changed</button></body></html>",
        });
        let runner = ObservationRunner {
            browser: driver.clone(),
            artifacts: None,
            events: None,
            visual_processor: None,
            org_id: "org".into(),
        };
        let lease = BrowserLease {
            lease_id: Id::new(),
            profile_id: Id::new(),
            session_affinity_key: "test".into(),
            proxy_affinity: ProxyAffinity {
                pool: String::new(),
                sticky_key: None,
            },
            ttl_s: 60,
            capabilities: vec![],
            artifact_bucket: String::new(),
            persist_profile: false,
            viewport: None,
            org_id: "org".into(),
        };
        let session = driver.acquire(&lease).await.unwrap();
        let ctx = ObservationContext {
            step: 4,
            current_url: "https://example.test".into(),
            page_hash: "blake3:old".into(),
            previous_page_hash: None,
            previous_screenshot: None,
            previous_url: None,
            previous_title: None,
            previous_dom_node_count: None,
            previous_network_keys: vec![],
            last_egress_sequence: 0,
            observed_action_count: 0,
            challenge_observation_count: 0,
            active_snapshot: Some(active_snapshot),
        };
        let error = runner
            .resolve_snapshot_action(
                &AgentAction::ClickRef {
                    snapshot_id,
                    generation: 3,
                    ref_id,
                },
                &session,
                &ctx,
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::TargetRepairRequired);
    }

    #[test]
    fn devtools_events_build_console_and_network_summaries() {
        let events = vec![
            BrowserDevtoolsEvent {
                sequence: 1,
                tab_id: Some("tab-1".to_owned()),
                category: "console".to_owned(),
                name: "Runtime.consoleAPICalled".to_owned(),
                level: Some("warning".to_owned()),
                method: Some("warning".to_owned()),
                url: None,
                status: None,
                text: Some("slow script".to_owned()),
                timestamp_ms: 1,
                payload: serde_json::json!({}),
            },
            BrowserDevtoolsEvent {
                sequence: 2,
                tab_id: Some("tab-1".to_owned()),
                category: "network".to_owned(),
                name: "Network.requestWillBeSent".to_owned(),
                level: None,
                method: Some("POST".to_owned()),
                url: Some("https://example.com/api".to_owned()),
                status: None,
                text: None,
                timestamp_ms: 2,
                payload: serde_json::json!({}),
            },
            BrowserDevtoolsEvent {
                sequence: 3,
                tab_id: Some("tab-1".to_owned()),
                category: "network".to_owned(),
                name: "Network.responseReceived".to_owned(),
                level: None,
                method: None,
                url: Some("https://example.com/api".to_owned()),
                status: Some(201),
                text: None,
                timestamp_ms: 3,
                payload: serde_json::json!({
                    "response": { "mimeType": "application/json" }
                }),
            },
        ];

        let console = console_summary_from_devtools(&events);
        let network = network_summary_from_devtools(&events);

        assert_eq!(console[0].level, "warning");
        assert_eq!(console[0].text, "slow script");
        assert_eq!(network[0].method, "POST");
        assert_eq!(network[0].status, 201);
        assert_eq!(network[0].content_type.as_deref(), Some("application/json"));
    }

    #[test]
    fn strip_tags_works() {
        assert_eq!(strip_tags("<b>bold</b> text"), "bold text");
    }

    #[test]
    fn agent_action_converts_to_browser_action() {
        let action = AgentAction::Click {
            selector: "#btn".into(),
        };
        let browser_action = ObservationRunner::agent_action_to_browser_action(&action).unwrap();
        match browser_action {
            Action::Click { selector } => assert_eq!(selector, "#btn"),
            _ => panic!("wrong action type"),
        }
    }

    #[test]
    fn forward_action_converts_to_browser_action() {
        let browser_action =
            ObservationRunner::agent_action_to_browser_action(&AgentAction::Forward).unwrap();
        assert!(matches!(browser_action, Action::Forward));
    }

    #[tokio::test]
    async fn put_artifact_if_allowed_skips_zdr_writes() {
        let store = Arc::new(InMemoryStore::new());
        let runner = ObservationRunner {
            browser: Arc::new(crate::tests::MockBrowserDriver),
            artifacts: Some(store.clone()),
            events: None,
            visual_processor: None,
            org_id: "org_a".into(),
        };
        let run_id: RunKind = Id::new();

        let id = runner
            .put_artifact_if_allowed(
                ZdrMode::On,
                &run_id,
                "blake3:page",
                "visual_observation",
                b"{}".to_vec(),
            )
            .await
            .unwrap();

        assert!(id.is_none());
        assert_eq!(store.count("org_a").await.unwrap(), Some(0));
    }

    #[tokio::test]
    async fn put_artifact_if_allowed_stores_visual_observation_when_zdr_off() {
        let store = Arc::new(InMemoryStore::new());
        let runner = ObservationRunner {
            browser: Arc::new(crate::tests::MockBrowserDriver),
            artifacts: Some(store.clone()),
            events: None,
            visual_processor: None,
            org_id: "org_a".into(),
        };
        let run_id: RunKind = Id::new();

        let id = runner
            .put_artifact_if_allowed(
                ZdrMode::Off,
                &run_id,
                "blake3:page",
                "visual_observation",
                b"{}".to_vec(),
            )
            .await
            .unwrap();

        assert!(id.is_some());
        let listed = store
            .list("org_a", &quarry_core::pagination::ListFilter::default())
            .await
            .unwrap();
        assert_eq!(listed.items[0].kind, "visual_observation");
    }

    struct MockVisualProcessor;

    #[async_trait::async_trait]
    impl VisualObservationProcessor for MockVisualProcessor {
        async fn observe(
            &self,
            _input: VisualObservationInput,
        ) -> QuarryResult<VisualObservationResult> {
            Ok(VisualObservationResult {
                artifact: VisualObservationArtifact {
                    version: 1,
                    backend: "mock-opencv5-sidecar".into(),
                    step: 1,
                    previous_available: true,
                    changed: true,
                    change_ratio: 0.42,
                    regions: vec![VisualRegion {
                        x: 1,
                        y: 2,
                        width: 3,
                        height: 4,
                        score: Some(0.5),
                        label: Some("changed".into()),
                    }],
                    metrics: serde_json::json!({ "threshold": 18 }),
                    annotated_artifact_id: None,
                    change_artifact_id: None,
                    related_artifacts: Default::default(),
                },
                annotated_png: Some(b"annotated".to_vec()),
                clean_png: Some(b"clean".to_vec()),
                thumbnail_png: Some(b"thumbnail".to_vec()),
                tiles: Some(serde_json::json!({ "tiles": [{ "x": 0, "y": 0 }] })),
                ocr_preprocessed_png: Some(b"ocr".to_vec()),
                logo_candidate_png: Some(b"logo".to_vec()),
                rendered_palette: Some(serde_json::json!({ "colors": [{ "hex": "#102030" }] })),
            })
        }

        async fn preprocess_page_image(
            &self,
            input: VisualPreprocessInput,
        ) -> QuarryResult<VisualPreprocessResult> {
            Ok(VisualPreprocessResult {
                clean_png: input.image_png,
                metrics: serde_json::Value::Null,
                thumbnail_png: None,
                tiles: None,
                ocr_preprocessed_png: None,
                logo_candidate_png: None,
                rendered_palette: None,
            })
        }
    }

    #[tokio::test]
    async fn execute_stores_visual_artifact_fanout_when_zdr_off() {
        let store = Arc::new(InMemoryStore::new());
        let browser = Arc::new(crate::tests::MockBrowserDriver);
        let runner = ObservationRunner {
            browser: browser.clone(),
            artifacts: Some(store.clone()),
            events: None,
            visual_processor: Some(Arc::new(MockVisualProcessor)),
            org_id: "org_a".into(),
        };
        let lease = BrowserLease {
            lease_id: Id::new(),
            profile_id: Id::new(),
            session_affinity_key: "test".into(),
            proxy_affinity: ProxyAffinity {
                pool: "default".into(),
                sticky_key: None,
            },
            ttl_s: 60,
            capabilities: vec![],
            artifact_bucket: "test".into(),
            persist_profile: false,
            viewport: None,
            org_id: "org_a".into(),
        };
        let session = browser.acquire(&lease).await.unwrap();
        let run_id: RunKind = Id::new();
        let request = AgentActionRequest {
            run_id: run_id.clone(),
            lease_id: lease.lease_id.clone(),
            action: AgentAction::Wait { ms: 0 },
            instruction: None,
            constraints: AgentConstraints {
                max_steps: 3,
                allowed_domains: vec![],
                max_runtime_s: None,
                max_cost_usd: None,
            },
            zdr: ZdrMode::Off,
            extraction_profile: None,
        };
        let mut ctx = ObservationContext {
            step: 1,
            current_url: "https://example.com".into(),
            page_hash: "blake3:page".into(),
            previous_page_hash: None,
            previous_screenshot: Some(b"previous".to_vec()),
            previous_url: None,
            previous_title: None,
            previous_dom_node_count: None,
            previous_network_keys: vec![],
            last_egress_sequence: 0,
            active_snapshot: None,
            observed_action_count: 0,
            challenge_observation_count: 0,
        };

        let observation = runner.execute(&request, &session, &mut ctx).await.unwrap();
        assert!(observation.visual_observation_artifact_id.is_some());

        let listed = store
            .list("org_a", &quarry_core::pagination::ListFilter::default())
            .await
            .unwrap();
        let kinds: std::collections::HashSet<_> =
            listed.items.iter().map(|item| item.kind.as_str()).collect();
        for kind in [
            "screenshot",
            "screenshot_annotated",
            "page_image_clean",
            "thumbnail",
            "tiles",
            "ocr_preprocessed",
            "logo_candidate",
            "rendered_palette",
            "visual_change",
            "visual_observation",
        ] {
            assert!(kinds.contains(kind), "missing visual artifact kind {kind}");
        }

        let obs_id = observation.visual_observation_artifact_id.unwrap();
        let bytes = store.get("org_a", &obs_id).await.unwrap();
        let artifact: VisualObservationArtifact = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(artifact.change_ratio, 0.42);
        assert!(artifact.change_artifact_id.is_some());
        assert!(artifact.related_artifacts.contains_key("visual_change"));
        assert!(artifact.related_artifacts.contains_key("rendered_palette"));
    }
}
