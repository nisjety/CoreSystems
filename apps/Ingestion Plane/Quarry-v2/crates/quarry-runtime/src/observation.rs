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
    BrowserTelemetry, ChallengeSignal, ChallengeStrength, ConsoleLine, DomEvidenceDelta,
    DomSummary,
    ElementClickTarget, ElementFingerprint, EvidenceDelta, ExtractionFieldResult,
    ExtractionProfile, ExtractionResult, ExtractionSource, InteractiveElement, NetworkEntry,
    NetworkEvidenceDelta, ObservationDelta, ProofBundle, ResolvedTargetProof, SemanticLocator,
    SnapshotTarget,
};
use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_core::event::EventType;
use quarry_core::ids::kinds::ArtifactKind;
use quarry_core::redaction::{self, FieldKind, Tally, RULE_VERSION};
use quarry_core::zdr::{self, WriteKind, ZdrMode};
use serde_json::{json, Value};

use crate::artifact_store::ArtifactStore;
use crate::challenge::{self, BlockEpisodes, ChallengeConfig, ChallengeInput, ClassifierMode, EnforceMode};
use crate::events::EventSink;
use crate::observation_redaction::{self, Redactor, ScreenshotReport};
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

#[derive(Default)]
pub struct ObservationContext {
    pub step: u32,
    pub current_url: String,
    pub page_hash: String,
    pub previous_screenshot: Option<Vec<u8>>,
    /// Page identity captured by the last completed observation and used to
    /// compute each new observation's delta. This is separate from
    /// `page_hash`, which is used while an action is executing for artifact
    /// attribution.
    pub previous_observation: Option<ObservationSnapshot>,
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
    /// Strong challenge signals only; a login page or a rate limit is not
    /// counted.
    pub challenge_observation_count: u32,
    /// Per-host challenge bookkeeping for the one-recovery rule. In memory,
    /// run-local, empty after a resume.
    pub block_episodes: BlockEpisodes,
    /// Hosts the run's web policy denies (WS7.G). Set once when the run starts. Every action
    /// installs the run's egress policy again, so the deny list rides along: a sub-resource,
    /// frame or redirect to one of these hosts is refused at the browser boundary. Empty
    /// means the policy installed is exactly the one built from the run's allowed domains.
    pub egress_denied: Vec<String>,
    /// D2-019: the egress boundary the run was started under (its allow list narrowed by the
    /// web policy, the open-web policy, the deny list). When set, every action installs
    /// exactly this policy instead of rebuilding one from `constraints.allowed_domains`,
    /// which would drop the narrowing and the open-web mode after the first step. `None`
    /// (a context built outside `start_run`, the legacy agent loop) keeps the rebuild.
    pub egress_policy: Option<BrowserEgressPolicy>,
    /// D2-065: the kind of the control the keyboard focus is on, as far as this run
    /// has seen it. `None` is "not known" and is the start state, after a resume, and
    /// after any action that can move focus without the run seeing where. A key
    /// press that types a character is masked by this kind when its receipt is
    /// written (`receipt_field_kind`), and as a secret when it is `None`. In memory
    /// only, run-local, like `active_snapshot`.
    pub key_focus: Option<FieldKind>,
}

/// Page identity captured at observation time. Deliberately tiny so carrying
/// it between steps never retains page bodies, screenshots, or DevTools data.
#[derive(Debug, Clone)]
pub struct ObservationSnapshot {
    url: String,
    title: Option<String>,
    dom_node_count: Option<u32>,
    page_hash: Option<String>,
}

impl ObservationSnapshot {
    pub fn capture(
        url: &str,
        title: Option<&str>,
        dom_node_count: Option<u32>,
        page_hash: Option<&str>,
    ) -> Self {
        Self {
            url: url.to_owned(),
            title: title.map(str::to_owned),
            dom_node_count,
            page_hash: page_hash.map(str::to_owned),
        }
    }

    fn delta_against(&self, current: &Self) -> ObservationDelta {
        let url_changed = self.url != current.url;
        let title_changed = self.title != current.title;
        let dom_changed = self
            .dom_node_count
            .zip(current.dom_node_count)
            .is_some_and(|(previous, nodes)| nodes != previous);
        let content_changed = self
            .page_hash
            .as_ref()
            .zip(current.page_hash.as_ref())
            .is_some_and(|(previous, hash)| hash != previous);
        let mut changed_fields = Vec::new();
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
        ObservationDelta {
            changed_fields,
            url_changed,
            title_changed,
            dom_changed,
            content_changed,
        }
    }
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
    /// signal is conservative and evidence-bearing so the Model Plane can decide
    /// whether to ask for a user handoff or use another source; it is not a
    /// success/failure verdict for the business action. A thin wrapper over
    /// `challenge::classify` for a caller that has only text (no HTML, no
    /// status): the runner itself passes both. `QUARRY_CHALLENGE_CLASSIFIER=
    /// legacy` selects the old substring classifier.
    pub fn classify_challenge(
        url: &str,
        title: Option<&str>,
        text: Option<&str>,
    ) -> Option<ChallengeSignal> {
        match ChallengeConfig::from_env().classifier {
            ClassifierMode::Legacy => challenge::classify_legacy(url, title, text),
            ClassifierMode::V2 => challenge::classify(&ChallengeInput {
                url,
                title,
                html: None,
                text_snippet: text,
                main_doc_status: None,
            }),
        }
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
            | AgentAction::Forward
            | AgentAction::Reload { .. } => ActionOutcome::unknown(
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
            AgentAction::Reload { ignore_cache } => Action::Reload {
                ignore_cache: *ignore_cache,
            },
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
        let egress = match ctx.egress_policy.as_ref() {
            Some(resolved) => resolved.clone(),
            None => BrowserEgressPolicy::from_allowed_domains(&request.constraints.allowed_domains)
                .with_denied_domains(&ctx.egress_denied),
        };
        self.browser.configure_egress_policy(session, egress).await?;
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
        // The one-recovery rule. A host that kept challenging after its one wait
        // or reload gets no more navigation and no more interaction. Checked
        // before anything touches the browser, so a refused action has no effect.
        let current_host = challenge::host_of(&ctx.current_url).unwrap_or_default();
        if let Err(error) =
            ctx.block_episodes
                .reject_if_exhausted(&request.action, &current_host, request.zdr)
        {
            match ctx.block_episodes.config().enforce {
                EnforceMode::Enforce => return Err(error),
                // Validation period: say what would have happened (a reason,
                // never a host or URL) and let the action through.
                EnforceMode::Log => {
                    tracing::warn!(reason = "challenge_loop", "quarry.challenge.would_reject");
                }
            }
        }
        ctx.block_episodes
            .note_recovery_action(&request.action, &ctx.current_url, request.zdr);

        // WS12.S2: the field kind of the control a typing action goes to, read from the
        // snapshot the action names; `ctx.active_snapshot` is replaced further down.
        // D2-065: a key press is read against the control the run last focused.
        let typed_kind = receipt_field_kind(ctx, &request.action);
        // Where the keyboard focus is once this action has run. Unknown until it has: an
        // action that fails, or returns early, leaves the focus unknown (fails closed).
        let focus_after = key_focus_after(ctx, &request.action, typed_kind);
        ctx.key_focus = None;
        let (effective_action, target_resolution, verified_target_action, transfer_target) = self
            .resolve_snapshot_action(&request.action, session, ctx)
            .await?;

        if let Some(events) = &self.events {
            events
                .emit_for_zdr(
                    request.zdr,
                    run_id.clone(),
                    EventType::ActionStarted,
                    json!({
                        "step": ctx.step,
                        "action": observation_redaction::action_for_event(
                            &request.action,
                            typed_kind,
                            // X-19: the event keeps what the stored receipt keeps.
                            quarry_core::redaction::action::record_policy(),
                            redaction::mode(),
                            &mut Tally::new(),
                        ),
                    }),
                    format!("{}:action:{}:started", run_id, ctx.step),
                )
                .await;
        }

        let mut screenshot_artifact_id: Option<ArtifactKind> = None;
        // WS12.S5: what the screenshot mask did (counts only), for the stamp.
        let mut screenshot_report: Option<ScreenshotReport> = None;
        let mut visual_observation_artifact_id: Option<ArtifactKind> = None;
        let mut download_artifact_id: Option<ArtifactKind> = None;
        let policy_denials: Vec<String> = Vec::new();
        let mut current_screenshot: Option<Vec<u8>> = None;

        // SC4 D1: an action that only reads the page observes the session as it is, also
        // when it has no page yet (a run that never navigated, one resumed from a
        // checkpoint with no URL): the driver gives it a tab on `about:blank`, navigated
        // nowhere, instead of the step failing with `no_current_page`. An action that
        // acts on a page still needs one.
        if action_only_observes(&effective_action) {
            self.browser.ensure_page(session).await?;
        }

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
                    // WS12.S5: a withheld screenshot is a `SecurityBlocked` error
                    // (403) with the same reason the observation note carries.
                    let masked = self.browser.screenshot_masked(session, *full_page).await?;
                    screenshot_report = Some(ScreenshotReport::of(&masked));
                    let bytes = masked.bytes.ok_or_else(|| {
                        quarry_browser::MaskedScreenshot::withheld_error(
                            masked.note.unwrap_or("screenshot_withheld:sensitive_field_unresolved"),
                        )
                    })?;
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
                AgentAction::Evaluate { .. } => {
                    // Raw script is refused before any driver call. The edge
                    // gates it first; this keeps library callers (agent loop,
                    // compiled procedures) from reaching the driver either.
                    return Err(QuarryError::raw_script_denied());
                }
                AgentAction::Back => {
                    self.browser.back(session).await?;
                }
                AgentAction::Forward => {
                    self.browser.forward(session).await?;
                }
                AgentAction::Reload { ignore_cache } => {
                    self.browser.reload(session, *ignore_cache).await?;
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

        ctx.key_focus = focus_after;

        // SC4 T3: a page that did not answer one of the reads below in time does not
        // answer the next one either (a load that still holds it, a renderer in a
        // loop). Each read has its own deadline, so reading on made a step wait for
        // their sum (page state + content + accessibility tree: 20 s at a 0.5 scale,
        // where the tree's own deadline was 5 s). The first timeout ends the page
        // reads, and the observation fails with it instead of the last one.
        let mut stalled: Option<QuarryError> = None;
        let page_state = self.page_state(session, &mut stalled).await;
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
        // The cursor moves only once the observation that carries these receipts is
        // returned: a step that fails below leaves them for the next observation.
        let next_egress_sequence = egress_receipts
            .last()
            .map_or(ctx.last_egress_sequence, |last_receipt| {
                ctx.last_egress_sequence.max(last_receipt.sequence)
            });
        // WS12.S2: everything below that is returned to the caller passes the Live profile.
        let mut redactor = Redactor::new();
        let mut dialogs = self.browser.dialogs(session).await.unwrap_or_default();
        redactor.dialogs(&mut dialogs);

        let html_bytes = if stalled.is_some() {
            None
        } else {
            match self.browser.content(session).await {
                Ok(bytes) => Some(bytes),
                Err(error) => {
                    note_stalled_read(&mut stalled, error);
                    None
                }
            }
        };
        if let Some(error) = stalled {
            return Err(observation_read_error(&request.action, error));
        }
        if let Some(bytes) = html_bytes.as_ref() {
            // Proof must identify the bytes actually observed, not the
            // synthetic run hash used before the first page is loaded.
            ctx.page_hash = format!("blake3:{}", blake3::hash(bytes).to_hex());
        }

        let mut dom_summary = html_bytes.as_ref().map(|bytes| {
            let html_str = String::from_utf8_lossy(bytes);
            build_dom_summary(&html_str)
        });
        if let Some(summary) = dom_summary.as_mut() {
            redactor.dom_summary(summary);
        }
        let mut native_projection = match self.browser.native_page_projection(session).await {
            Ok(projection) => Some(projection),
            // Older/read-only drivers can continue to expose the explicitly
            // marked compatibility snapshot. A driver that advertises native
            // browser operation must not silently downgrade because its AX
            // query failed mid-run.
            Err(error) if error.code == ErrorCode::Unsupported => None,
            Err(error) => return Err(observation_read_error(&request.action, error)),
        };
        ctx.last_egress_sequence = next_egress_sequence;
        if let Some(projection) = native_projection.as_mut() {
            redactor.projection(projection);
        }
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

        let mut title = page_state.title.or_else(|| {
            html_bytes.as_ref().and_then(|bytes| {
                let html_str = String::from_utf8_lossy(bytes);
                extract_title(&html_str)
            })
        });
        redactor.title(&mut title);

        let extraction_profile = request.extraction_profile.clone();
        let mut extraction_result = extraction_profile.as_ref().and_then(|profile| {
            html_bytes
                .as_ref()
                .and_then(|bytes| extract_profile(bytes, profile))
        });
        if let Some(result) = extraction_result.as_mut() {
            redactor.extraction(result);
        }

        if screenshot_artifact_id.is_none() {
            if let Ok(masked) = self.browser.screenshot_masked(session, false).await {
                screenshot_report = Some(ScreenshotReport::of(&masked));
                if let Some(bytes) = masked.bytes {
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
                } else {
                    // WS12.S5: withheld (`screenshot_withheld:sensitive_field_unresolved`
                    // and the other closed causes). No artifact, no sidecar input, and
                    // no stale previous frame for the next step to diff against.
                    ctx.previous_screenshot = None;
                    // zdr-log-ok: a closed note string, no page-derived value
                    tracing::info!(
                        note = masked.note.unwrap_or("screenshot_withheld"),
                        "quarry.screenshot.withheld"
                    );
                }
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
                                    error_class = e.code.wire_name(),
                                    "visual observation artifact write failed"
                                ),
                            }
                        }
                        Err(e) => tracing::warn!(
                            error_class = e.code.wire_name(),
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

        let current_observation = ObservationSnapshot::capture(
            &ctx.current_url,
            title.as_deref(),
            dom_summary.as_ref().map(|summary| summary.node_count),
            (!ctx.page_hash.is_empty()).then_some(ctx.page_hash.as_str()),
        );
        // The first completed observation is the baseline: everything that
        // follows is diffed against it instead of being reported as changed.
        let delta = match &ctx.previous_observation {
            Some(previous) => previous.delta_against(&current_observation),
            None => ObservationDelta {
                changed_fields: vec!["initial_observation".to_string()],
                url_changed: false,
                title_changed: false,
                dom_changed: false,
                content_changed: false,
            },
        };
        let network_summary = network_summary_from_devtools(&devtools_events);
        let network_delta = network_evidence_delta(&ctx.previous_network_keys, &network_summary);
        let evidence_delta = EvidenceDelta {
            version: 1,
            step: ctx.step,
            dom: DomEvidenceDelta {
                previous_node_count: ctx
                    .previous_observation
                    .as_ref()
                    .and_then(|previous| previous.dom_node_count),
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
                tracing::warn!(error_class = error.code.wire_name(), "evidence delta artifact write failed");
                None
            });
        let action_outcome = Self::outcome_for_action(&request.action, &ctx.current_url);
        let text_snippet = dom_summary
            .as_ref()
            .and_then(|summary| summary.text_snippet.as_deref());
        let mut challenge = match ctx.block_episodes.config().classifier {
            ClassifierMode::Legacy => {
                challenge::classify_legacy(&ctx.current_url, title.as_deref(), text_snippet)
            }
            ClassifierMode::V2 => {
                let page_html = html_bytes
                    .as_ref()
                    .map(|bytes| String::from_utf8_lossy(bytes));
                let classified = challenge::classify(&ChallengeInput {
                    url: &ctx.current_url,
                    title: title.as_deref(),
                    html: page_html.as_deref(),
                    text_snippet,
                    main_doc_status: challenge::main_document_status(
                        &devtools_events,
                        &ctx.current_url,
                    ),
                });
                let host = challenge::host_of(&ctx.current_url).unwrap_or_default();
                let fingerprint = ctx.page_hash.clone();
                ctx.block_episodes
                    .observe_after(&request.action, &host, request.zdr, classified, &fingerprint)
            }
        };
        if let Some(signal) = challenge.as_mut() {
            redactor.challenge(signal);
        }
        ctx.observed_action_count = ctx.observed_action_count.saturating_add(1);
        if challenge
            .as_ref()
            .is_some_and(|signal| signal.strength == ChallengeStrength::Strong)
        {
            ctx.challenge_observation_count = ctx.challenge_observation_count.saturating_add(1);
        }
        let driver_telemetry = match self.browser.telemetry(session).await {
            Ok(telemetry) => telemetry,
            Err(error) => {
                // A telemetry sample must not turn a completed browser action
                // into a retryable effect. Keep the unknown/default fields and
                // retain the real execution outcome separately.
                tracing::warn!(error_class = error.code.wire_name(), "browser telemetry sample failed");
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
            source_url: redactor.live_url(&ctx.current_url),
            run_id: run_id.clone(),
            step: ctx.step,
            action_outcome: action_outcome.clone(),
            artifact_ids,
            content_fingerprint: (!ctx.page_hash.is_empty()).then(|| ctx.page_hash.clone()),
            challenge: challenge.clone(),
            target_resolution: target_resolution.clone(),
            evidence_delta_artifact_id: evidence_delta_artifact_id.clone(),
            redaction_rule_version: Some(RULE_VERSION.to_owned()),
            observed_at: Utc::now(),
        });
        let observation_url = redactor.live_url(&ctx.current_url);
        let mut console_summary = console_summary_from_devtools(&devtools_events);
        redactor.console(&mut console_summary);
        if let Some(report) = screenshot_report {
            redactor.screenshot(report);
        }
        let redaction_stamp = redactor.stamp();
        if redaction_stamp.values_masked > 0 {
            // zdr-log-ok: counts and the process mode only, no page-derived value
            tracing::info!(
                values_masked = redaction_stamp.values_masked,
                rule_count = redaction_stamp.by_rule.len(),
                probe_ms = redaction_stamp.probe_ms,
                shadow = redaction_stamp.mode == redaction::Mode::Shadow,
                "quarry.redaction.observation"
            );
        }
        let observation = BrowserObservation {
            run_id: run_id.clone(),
            step: ctx.step,
            url: observation_url,
            title,
            snapshot,
            dom_summary,
            screenshot_artifact_id,
            visual_observation_artifact_id,
            evidence_delta_artifact_id,
            console_summary,
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
            redaction: Some(redaction_stamp),
            observed_at: Utc::now(),
        };

        ctx.previous_observation = Some(current_observation);
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
                        "url": observation_redaction::event_url(
                            &observation.url,
                            redaction::mode(),
                            &mut Tally::new(),
                        ),
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
        let body = observation_redaction::redact_json_artifact(
            value,
            redaction::mode(),
            &mut Tally::new(),
        )
        .map_err(|e| {
            QuarryError::new(
                ErrorCode::Internal,
                format!("serialize visual artifact {kind}: {e}"),
            )
        })?;
        self.put_artifact_if_allowed(zdr_mode, run_id, page_hash, kind, body)
            .await
    }

    /// The page's URL and title, or nothing. A timeout is recorded in `stalled`.
    async fn page_state(
        &self,
        session: &quarry_browser::BrowserSession,
        stalled: &mut Option<QuarryError>,
    ) -> PageState {
        self.browser
            .evaluate(
                session,
                r#"(() => ({
                    url: window.location && window.location.href ? window.location.href : null,
                    title: document && document.title ? document.title : null
                }))()"#,
            )
            .await
            .map_err(|error| note_stalled_read(stalled, error))
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

/// SC4 D1: the actions that only read the page as it is (a wait, a capture). They observe
/// a session with no page yet on a blank tab (`BrowserDriver::ensure_page`).
fn action_only_observes(action: &AgentAction) -> bool {
    matches!(
        action,
        AgentAction::Wait { .. }
            | AgentAction::Screenshot { .. }
            | AgentAction::Pdf
            | AgentAction::GetContent
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
                // Native AX projections carry their own authority; a
                // string-scanned click index would be a second numbering.
                click_index: None,
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
                click_index: element.click_index,
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

/// WS12.S2 (D12-7): the field kind of the control a typing action goes to, read
/// from the snapshot the action names. Call it before the action resolves:
/// `execute` replaces `ctx.active_snapshot` with the post-action snapshot.
///
/// A native target carries the kind the field probe decided (`None` for a
/// control that takes no typed text). A scanner target (a driver without a
/// native projection) has no `type` attribute to read, so only its labels
/// classify it; an input or textarea the labels do not explain is `Secret`,
/// which fails closed. `None` for an action that does not type into a snapshot
/// target, or when the snapshot does not hold the target.
pub fn typed_field_kind(
    snapshot: Option<&ActiveSnapshot>,
    action: &AgentAction,
) -> Option<FieldKind> {
    let snapshot = snapshot?;
    let ref_id = match action {
        AgentAction::TypeRef { ref_id, .. } | AgentAction::FrameTypeRef { ref_id, .. } => {
            ref_id.clone()
        }
        AgentAction::TypeSemantic { locator, .. } => resolve_semantic_ref(snapshot, locator).ok()?,
        _ => return None,
    };
    held_target_kind(snapshot, &ref_id, false)
}

/// The field kind of a held snapshot target. `for_click` is a click: a target that
/// is not a text control then has no kind (`None`: a click on a button says nothing
/// about where the keyboard focus is), where a typing action treats it as an
/// ordinary control.
fn held_target_kind(
    snapshot: &ActiveSnapshot,
    ref_id: &str,
    for_click: bool,
) -> Option<FieldKind> {
    let held = snapshot.targets.get(ref_id)?;
    if let Some(native) = &held.native_target {
        return native.field_kind;
    }
    let target = &held.target;
    let facts = quarry_core::redaction::FieldFacts {
        placeholder: target.placeholder.clone(),
        accessible_name: target.name.clone().or_else(|| target.text.clone()),
        ..Default::default()
    };
    let kind = quarry_core::redaction::classify_field(&facts);
    if kind.is_sensitive() {
        return Some(kind);
    }
    if matches!(target.tag.as_str(), "input" | "textarea") {
        return Some(FieldKind::Secret);
    }
    (!for_click).then_some(FieldKind::Unknown)
}

/// D2-065: the field kind a stored receipt (and the `ActionStarted` event) masks
/// this action's text by. A typing action reads the control it names from the
/// snapshot (`typed_field_kind`). A key press reads the control the run last
/// focused (`ObservationContext::key_focus`); `None` when the run cannot say.
/// Call it before `execute`, which moves both.
#[must_use]
pub fn receipt_field_kind(ctx: &ObservationContext, action: &AgentAction) -> Option<FieldKind> {
    match action {
        AgentAction::Press { .. } => ctx.key_focus,
        _ => typed_field_kind(ctx.active_snapshot.as_ref(), action),
    }
}

/// D2-065: where the keyboard focus is once `action` has run, as far as the run can
/// tell; `None` when it cannot. `typed` is [`receipt_field_kind`] for the action.
///
/// Typing into a control, or clicking a text control, puts focus there (a raw
/// selector only counts when its own words name a sensitive control). A key press
/// that edits or moves inside the control, and a wait or a scroll, leave focus where
/// it was. Everything else may move it (a click on a link, a navigation, `Tab`,
/// `Enter`, a script on the page) and the answer is "unknown".
fn key_focus_after(
    ctx: &ObservationContext,
    action: &AgentAction,
    typed: Option<FieldKind>,
) -> Option<FieldKind> {
    let selector_focus = |selector: &str| {
        let kind = quarry_core::redaction::action::selector_kind(selector);
        kind.is_sensitive().then_some(kind)
    };
    match action {
        AgentAction::TypeRef { .. }
        | AgentAction::FrameTypeRef { .. }
        | AgentAction::TypeSemantic { .. } => typed,
        AgentAction::Type { selector, .. } | AgentAction::Click { selector } => {
            selector_focus(selector)
        }
        AgentAction::ClickRef { ref_id, .. } | AgentAction::FrameClickRef { ref_id, .. } => ctx
            .active_snapshot
            .as_ref()
            .and_then(|snapshot| held_target_kind(snapshot, ref_id, true)),
        AgentAction::ClickSemantic { locator, .. } => {
            let snapshot = ctx.active_snapshot.as_ref()?;
            let ref_id = resolve_semantic_ref(snapshot, locator).ok()?;
            held_target_kind(snapshot, &ref_id, true)
        }
        AgentAction::Press { key } => {
            let last = key.rsplit('+').find(|part| !part.is_empty()).unwrap_or(key);
            let lower = last.trim().to_ascii_lowercase();
            let moves_focus = matches!(lower.as_str(), "tab" | "enter" | "return" | "escape" | "esc")
                || (lower.len() >= 2
                    && lower.starts_with('f')
                    && lower[1..].bytes().all(|b| b.is_ascii_digit()));
            if moves_focus {
                None
            } else {
                ctx.key_focus
            }
        }
        AgentAction::Wait { .. }
        | AgentAction::WaitFor { .. }
        | AgentAction::WaitForRef { .. }
        | AgentAction::FrameWaitForRef { .. }
        | AgentAction::WaitForSemantic { .. }
        | AgentAction::Scroll { .. }
        | AgentAction::MouseWheel { .. } => ctx.key_focus,
        _ => None,
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

/// Keep the first page read that timed out (SC4 T3). Any other failure of an
/// optional read is dropped, as before.
fn note_stalled_read(stalled: &mut Option<QuarryError>, error: QuarryError) {
    if error.code == ErrorCode::Timeout && stalled.is_none() {
        *stalled = Some(error);
    }
}

/// The step's error for an observation read that timed out after `action` ran. The
/// read itself changed nothing, but the step did whatever its action did: after an
/// action that only reads or waits nothing changed and the step may be repeated;
/// after any other (a click that landed, a navigation) the effect is unknown and a
/// blind retry could act twice. Other errors pass through.
fn observation_read_error(action: &AgentAction, mut error: QuarryError) -> QuarryError {
    if error.code != ErrorCode::Timeout {
        return error;
    }
    let only_reads = matches!(
        action,
        AgentAction::Wait { .. }
            | AgentAction::WaitFor { .. }
            | AgentAction::WaitForRef { .. }
            | AgentAction::FrameWaitForRef { .. }
            | AgentAction::WaitForSemantic { .. }
            | AgentAction::Screenshot { .. }
            | AgentAction::Pdf
            | AgentAction::GetContent
    );
    let effect = if only_reads { "none" } else { "unknown" };
    error.retryable = only_reads;
    if let Some(Value::Object(details)) = error.details.as_mut() {
        details.insert("effect".to_owned(), Value::from(effect));
    }
    error
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

/// Build the bounded agent-facing DOM summary.
///
/// This scanner has no layout engine, so viewport awareness is implemented at
/// the attribute level: elements a renderer would not paint (`hidden`,
/// `inert`, `aria-hidden="true"`, `input[type=hidden]`, and inline styles
/// forcing invisibility such as `display:none`) are dropped before any slot
/// is consumed. Stylesheets and offscreen geometry stay out of reach by
/// design — the heuristic removes only what is explicitly marked invisible.
/// Surviving elements receive stable 1-based click indices aligned with the
/// `@eN` snapshot ref convention so action planning can reuse them directly.
/// Public so the WS4 property suite can pin its invariants from outside
/// the crate; production callers remain internal.
pub fn build_dom_summary(html: &str) -> DomSummary {
    let node_count = html.matches('<').count() as u32;
    let mut interactive_elements = Vec::new();

    for tag in &["a", "button", "input", "select", "textarea"] {
        for idx in tag_openings(html, tag) {
            let remaining = &html[idx..];
            let end = remaining.find('>').unwrap_or(remaining.len());
            let element_str = &remaining[..end];

            if !is_rendered_element(tag, element_str) {
                continue;
            }
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
                // Assigned when the dense click map is built below.
                click_index: None,
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

    // Dense, 1-based action-planning indices over exactly the elements that
    // survived filtering. Row N names `@eN`, the same ref-id form agent
    // snapshots mint, so a planner can carry an index straight into a
    // snapshot-backed action without renumbering. Filtered-out elements never
    // consume an index, keeping the numbering gap-free and stable for a given
    // page state.
    let click_map = if interactive_elements.is_empty() {
        None
    } else {
        Some(
            interactive_elements
                .iter_mut()
                .enumerate()
                .map(|(offset, element)| {
                    let index = offset as u32 + 1;
                    element.click_index = Some(index);
                    ElementClickTarget {
                        ref_id: format!("@e{index}"),
                        index,
                        tag: element.tag.clone(),
                        selector: Some(element.selector.clone()),
                        name: element
                            .accessible_name
                            .clone()
                            .or_else(|| element.aria_label.clone())
                            .or_else(|| element.text.clone()),
                    }
                })
                .collect(),
        )
    };

    let text_snippet = html
        .find("<body")
        .and_then(|start| html[start..].find('>').map(|i| start + i + 1))
        .map(|body_start| {
            let body = &html[body_start..];
            let stripped = strip_tags(body);
            let trimmed = stripped.trim();
            text_snippet_of(trimmed)
        })
        .filter(|s| !s.is_empty());

    DomSummary {
        node_count,
        interactive_elements,
        text_snippet,
        click_map,
    }
}

/// Attribute-level approximation of "would a renderer paint this?". Only
/// explicit invisibility markers drop an element; absence of styling evidence
/// always keeps it (fail-open towards discoverability, never silently hides
/// actionable controls the snapshot authority did not).
fn is_rendered_element(tag: &str, element: &str) -> bool {
    if extract_attr(element, "hidden").is_some() || has_unquoted_token(element, "hidden") {
        return false;
    }
    if has_unquoted_token(element, "inert") {
        return false;
    }
    if extract_attr(element, "aria-hidden") == Some("true") {
        return false;
    }
    if tag == "input" && extract_attr(element, "type") == Some("hidden") {
        return false;
    }
    !inline_style_hides(element)
}

fn inline_style_hides(element: &str) -> bool {
    let Some(style) = extract_attr(element, "style") else {
        return false;
    };
    let normalized: String = style
        .to_ascii_lowercase()
        .chars()
        .filter(|character| !character.is_ascii_whitespace())
        .collect();
    css_declares(&normalized, "display", "none")
        || css_declares(&normalized, "visibility", "hidden")
        || css_declares(&normalized, "opacity", "0")
}

/// Match a full CSS declaration `property:value` in whitespace-stripped,
/// lowercased inline style text. Requires a declaration boundary after the
/// value so `opacity:0` does not swallow `opacity:0.5`.
fn css_declares(normalized_style: &str, property: &str, value: &str) -> bool {
    let needle = format!("{property}:{value}");
    normalized_style.match_indices(&needle).any(|(position, _)| {
        let rest = &normalized_style[position + needle.len()..];
        rest.is_empty() || rest.starts_with(';') || rest.starts_with('}')
    })
}

/// True when `name` appears as a bare boolean-style attribute (e.g. `<button
/// hidden>`), ignoring anything inside quoted attribute values so a title
/// like "the hidden gem" cannot trigger a false match.
fn has_unquoted_token(element: &str, name: &str) -> bool {
    let mut quote: Option<char> = None;
    let mut candidate = String::new();
    for character in element.chars() {
        match quote {
            Some(open) if character == open => quote = None,
            Some(_) => {}
            None if character == '"' || character == '\'' => quote = Some(character),
            None if character.is_ascii_whitespace() || character == '>' => {
                if candidate == name {
                    return true;
                }
                candidate.clear();
            }
            None => candidate.push(character),
        }
    }
    candidate == name
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

/// The first 500 bytes of the visible text, cut on a character boundary. Slicing
/// at byte 500 panics when a multi-byte character straddles it (any CJK, accented
/// or emoji text), which took down the request for a page in that language.
fn text_snippet_of(trimmed: &str) -> String {
    const MAX_BYTES: usize = 500;
    if trimmed.len() <= MAX_BYTES {
        return trimmed.to_string();
    }
    let mut end = MAX_BYTES;
    while !trimmed.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &trimmed[..end])
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
    use crate::challenge::ChallengeConfig;
    use bytes::Bytes;
    use quarry_browser::BrowserDriver;
    use quarry_core::contracts::{AgentConstraints, ChallengeKind};
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
        // The text-only wrapper: a title asking for proof of humanity on a
        // short page is two markers (a title hit counts twice).
        let signal = ObservationRunner::classify_challenge(
            "https://example.test/challenge",
            Some("Verify you are human"),
            Some("captcha required"),
        )
        .expect("a human check should be classified");
        assert_eq!(signal.kind, ChallengeKind::Captcha);
        assert_eq!(signal.strength, ChallengeStrength::Strong);
        assert!(signal.requires_escalation);
        assert_eq!(
            signal.block_reason,
            Some(quarry_core::contracts::BlockReason::UnexpectedBotError)
        );
        assert!(signal.confidence >= 0.85);
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
    fn a_login_page_through_the_wrapper_is_not_an_escalation() {
        let signal = ObservationRunner::classify_challenge(
            "https://example.test/login",
            Some("Sign in"),
            Some("Sign in to your account"),
        );
        assert!(signal.is_none_or(|s| !s.requires_escalation));
    }

    const TURNSTILE_HTML: &[u8] = b"<html><head><title>Just a moment...</title></head><body><div class=\"cf-turnstile\"></div><script src=\"https://challenges.cloudflare.com/turnstile/v0/api.js\"></script></body></html>";
    const LEGACY_HTML: &[u8] = b"<html><head><title>Verify you are human</title></head><body>captcha</body></html>";
    const LOGIN_HTML: &[u8] = b"<html><head><title>Sign in</title></head><body><form><input type=\"password\" name=\"p\"></form></body></html>";

    fn challenge_lease() -> BrowserLease {
        BrowserLease {
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
        }
    }

    fn challenge_request(lease: &BrowserLease, action: AgentAction, zdr: ZdrMode) -> AgentActionRequest {
        AgentActionRequest {
            run_id: Id::new(),
            lease_id: lease.lease_id.clone(),
            action,
            instruction: None,
            constraints: AgentConstraints {
                max_steps: 10,
                allowed_domains: vec![],
                max_runtime_s: None,
                max_cost_usd: None,
            },
            zdr,
            extraction_profile: None,
        }
    }

    fn challenge_runner(driver: &Arc<ContentDriver>) -> ObservationRunner {
        ObservationRunner {
            browser: driver.clone(),
            artifacts: None,
            events: None,
            visual_processor: None,
            org_id: "org".into(),
        }
    }

    fn challenge_ctx(config: ChallengeConfig) -> ObservationContext {
        ObservationContext {
            block_episodes: BlockEpisodes::new(config),
            ..Default::default()
        }
    }

    /// Every way the planner can act on a page or navigate to it, snapshot-ref
    /// and semantic forms included: all of them are refused on an exhausted host.
    fn refused_on_an_exhausted_page() -> Vec<AgentAction> {
        let snapshot = || "snap".to_owned();
        let locator = || quarry_core::contracts::SemanticLocator::Text { text: "go".into(), exact: false };
        vec![
            AgentAction::Navigate { url: "https://blocked.example/other".into() },
            AgentAction::Navigate { url: "https://blocked.example/page".into() },
            AgentAction::Click { selector: "#go".into() },
            AgentAction::ClickPoint { x: 1.0, y: 2.0 },
            AgentAction::ClickRef { snapshot_id: snapshot(), generation: 1, ref_id: "e1".into() },
            AgentAction::FrameClickRef { snapshot_id: snapshot(), generation: 1, frame_id: "f1".into(), ref_id: "e1".into() },
            AgentAction::ClickSemantic { snapshot_id: snapshot(), generation: 1, locator: locator() },
            AgentAction::Type { selector: "#q".into(), text: "x".into() },
            AgentAction::TypeRef { snapshot_id: snapshot(), generation: 1, ref_id: "e1".into(), text: "x".into() },
            AgentAction::FrameTypeRef { snapshot_id: snapshot(), generation: 1, frame_id: "f1".into(), ref_id: "e1".into(), text: "x".into() },
            AgentAction::TypeSemantic { snapshot_id: snapshot(), generation: 1, locator: locator(), text: "x".into() },
            AgentAction::Press { key: "Enter".into() },
            AgentAction::Select { selector: "#s".into(), value: "v".into() },
            AgentAction::SelectRef { snapshot_id: snapshot(), generation: 1, ref_id: "e1".into(), value: "v".into() },
            AgentAction::FrameSelectRef { snapshot_id: snapshot(), generation: 1, frame_id: "f1".into(), ref_id: "e1".into(), value: "v".into() },
            AgentAction::SelectSemantic { snapshot_id: snapshot(), generation: 1, locator: locator(), value: "v".into() },
            AgentAction::UploadRef { snapshot_id: snapshot(), generation: 1, ref_id: "e1".into(), artifact_id: Id::new(), approval_grant_id: "g".into() },
            AgentAction::DownloadRef { snapshot_id: snapshot(), generation: 1, ref_id: "e1".into(), approval_grant_id: "g".into() },
            // Raw script is denied anyway; on an exhausted host the rule answers first.
            AgentAction::Evaluate { script: "document.forms[0].submit()".into() },
        ]
    }

    /// Collects tracing events as text, for the thread that installs it.
    #[derive(Clone, Default)]
    struct EventLog(Arc<std::sync::Mutex<Vec<String>>>);

    impl EventLog {
        fn lines(&self) -> Vec<String> {
            self.0.lock().unwrap().clone()
        }

        fn count(&self, message: &str) -> usize {
            self.lines().iter().filter(|line| line.contains(message)).count()
        }
    }

    impl tracing::Subscriber for EventLog {
        fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
        fn event(&self, event: &tracing::Event<'_>) {
            struct Fields(String);
            impl tracing::field::Visit for Fields {
                fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                    if !self.0.is_empty() {
                        self.0.push(' ');
                    }
                    self.0.push_str(&format!("{}={:?}", field.name(), value));
                }
            }
            let mut fields = Fields(String::new());
            event.record(&mut fields);
            self.0
                .lock()
                .unwrap()
                .push(format!("{} {}", event.metadata().level(), fields.0));
        }
        fn enter(&self, _: &tracing::span::Id) {}
        fn exit(&self, _: &tracing::span::Id) {}
    }

    #[tokio::test]
    async fn a_same_url_navigate_alone_spends_the_recovery_at_the_runner() {
        let driver = Arc::new(ContentDriver { html: TURNSTILE_HTML, atomic_targets: true, ..Default::default() });
        let runner = challenge_runner(&driver);
        let lease = challenge_lease();
        let session = driver.acquire(&lease).await.unwrap();
        let mut ctx = challenge_ctx(ChallengeConfig::enforcing());
        let go = |url: &str| challenge_request(&lease, AgentAction::Navigate { url: url.into() }, ZdrMode::Off);

        let first = runner
            .execute(&go("https://blocked.example/page"), &session, &mut ctx)
            .await
            .unwrap();
        assert_eq!(
            first.challenge.expect("challenge").recovery,
            quarry_core::contracts::ChallengeRecovery::Available
        );
        // The only recovery here is a reload of the page the browser is on (a
        // fragment does not make it another page). No wait anywhere.
        let reload = runner
            .execute(&go("https://blocked.example/page#again"), &session, &mut ctx)
            .await
            .expect("a reload of the same URL is allowed once");
        let signal = reload.challenge.expect("still challenged");
        assert_eq!(signal.block_reason, Some(quarry_core::contracts::BlockReason::ChallengeLoop));
        assert_eq!(signal.recovery, quarry_core::contracts::ChallengeRecovery::Exhausted);

        let reached_before = driver.calls();
        for action in refused_on_an_exhausted_page() {
            let error = runner
                .execute(&challenge_request(&lease, action, ZdrMode::Off), &session, &mut ctx)
                .await
                .expect_err("an exhausted host must refuse");
            assert_eq!(error.code, ErrorCode::ChallengeDetected);
        }
        assert_eq!(driver.calls(), reached_before);
    }

    /// The runner, not the episode table, decides which host a step is on: after
    /// `Back` it is the landing page's host (read from the page), so an exhausted
    /// host stays refused when the history step lands on it.
    #[tokio::test]
    async fn a_history_step_onto_an_exhausted_host_leaves_every_interaction_refused() {
        async fn run(landing: &'static str) -> (usize, usize, Result<(), ErrorCode>) {
            let driver = Arc::new(ContentDriver {
                html: TURNSTILE_HTML,
                atomic_targets: true,
                back_lands_on: Some(landing),
                ..Default::default()
            });
            let runner = challenge_runner(&driver);
            let lease = challenge_lease();
            let session = driver.acquire(&lease).await.unwrap();
            let mut ctx = challenge_ctx(ChallengeConfig::enforcing());
            let step = |action: AgentAction| challenge_request(&lease, action, ZdrMode::Off);
            // blocked.example: a wall, then the one reload that is still a wall.
            for url in ["https://blocked.example/page", "https://blocked.example/page#again"] {
                runner
                    .execute(&step(AgentAction::Navigate { url: url.into() }), &session, &mut ctx)
                    .await
                    .unwrap();
            }
            // Leave it for another host, then step back in history.
            runner
                .execute(&step(AgentAction::Navigate { url: "https://elsewhere.example/".into() }), &session, &mut ctx)
                .await
                .expect("another host is still reachable");
            runner
                .execute(&step(AgentAction::Back), &session, &mut ctx)
                .await
                .expect("a history step is the way off a wall and is never refused");
            let before = driver.calls();
            let click = runner
                .execute(&step(AgentAction::Click { selector: "#go".into() }), &session, &mut ctx)
                .await;
            let after = driver.calls();
            (before, after, click.map(|_| ()).map_err(|e| e.code))
        }

        // Landing on the exhausted host: refused, and the browser never saw the click.
        let (before, after, click) = run("https://blocked.example/page").await;
        assert_eq!(click, Err(ErrorCode::ChallengeDetected));
        assert_eq!(after, before, "a refused click never reaches the browser");

        // Landing on a host that is not exhausted: the click reaches the browser
        // (the test driver then reports the click as unsupported, which is not a refusal).
        let (before, after, click) = run("https://elsewhere.example/").await;
        assert_eq!(click, Err(ErrorCode::Unsupported));
        assert_eq!(after, before + 1, "a click on an open host reaches the browser");
    }

    #[tokio::test]
    async fn the_log_phase_reports_but_does_not_reject() {
        let events = EventLog::default();
        let _installed = tracing::subscriber::set_default(events.clone());
        let driver = Arc::new(ContentDriver { html: TURNSTILE_HTML, atomic_targets: true, ..Default::default() });
        let runner = challenge_runner(&driver);
        let lease = challenge_lease();
        let session = driver.acquire(&lease).await.unwrap();
        let go = challenge_request(&lease, AgentAction::Navigate { url: "https://blocked.example/page".into() }, ZdrMode::Off);
        let wait = challenge_request(&lease, AgentAction::Wait { ms: 0 }, ZdrMode::Off);

        // The same sequence under both settings: navigate, wait, navigate again.
        let mut log_ctx = challenge_ctx(ChallengeConfig::from_values(None, Some("log")));
        runner.execute(&go, &session, &mut log_ctx).await.unwrap();
        let second = runner.execute(&wait, &session, &mut log_ctx).await.unwrap();
        // The precondition: the loop is real.
        assert_eq!(
            second.challenge.expect("still challenged").block_reason,
            Some(quarry_core::contracts::BlockReason::ChallengeLoop)
        );
        assert_eq!(events.count("quarry.challenge.would_reject"), 0, "nothing was refused yet");
        let reached_before = driver.calls();
        runner
            .execute(&go, &session, &mut log_ctx)
            .await
            .expect("log mode lets the navigation through");
        assert_eq!(driver.calls(), reached_before + 1, "the navigation reached the browser");
        assert_eq!(events.count("quarry.challenge.would_reject"), 1, "{:?}", events.lines());
        assert!(
            events.lines().iter().all(|line| !line.contains("blocked.example")),
            "no event may carry the host: {:?}",
            events.lines()
        );

        // Enforce mode refuses the very same sequence, before the browser, and
        // logs no would_reject.
        let mut enforce_ctx = challenge_ctx(ChallengeConfig::enforcing());
        runner.execute(&go, &session, &mut enforce_ctx).await.unwrap();
        runner.execute(&wait, &session, &mut enforce_ctx).await.unwrap();
        let reached_before = driver.calls();
        let error = runner
            .execute(&go, &session, &mut enforce_ctx)
            .await
            .expect_err("enforce mode refuses");
        assert_eq!(error.code, ErrorCode::ChallengeDetected);
        assert_eq!(driver.calls(), reached_before, "a refused navigation never reaches the browser");
        assert_eq!(events.count("quarry.challenge.would_reject"), 1, "enforce mode logs no would_reject");
    }

    #[tokio::test]
    async fn execute_rejects_same_host_navigation_after_challenge_loop() {
        let driver = Arc::new(ContentDriver { html: TURNSTILE_HTML, atomic_targets: true, ..Default::default() });
        let runner = challenge_runner(&driver);
        let lease = challenge_lease();
        let session = driver.acquire(&lease).await.unwrap();
        let mut ctx = challenge_ctx(ChallengeConfig::enforcing());
        let go = |url: &str| challenge_request(&lease, AgentAction::Navigate { url: url.into() }, ZdrMode::Off);

        // First hit: a strong signal, and the one recovery is still there.
        let first = runner
            .execute(&go("https://blocked.example/page"), &session, &mut ctx)
            .await
            .expect("the first navigation is allowed");
        let signal = first.challenge.expect("the turnstile page is a challenge");
        assert_eq!(signal.strength, ChallengeStrength::Strong);
        assert_eq!(signal.recovery, quarry_core::contracts::ChallengeRecovery::Available);
        assert!(signal.requires_escalation);
        assert_eq!(first.telemetry.challenge_observation_count, 1);

        // The recovery: one wait. The page is still the challenge, so this is a loop.
        let second = runner
            .execute(&challenge_request(&lease, AgentAction::Wait { ms: 0 }, ZdrMode::Off), &session, &mut ctx)
            .await
            .expect("a wait is always allowed");
        let signal = second.challenge.expect("still challenged");
        assert_eq!(signal.block_reason, Some(quarry_core::contracts::BlockReason::ChallengeLoop));
        assert_eq!(signal.recovery, quarry_core::contracts::ChallengeRecovery::Exhausted);
        assert_eq!(second.telemetry.challenge_observation_count, 2);

        // Now the same host refuses navigation and interaction ...
        let reached_before = driver.calls();
        for action in refused_on_an_exhausted_page() {
            let error = runner
                .execute(&challenge_request(&lease, action, ZdrMode::Off), &session, &mut ctx)
                .await
                .expect_err("an exhausted host must refuse");
            assert_eq!(error.code, ErrorCode::ChallengeDetected);
            assert_eq!(error.code.http_status(), 403);
            let details = error.details.expect("details");
            assert_eq!(details["recovery"], "exhausted");
            assert_eq!(details["host"], "blocked.example");
            assert_eq!(details["next"], "user_takeover_or_other_source");
        }
        assert_eq!(
            driver.calls(),
            reached_before,
            "a refused action must not reach the browser"
        );

        // ... and still lets the planner look and report.
        runner
            .execute(&challenge_request(&lease, AgentAction::GetContent, ZdrMode::Off), &session, &mut ctx)
            .await
            .expect("reading the page is allowed");
        runner
            .execute(&challenge_request(&lease, AgentAction::Wait { ms: 0 }, ZdrMode::Off), &session, &mut ctx)
            .await
            .expect("waiting is allowed");

        // Another host is a fresh start ...
        let elsewhere = runner
            .execute(&go("https://other.example/"), &session, &mut ctx)
            .await
            .expect("another host is allowed");
        assert_eq!(
            elsewhere.challenge.expect("this fixture challenges everywhere").recovery,
            quarry_core::contracts::ChallengeRecovery::Available
        );
        // ... but the exhausted host stays closed from there.
        let error = runner
            .execute(&go("https://blocked.example/page"), &session, &mut ctx)
            .await
            .expect_err("back to the exhausted host");
        assert_eq!(error.code, ErrorCode::ChallengeDetected);
    }

    #[tokio::test]
    async fn the_legacy_classifier_is_not_subject_to_the_rule() {
        let driver = Arc::new(ContentDriver { html: LEGACY_HTML, ..Default::default() });
        let runner = challenge_runner(&driver);
        let lease = challenge_lease();
        let session = driver.acquire(&lease).await.unwrap();
        let mut ctx = challenge_ctx(ChallengeConfig::from_values(Some("legacy"), None));
        let go = challenge_request(&lease, AgentAction::Navigate { url: "https://blocked.example/page".into() }, ZdrMode::Off);
        for _ in 0..4 {
            let observation = runner.execute(&go, &session, &mut ctx).await.unwrap();
            let signal = observation.challenge.expect("legacy matches the title text");
            assert_eq!(signal.block_reason, None);
        }
    }

    #[tokio::test]
    async fn a_login_page_is_reported_without_escalation_and_is_not_counted() {
        let driver = Arc::new(ContentDriver { html: LOGIN_HTML, ..Default::default() });
        let runner = challenge_runner(&driver);
        let lease = challenge_lease();
        let session = driver.acquire(&lease).await.unwrap();
        let mut ctx = challenge_ctx(ChallengeConfig::enforcing());
        let request = challenge_request(&lease, AgentAction::Navigate { url: "https://example.com/login".into() }, ZdrMode::Off);
        let observation = runner.execute(&request, &session, &mut ctx).await.unwrap();
        let signal = observation.challenge.expect("a login page is named");
        assert_eq!(signal.kind, ChallengeKind::Login);
        assert_eq!(signal.strength, ChallengeStrength::NonSignal);
        assert!(!signal.requires_escalation);
        assert_eq!(observation.telemetry.challenge_observation_count, 0);
        assert_eq!(observation.telemetry.challenge_rate_per_mille, 0);
        // Repeated visits never exhaust anything: it is not a block.
        for _ in 0..4 {
            runner.execute(&request, &session, &mut ctx).await.expect("login stays reachable");
        }
    }

    #[tokio::test]
    async fn a_zdr_run_leaves_the_host_out_of_the_rejection() {
        let driver = Arc::new(ContentDriver { html: TURNSTILE_HTML, atomic_targets: true, ..Default::default() });
        let runner = challenge_runner(&driver);
        let lease = challenge_lease();
        let session = driver.acquire(&lease).await.unwrap();
        let mut ctx = challenge_ctx(ChallengeConfig::enforcing());
        let go = challenge_request(&lease, AgentAction::Navigate { url: "https://blocked.example/page".into() }, ZdrMode::On);
        runner.execute(&go, &session, &mut ctx).await.unwrap();
        runner
            .execute(&challenge_request(&lease, AgentAction::Wait { ms: 0 }, ZdrMode::On), &session, &mut ctx)
            .await
            .unwrap();
        let error = runner.execute(&go, &session, &mut ctx).await.expect_err("refused");
        assert_eq!(error.code, ErrorCode::ChallengeDetected);
        assert!(error.details.expect("details").get("host").is_none());
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
    fn dom_summary_filters_hidden_elements_and_keeps_rendered_ones() {
        let html = r#"<body>
            <button data-testid='visible'>Visible</button>
            <button hidden data-testid='attr-hidden'>Hidden attr</button>
            <button data-testid='bare-hidden' hidden>Bare hidden</button>
            <button aria-hidden='true' data-testid='aria-hidden'>Aria hidden</button>
            <button style="display:none" data-testid='style-hidden'>Style hidden</button>
            <button style="display : NONE ;" data-testid='style-case-hidden'>Case style</button>
            <button style="visibility:hidden" data-testid='vis-hidden'>Vis hidden</button>
            <button style="opacity:0" data-testid='opacity-hidden'>Opacity hidden</button>
            <input type='hidden' name='csrf' value='token'>
            <a href='/real' id='real-link'>Real link</a>
            <button title='the hidden gem' data-testid='false-positive'>Title mention</button>
        </body>"#;
        let summary = build_dom_summary(html);
        let test_ids: Vec<_> = summary
            .interactive_elements
            .iter()
            .map(|element| {
                element
                    .test_id
                    .as_deref()
                    .or(Some(element.selector.as_str()))
                    .unwrap_or("")
            })
            .collect();
        for hidden in [
            "attr-hidden",
            "bare-hidden",
            "aria-hidden",
            "style-hidden",
            "style-case-hidden",
            "vis-hidden",
            "opacity-hidden",
        ] {
            assert!(
                !test_ids.iter().any(|id| id.contains(hidden)),
                "hidden element leaked into summary: {hidden}"
            );
        }
        assert!(!summary
            .interactive_elements
            .iter()
            .any(|element| element.tag == "input"));
        // Exactly the three rendered controls survive: the visible button,
        // the real link, and the button whose *title text* merely mentions
        // "hidden" (quoted attribute values must never trigger the filter).
        assert_eq!(summary.interactive_elements.len(), 3);
    }

    #[test]
    fn click_map_is_stable_dense_and_aligned_with_snapshot_refs() {
        let summary = build_dom_summary(concat!(
            "<button data-testid='save'>Save</button>",
            "<a href='/next' id='next'>Next page</a>",
            "<textarea placeholder='Notes'></textarea>",
        ));
        let click_map = summary.click_map.as_ref().expect("non-empty page has a map");
        assert_eq!(click_map.len(), 3);
        for (offset, row) in click_map.iter().enumerate() {
            let expected = offset as u32 + 1;
            assert_eq!(row.index, expected);
            assert_eq!(row.ref_id, format!("@e{expected}"));
            assert_eq!(summary.interactive_elements[offset].click_index, Some(expected));
        }
        // Scanner order follows tag scan order (a, button, input, select,
        // textarea), so the anchor lands first.
        assert_eq!(click_map[0].tag, "a");
        assert_eq!(click_map[0].selector.as_deref(), Some("[id=\"next\"]"));
        assert_eq!(click_map[0].name.as_deref(), Some("Next page"));
        let save_row = click_map
            .iter()
            .find(|row| row.tag == "button")
            .expect("save button row");
        assert_eq!(save_row.selector.as_deref(), Some("[data-testid=\"save\"]"));
        assert_eq!(save_row.name.as_deref(), Some("Save"));

        // The indices are reusable by action planning: every snapshot target
        // minted from this summary carries the click index it came from and
        // names the matching @eN ref. The snapshot stays a strict subset of
        // the click map (the bare textarea has no unique selector to bind an
        // executable action to), so subset membership — not equality — is
        // the alignment contract.
        let run_id: RunKind = Id::new();
        let snapshot = build_active_snapshot(&run_id, 1, "blake3:page", Some(&summary), None);
        assert_eq!(snapshot.snapshot.targets.len(), 2);
        for target in &snapshot.snapshot.targets {
            let index = target.click_index.expect("scanner targets carry an index");
            assert_eq!(target.ref_id, format!("@e{index}"));
            assert_eq!(click_map[index as usize - 1].ref_id, target.ref_id);
            assert_eq!(click_map[index as usize - 1].tag, target.tag);
        }
    }

    #[test]
    fn empty_page_yields_no_click_map() {
        let summary = build_dom_summary("<html><body><p>Plain text only</p></body></html>");
        assert!(summary.interactive_elements.is_empty());
        assert!(summary.click_map.is_none());
    }

    #[test]
    fn dom_summary_wire_shapes_stay_backward_compatible() {
        // Legacy payloads without the click-map fields still decode.
        let legacy = serde_json::json!({
            "node_count": 1,
            "interactive_elements": [
                { "tag": "button", "selector": "[data-testid=\"go\"]" }
            ]
        });
        let summary: DomSummary = serde_json::from_value(legacy).unwrap();
        assert!(summary.click_map.is_none());
        assert_eq!(summary.interactive_elements[0].click_index, None);

        // Absent optional fields stay absent on re-encode (no null noise).
        let encoded = serde_json::to_value(&summary).unwrap();
        assert!(encoded.get("click_map").is_none());
        assert!(encoded["interactive_elements"][0]
            .get("click_index")
            .is_none());

        // Populated maps round-trip losslessly.
        let full = build_dom_summary("<button data-testid='go'>Go</button>");
        let json = serde_json::to_string(&full).unwrap();
        let back: DomSummary = serde_json::from_str(&json).unwrap();
        assert_eq!(back.click_map, full.click_map);
        assert_eq!(back.interactive_elements, full.interactive_elements);
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

    /// Serves fixed HTML. `calls` counts every call that would move or act on
    /// the page (goto, click, click_point, type, press, select), so a test can
    /// show that a refused action never reached the browser.
    #[derive(Default)]
    struct ContentDriver {
        html: &'static [u8],
        calls: std::sync::atomic::AtomicUsize,
        /// Claims atomic snapshot-target actions, so that ref and semantic
        /// actions get past the capability gate and reach what a test is about.
        atomic_targets: bool,
        /// When set, `back` succeeds and the page then reports this URL.
        back_lands_on: Option<&'static str>,
        landed: std::sync::atomic::AtomicBool,
        /// WS12.S5: what `screenshot_masked` reports; unset means an unscanned image.
        masked_screenshot: Option<quarry_browser::MaskedScreenshot>,
        /// Claims governed full-fidelity visual evidence, so an explicit screenshot
        /// action gets past the capability gate and reaches the withheld-image path.
        visual_fidelity: bool,
        /// D2-019: every egress policy the runner installed, in order.
        egress_seen: std::sync::Mutex<Vec<BrowserEgressPolicy>>,
        /// SC4 T3: the page read (`evaluate` or `content`) that times out.
        stalls_at: Option<&'static str>,
        /// SC4 T3: every `content` and `native_page_projection` call.
        reads: std::sync::atomic::AtomicUsize,
        /// The egress receipts the driver holds, by sequence; none means the driver
        /// has no receipt authority.
        egress_sequences: &'static [u64],
        /// SC4 D1: every `ensure_page` call.
        ensured: std::sync::atomic::AtomicUsize,
    }

    /// The typed timeout the chromiumoxide driver returns for `op`.
    fn driver_timeout(op: &str, effect: &str) -> QuarryError {
        let mut error = QuarryError::new(
            ErrorCode::Timeout,
            format!("browser operation {op} timed out"),
        )
        .with_details(serde_json::json!({ "op": op, "deadline_ms": 7500, "effect": effect }));
        error.retryable = effect == "none";
        error
    }

    impl ContentDriver {
        fn calls(&self) -> usize {
            self.calls.load(std::sync::atomic::Ordering::SeqCst)
        }

        fn touch(&self) {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }

    #[async_trait::async_trait]
    impl BrowserDriver for ContentDriver {
        async fn configure_egress_policy(
            &self,
            _session: &quarry_browser::BrowserSession,
            policy: BrowserEgressPolicy,
        ) -> QuarryResult<()> {
            self.egress_seen.lock().unwrap().push(policy);
            Ok(())
        }
        fn capabilities(&self) -> quarry_browser::BrowserDriverCapabilities {
            quarry_browser::BrowserDriverCapabilities {
                atomic_target_actions: self.atomic_targets,
                full_visual_fidelity: self.visual_fidelity,
                ..Default::default()
            }
        }
        async fn acquire(
            &self,
            lease: &BrowserLease,
        ) -> QuarryResult<quarry_browser::BrowserSession> {
            Ok(quarry_browser::BrowserSession {
                lease: lease.clone(),
                tab_pin: None,
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
            self.touch();
            Ok(())
        }
        async fn ensure_page(&self, _session: &quarry_browser::BrowserSession) -> QuarryResult<()> {
            self.ensured
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }
        async fn click(
            &self,
            _session: &quarry_browser::BrowserSession,
            _selector: &str,
        ) -> QuarryResult<()> {
            self.touch();
            Err(QuarryError::unsupported_action("click"))
        }
        async fn click_point(
            &self,
            _session: &quarry_browser::BrowserSession,
            _x: f64,
            _y: f64,
        ) -> QuarryResult<()> {
            self.touch();
            Err(QuarryError::unsupported_action("click_point"))
        }
        async fn type_text(
            &self,
            _session: &quarry_browser::BrowserSession,
            _selector: &str,
            _text: &str,
        ) -> QuarryResult<()> {
            self.touch();
            Err(QuarryError::unsupported_action("type"))
        }
        async fn press(
            &self,
            _session: &quarry_browser::BrowserSession,
            _key: &str,
        ) -> QuarryResult<()> {
            self.touch();
            Err(QuarryError::unsupported_action("press"))
        }
        async fn select(
            &self,
            _session: &quarry_browser::BrowserSession,
            _selector: &str,
            _value: &str,
        ) -> QuarryResult<()> {
            self.touch();
            Err(QuarryError::unsupported_action("select"))
        }
        async fn egress_receipts(
            &self,
            _session: &quarry_browser::BrowserSession,
            after_sequence: u64,
            _limit: usize,
        ) -> QuarryResult<Vec<quarry_core::contracts::BrowserEgressReceipt>> {
            if self.egress_sequences.is_empty() {
                return Err(QuarryError::unsupported_action("egress_receipts"));
            }
            Ok(self
                .egress_sequences
                .iter()
                .filter(|sequence| **sequence > after_sequence)
                .map(|sequence| quarry_core::contracts::BrowserEgressReceipt {
                    sequence: *sequence,
                    tab_id: None,
                    method: "GET".into(),
                    url: "https://example.com/".into(),
                    decision: quarry_core::contracts::BrowserEgressDecision::Allow,
                    policy: "url_dns_public".into(),
                    timestamp_ms: 0,
                })
                .collect())
        }
        async fn content(&self, _session: &quarry_browser::BrowserSession) -> QuarryResult<Bytes> {
            self.reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if self.stalls_at == Some("content") {
                return Err(driver_timeout("content", "none"));
            }
            Ok(Bytes::from_static(self.html))
        }
        async fn native_page_projection(
            &self,
            _session: &quarry_browser::BrowserSession,
        ) -> QuarryResult<quarry_browser::BrowserNativeProjection> {
            self.reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(QuarryError::unsupported_action("native_page_projection"))
        }
        async fn screenshot(
            &self,
            _session: &quarry_browser::BrowserSession,
            _full_page: bool,
        ) -> QuarryResult<Bytes> {
            Ok(Bytes::new())
        }
        async fn screenshot_masked(
            &self,
            _session: &quarry_browser::BrowserSession,
            _full_page: bool,
        ) -> QuarryResult<quarry_browser::MaskedScreenshot> {
            Ok(match &self.masked_screenshot {
                Some(masked) => masked.clone(),
                None => quarry_browser::MaskedScreenshot::unmasked(Bytes::new()),
            })
        }
        async fn pdf(&self, _session: &quarry_browser::BrowserSession) -> QuarryResult<Bytes> {
            Ok(Bytes::new())
        }
        async fn back(&self, _session: &quarry_browser::BrowserSession) -> QuarryResult<()> {
            match self.back_lands_on {
                Some(_) => {
                    self.landed.store(true, std::sync::atomic::Ordering::SeqCst);
                    Ok(())
                }
                None => Err(QuarryError::unsupported_action("back")),
            }
        }
        async fn evaluate(
            &self,
            _session: &quarry_browser::BrowserSession,
            _script: &str,
        ) -> QuarryResult<serde_json::Value> {
            if self.stalls_at == Some("evaluate") {
                return Err(driver_timeout("evaluate", "unknown"));
            }
            match self.back_lands_on {
                Some(url) if self.landed.load(std::sync::atomic::Ordering::SeqCst) => {
                    Ok(serde_json::json!({ "url": url, "title": null }))
                }
                _ => Err(QuarryError::unsupported_action("evaluate")),
            }
        }
    }

    async fn observe_a_wait(driver: Arc<ContentDriver>) -> QuarryResult<BrowserObservation> {
        observe(
            driver,
            AgentAction::Wait { ms: 0 },
            &mut ObservationContext::default(),
        )
        .await
    }

    async fn observe(
        driver: Arc<ContentDriver>,
        action: AgentAction,
        ctx: &mut ObservationContext,
    ) -> QuarryResult<BrowserObservation> {
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
        let request = AgentActionRequest {
            run_id: Id::new(),
            lease_id: lease.lease_id.clone(),
            action,
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
        runner.execute(&request, &session, ctx).await
    }

    /// SC4 T3: the page state read timed out, so the content and accessibility reads are
    /// not made (each would wait out its own deadline on the same held page), and the
    /// step fails with that first timeout. The step only waited, so nothing changed and
    /// it may be retried, whatever a generic `evaluate` has to assume.
    #[tokio::test]
    async fn a_page_state_timeout_ends_the_page_reads_with_a_read_timeout() {
        let driver = Arc::new(ContentDriver {
            html: b"<html><body><p>hello</p></body></html>",
            stalls_at: Some("evaluate"),
            ..Default::default()
        });
        let error = observe_a_wait(driver.clone()).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::Timeout);
        assert!(error.retryable, "the page state script only reads");
        let details = error.details.expect("details");
        assert_eq!(details["op"], "evaluate");
        assert_eq!(details["effect"], "none");
        assert_eq!(
            driver.reads.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "no read after the first timeout"
        );
    }

    /// SC4 T3: a content timeout fails the step at once; the accessibility read is not made.
    #[tokio::test]
    async fn a_content_timeout_ends_the_page_reads() {
        let driver = Arc::new(ContentDriver {
            html: b"<html><body><p>hello</p></body></html>",
            stalls_at: Some("content"),
            ..Default::default()
        });
        let error = observe_a_wait(driver.clone()).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::Timeout);
        assert_eq!(error.details.expect("details")["op"], "content");
        assert_eq!(
            driver.reads.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "only the content read was made"
        );
    }

    /// SC4 D1: an action that only reads the page asks the driver for a page first, so a
    /// session with none yet is observed on a blank tab; an action that moves or acts on
    /// the page does not (a navigation opens its own tab, a click needs a real page).
    #[tokio::test]
    async fn only_an_observing_action_asks_the_driver_for_a_page() {
        let ensured =
            |driver: &ContentDriver| driver.ensured.load(std::sync::atomic::Ordering::SeqCst);
        for action in [AgentAction::Wait { ms: 0 }, AgentAction::GetContent] {
            let driver = Arc::new(ContentDriver {
                html: b"<html><body><p>hello</p></body></html>",
                ..Default::default()
            });
            observe(
                driver.clone(),
                action.clone(),
                &mut ObservationContext::default(),
            )
            .await
            .unwrap_or_else(|error| panic!("{action:?}: {error:?}"));
            assert_eq!(ensured(&driver), 1, "{action:?}");
        }
        for action in [
            AgentAction::Navigate {
                url: "https://example.com/".into(),
            },
            AgentAction::Click {
                selector: "#go".into(),
            },
        ] {
            let driver = Arc::new(ContentDriver {
                html: b"<html><body><p>hello</p></body></html>",
                ..Default::default()
            });
            let _ = observe(
                driver.clone(),
                action.clone(),
                &mut ObservationContext::default(),
            )
            .await;
            assert_eq!(ensured(&driver), 0, "{action:?}");
        }
    }

    /// SC4 T3: the same stalled read after an action that can change the page (here a
    /// navigation that went through) is no read timeout: the step's effect is unknown
    /// and it must not be retried blindly.
    #[tokio::test]
    async fn a_stalled_read_after_an_effectful_action_keeps_its_effect_unknown() {
        for stalls_at in ["evaluate", "content"] {
            let driver = Arc::new(ContentDriver {
                html: b"<html><body><p>hello</p></body></html>",
                stalls_at: Some(stalls_at),
                ..Default::default()
            });
            let navigate = AgentAction::Navigate {
                url: "https://example.com/".into(),
            };
            let error = observe(driver, navigate, &mut ObservationContext::default())
                .await
                .unwrap_err();
            assert_eq!(error.code, ErrorCode::Timeout, "{stalls_at}");
            assert!(!error.retryable, "{stalls_at}");
            assert_eq!(
                error.details.expect("details")["effect"],
                "unknown",
                "{stalls_at}"
            );
        }
    }

    /// A step whose observation fails leaves the egress cursor where it was: the
    /// receipts it read go out with the next observation instead of being skipped.
    #[tokio::test]
    async fn a_failed_observation_leaves_its_egress_receipts_for_the_next_one() {
        let mut ctx = ObservationContext::default();
        let stalled = Arc::new(ContentDriver {
            html: b"<html><body><p>hello</p></body></html>",
            stalls_at: Some("content"),
            egress_sequences: &[1, 2],
            ..Default::default()
        });
        observe(stalled, AgentAction::Wait { ms: 0 }, &mut ctx)
            .await
            .unwrap_err();
        assert_eq!(ctx.last_egress_sequence, 0);

        let answering = Arc::new(ContentDriver {
            html: b"<html><body><p>hello</p></body></html>",
            egress_sequences: &[1, 2],
            ..Default::default()
        });
        observe(answering, AgentAction::Wait { ms: 0 }, &mut ctx)
            .await
            .expect("observation");
        assert_eq!(ctx.last_egress_sequence, 2);
    }

    /// A page that answers keeps the old path: every read is made and an optional
    /// read that fails for another reason (here `evaluate` is unsupported) is dropped.
    #[tokio::test]
    async fn a_page_that_answers_is_read_in_full() {
        let driver = Arc::new(ContentDriver {
            html: b"<html><head><title>hi</title></head><body><p>hello</p></body></html>",
            ..Default::default()
        });
        let observation = observe_a_wait(driver.clone()).await.expect("observation");
        assert_eq!(
            driver.reads.load(std::sync::atomic::Ordering::SeqCst),
            2,
            "content and the accessibility tree"
        );
        assert!(observation.snapshot.is_some());
    }

    /// D2-019: a run that carries its resolved egress keeps it on every action. Before the
    /// fix, step 2 rebuilt the policy from `constraints.allowed_domains` and lost the
    /// narrowing and the open-web mode that `start_run` had installed.
    #[tokio::test]
    async fn the_run_carried_egress_is_installed_on_every_action_not_rebuilt_from_constraints() {
        let driver = Arc::new(ContentDriver {
            html: b"<html><body><p>hello</p></body></html>",
            ..Default::default()
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
        let request = AgentActionRequest {
            run_id: Id::new(),
            lease_id: lease.lease_id.clone(),
            action: AgentAction::Evaluate {
                script: "document.title = 'x'".into(),
            },
            instruction: None,
            constraints: AgentConstraints {
                max_steps: 3,
                // The grant's own list is wider than what the web policy narrowed it to.
                allowed_domains: vec!["example.com".into(), "other.test".into()],
                max_runtime_s: None,
                max_cost_usd: None,
            },
            zdr: ZdrMode::Off,
            extraction_profile: None,
        };

        // A narrowed allow list: only a.example.com stays reachable.
        let narrowed = BrowserEgressPolicy::from_allowed_domains(&["a.example.com".to_string()]);
        let mut ctx = ObservationContext {
            egress_policy: Some(narrowed.clone()),
            ..Default::default()
        };
        for _ in 0..2 {
            let _ = runner.execute(&request, &session, &mut ctx).await;
        }
        {
            let seen = driver.egress_seen.lock().unwrap();
            assert_eq!(seen.len(), 2, "one install per action");
            assert_eq!(seen[0], narrowed, "step 1 installs the resolved policy");
            assert_eq!(seen[1], narrowed, "step 2 installs the same resolved policy");
            assert!(!seen[1].allows_url("https://other.test/x"), "narrowing survives step 2");
        }

        // The open-web mode and its denied list survive too.
        driver.egress_seen.lock().unwrap().clear();
        let open = BrowserEgressPolicy::open_web(&["blocked.test".to_string()]);
        let mut open_ctx = ObservationContext {
            egress_policy: Some(open.clone()),
            ..Default::default()
        };
        for _ in 0..2 {
            let _ = runner.execute(&request, &session, &mut open_ctx).await;
        }
        let seen = driver.egress_seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert!(seen.iter().all(|policy| *policy == open));
        assert!(seen[1].allows_url("https://anywhere.test/x"));
        assert!(!seen[1].allows_url("https://blocked.test/x"), "denied list survives step 2");
    }

    /// Without a carried policy the old behaviour stands: the policy is built from the
    /// request's allowed domains and the context's deny list.
    #[tokio::test]
    async fn a_context_without_a_carried_egress_builds_it_from_the_constraints() {
        let driver = Arc::new(ContentDriver {
            html: b"<html><body><p>hello</p></body></html>",
            ..Default::default()
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
        let request = AgentActionRequest {
            run_id: Id::new(),
            lease_id: lease.lease_id.clone(),
            action: AgentAction::Evaluate {
                script: "document.title = 'x'".into(),
            },
            instruction: None,
            constraints: AgentConstraints {
                max_steps: 3,
                allowed_domains: vec!["example.com".into()],
                max_runtime_s: None,
                max_cost_usd: None,
            },
            zdr: ZdrMode::Off,
            extraction_profile: None,
        };
        let mut ctx = ObservationContext::default();
        let _ = runner.execute(&request, &session, &mut ctx).await;
        let seen = driver.egress_seen.lock().unwrap();
        assert_eq!(
            seen[0],
            BrowserEgressPolicy::from_allowed_domains(&["example.com".to_string()])
        );
    }

    #[tokio::test]
    async fn evaluate_action_is_refused_without_calling_the_driver() {
        // ContentDriver does not implement `evaluate`, so a driver call would
        // surface as `Unsupported`. `Forbidden` proves the refusal came first.
        let driver = Arc::new(ContentDriver {
            html: b"<html><body><p>hello</p></body></html>",
            ..Default::default()
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
        let mut ctx = ObservationContext {
            step: 1,
            current_url: "https://example.test".into(),
            page_hash: "blake3:page".into(),
            previous_screenshot: None,
            previous_observation: None,
            previous_network_keys: vec![],
            last_egress_sequence: 0,
            active_snapshot: None,
            observed_action_count: 0,
            challenge_observation_count: 0,
            ..Default::default()
        };
        let request = AgentActionRequest {
            run_id: Id::new(),
            lease_id: lease.lease_id.clone(),
            action: AgentAction::Evaluate {
                script: "document.title = 'x'".into(),
            },
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
        let error = runner
            .execute(&request, &session, &mut ctx)
            .await
            .expect_err("evaluate must be refused");
        assert_eq!(error.code, ErrorCode::Forbidden);
        assert_eq!(error.details.unwrap()["reason"], "raw_script_denied");
    }

    /// D2-065: the keyboard focus the run tracks decides how a key press is masked.
    #[test]
    fn the_key_focus_follows_clicks_and_typing_and_fails_closed() {
        let html = "<html><body><input id='pw' type='password' placeholder='Password'>\
                    <button id='go'>Go</button></body></html>";
        let run_id: RunKind = Id::new();
        let summary = build_dom_summary(html);
        let active = build_active_snapshot(&run_id, 1, "blake3:page", Some(&summary), None);
        let snapshot_id = active.snapshot.snapshot_id.clone();
        let ref_of = |needle: &str| {
            active
                .snapshot
                .targets
                .iter()
                .find(|t| t.tag == needle)
                .map(|t| t.ref_id.clone())
                .unwrap_or_else(|| panic!("no target {needle}"))
        };
        let (password, button) = (ref_of("input"), ref_of("button"));
        let click = |ref_id: &str| AgentAction::ClickRef {
            snapshot_id: snapshot_id.clone(),
            generation: 1,
            ref_id: ref_id.to_owned(),
        };
        let press = |key: &str| AgentAction::Press { key: key.to_owned() };
        let mut ctx = ObservationContext {
            active_snapshot: Some(active.clone()),
            ..Default::default()
        };
        // Start state: no known focus, so a character key is masked as a secret.
        assert_eq!(receipt_field_kind(&ctx, &press("a")), None);

        // A click on the password control puts the focus there.
        let typed = receipt_field_kind(&ctx, &click(&password));
        assert_eq!(typed, None, "a click is not a typing action");
        ctx.key_focus = key_focus_after(&ctx, &click(&password), typed);
        assert_eq!(ctx.key_focus, Some(FieldKind::Password));
        assert_eq!(receipt_field_kind(&ctx, &press("s")), Some(FieldKind::Password));

        // Characters, Backspace, a shortcut and a wait leave it where it was.
        for action in [
            press("s"),
            press("Backspace"),
            press("Control+a"),
            AgentAction::Wait { ms: 10 },
            AgentAction::Scroll { target: "#x".to_owned() },
        ] {
            ctx.key_focus = key_focus_after(&ctx, &action, None);
            assert_eq!(ctx.key_focus, Some(FieldKind::Password), "{action:?}");
        }

        // Tab, Enter, a click on a button, a navigation and a snapshot-less click: unknown.
        for action in [
            press("Tab"),
            press("Enter"),
            press("Shift+Tab"),
            press("F5"),
            click(&button),
            AgentAction::Navigate { url: "https://example.test/".to_owned() },
            AgentAction::Back,
            AgentAction::ClickPoint { x: 1.0, y: 2.0 },
        ] {
            let mut moved = ObservationContext {
                active_snapshot: Some(active.clone()),
                key_focus: Some(FieldKind::Password),
                ..Default::default()
            };
            moved.key_focus = key_focus_after(&moved, &action, None);
            assert_eq!(moved.key_focus, None, "{action:?}");
        }

        // Typing into a control sets the focus from the snapshot, and a raw selector only
        // counts when its own words name a sensitive control.
        let type_ref = AgentAction::TypeRef {
            snapshot_id: snapshot_id.clone(),
            generation: 1,
            ref_id: password,
            text: "x".to_owned(),
        };
        let typed = receipt_field_kind(&ctx, &type_ref);
        assert_eq!(typed, Some(FieldKind::Password));
        assert_eq!(key_focus_after(&ctx, &type_ref, typed), Some(FieldKind::Password));
        let raw = |selector: &str| AgentAction::Type {
            selector: selector.to_owned(),
            text: "x".to_owned(),
        };
        assert_eq!(key_focus_after(&ctx, &raw("input[type=password]"), None), Some(FieldKind::Password));
        assert_eq!(key_focus_after(&ctx, &raw("#q"), None), None);
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
            ..Default::default()
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
            previous_screenshot: None,
            previous_observation: None,
            previous_network_keys: vec![],
            last_egress_sequence: 0,
            observed_action_count: 0,
            challenge_observation_count: 0,
            active_snapshot: Some(active_snapshot),
            ..Default::default()
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
    fn text_snippet_cuts_on_a_character_boundary() {
        // A three-byte character covering bytes 498..501 used to panic at [..500].
        let text = format!("{}文{}", "a".repeat(498), "b".repeat(50));
        let snippet = text_snippet_of(&text);
        assert_eq!(snippet, format!("{}...", "a".repeat(498)));
        assert_eq!(text_snippet_of("short"), "short");
        let exact = "x".repeat(500);
        assert_eq!(text_snippet_of(&exact), exact);
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

    #[test]
    fn reload_action_converts_to_browser_reload() {
        let browser_action = ObservationRunner::agent_action_to_browser_action(&AgentAction::Reload {
            ignore_cache: true,
        })
        .unwrap();
        assert!(matches!(browser_action, Action::Reload { ignore_cache: true }));
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
            previous_screenshot: Some(b"previous".to_vec()),
            previous_observation: None,
            previous_network_keys: vec![],
            last_egress_sequence: 0,
            active_snapshot: None,
            observed_action_count: 0,
            challenge_observation_count: 0,
            ..Default::default()
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

    fn mask_test_lease() -> BrowserLease {
        BrowserLease {
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
        }
    }

    fn mask_test_request(lease: &BrowserLease, action: AgentAction) -> AgentActionRequest {
        AgentActionRequest {
            run_id: Id::new(),
            lease_id: lease.lease_id.clone(),
            action,
            instruction: None,
            constraints: AgentConstraints {
                max_steps: 3,
                allowed_domains: vec![],
                max_runtime_s: None,
                max_cost_usd: None,
            },
            zdr: ZdrMode::Off,
            extraction_profile: None,
        }
    }

    fn mask_test_ctx() -> ObservationContext {
        ObservationContext {
            step: 1,
            current_url: "https://example.com".into(),
            page_hash: "blake3:page".into(),
            previous_screenshot: Some(b"previous".to_vec()),
            previous_observation: None,
            previous_network_keys: vec![],
            last_egress_sequence: 0,
            active_snapshot: None,
            observed_action_count: 0,
            challenge_observation_count: 0,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn withheld_screenshot_writes_no_artifact_and_skips_the_sidecar() {
        let store = Arc::new(InMemoryStore::new());
        let browser = Arc::new(ContentDriver {
            html: b"<html><body><p>hello</p></body></html>",
            masked_screenshot: Some(quarry_browser::MaskedScreenshot::withheld(
                quarry_browser::screenshot_mask::Withheld::SensitiveFieldUnresolved,
                1,
                7,
            )),
            ..Default::default()
        });
        let runner = ObservationRunner {
            browser: browser.clone(),
            artifacts: Some(store.clone()),
            events: None,
            visual_processor: Some(Arc::new(MockVisualProcessor)),
            org_id: "org_a".into(),
        };
        let lease = mask_test_lease();
        let session = browser.acquire(&lease).await.unwrap();
        let request = mask_test_request(&lease, AgentAction::Wait { ms: 0 });
        let mut ctx = mask_test_ctx();

        let observation = runner.execute(&request, &session, &mut ctx).await.unwrap();

        assert!(observation.screenshot_artifact_id.is_none());
        assert!(observation.visual_observation_artifact_id.is_none());
        let stamp = observation.redaction.expect("a redaction stamp");
        assert_eq!(stamp.screenshot, quarry_core::redaction::ScreenshotState::Withheld);
        assert_eq!((stamp.rects, stamp.oopif_frames, stamp.mask_ms), (0, 1, 7));
        assert!(
            ctx.previous_screenshot.is_none(),
            "a withheld frame leaves no stale previous frame"
        );
        let listed = store
            .list("org_a", &quarry_core::pagination::ListFilter::default())
            .await
            .unwrap();
        for item in &listed.items {
            assert!(
                !item.kind.as_str().starts_with("screenshot")
                    && item.kind.as_str() != "visual_observation"
                    && item.kind.as_str() != "page_image_clean",
                "a withheld screenshot left an artifact of kind {}",
                item.kind.as_str()
            );
        }
    }

    #[tokio::test]
    async fn a_masked_screenshot_is_stored_and_stamped() {
        let store = Arc::new(InMemoryStore::new());
        let mut masked = quarry_browser::MaskedScreenshot::unmasked(Bytes::from_static(b"png"));
        masked.state = quarry_core::redaction::ScreenshotState::Masked;
        masked.rects = vec![
            quarry_browser::screenshot_mask::PixelRect { x: 1, y: 2, w: 3, h: 4 },
            quarry_browser::screenshot_mask::PixelRect { x: 5, y: 6, w: 7, h: 8 },
        ];
        masked.mask_ms = 9;
        let browser = Arc::new(ContentDriver {
            html: b"<html><body><p>hello</p></body></html>",
            masked_screenshot: Some(masked),
            ..Default::default()
        });
        let runner = ObservationRunner {
            browser: browser.clone(),
            artifacts: Some(store.clone()),
            events: None,
            visual_processor: None,
            org_id: "org_a".into(),
        };
        let lease = mask_test_lease();
        let session = browser.acquire(&lease).await.unwrap();
        let request = mask_test_request(&lease, AgentAction::Wait { ms: 0 });
        let mut ctx = mask_test_ctx();

        let observation = runner.execute(&request, &session, &mut ctx).await.unwrap();

        assert!(observation.screenshot_artifact_id.is_some());
        let stamp = observation.redaction.expect("a redaction stamp");
        assert_eq!(stamp.screenshot, quarry_core::redaction::ScreenshotState::Masked);
        assert_eq!((stamp.rects, stamp.mask_ms), (2, 9));
    }

    #[tokio::test]
    async fn an_explicit_screenshot_action_is_refused_when_the_image_is_withheld() {
        let browser = Arc::new(ContentDriver {
            html: b"<html><body><p>hello</p></body></html>",
            masked_screenshot: Some(quarry_browser::MaskedScreenshot::withheld(
                quarry_browser::screenshot_mask::Withheld::SensitiveFieldUnresolved,
                0,
                1,
            )),
            visual_fidelity: true,
            ..Default::default()
        });
        let runner = ObservationRunner {
            browser: browser.clone(),
            artifacts: None,
            events: None,
            visual_processor: None,
            org_id: "org_a".into(),
        };
        let lease = mask_test_lease();
        let session = browser.acquire(&lease).await.unwrap();
        let request = mask_test_request(&lease, AgentAction::Screenshot { full_page: false });
        let mut ctx = mask_test_ctx();

        let error = runner
            .execute(&request, &session, &mut ctx)
            .await
            .expect_err("a withheld screenshot is an error");

        assert_eq!(error.code, ErrorCode::SecurityBlocked);
        let reason = error.details.as_ref().and_then(|d| d["reason"].as_str());
        assert_eq!(reason, Some("screenshot_withheld:sensitive_field_unresolved"));
    }
}
