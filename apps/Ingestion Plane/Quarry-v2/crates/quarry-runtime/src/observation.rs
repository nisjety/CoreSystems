//! Observation protocol — executes an AgentActionRequest, produces a BrowserObservation.

use std::{collections::HashMap, sync::Arc};

use chrono::Utc;
use quarry_browser::actions::{Action, ScrollTarget};
use quarry_browser::{BrowserDevtoolsEvent, BrowserDriver};
use quarry_core::contracts::{
    ActionOutcome, AgentAction, AgentActionRequest, BrowserObservation, ChallengeKind,
    ChallengeSignal, ConsoleLine, DomSummary, ElementFingerprint, ExtractionFieldResult,
    ExtractionProfile, ExtractionResult, ExtractionSource, InteractiveElement, NetworkEntry,
    ObservationDelta, ProofBundle,
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
}

impl ObservationRunner {
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
            AgentAction::WaitFor { .. } => ActionOutcome::verified("selector_observed"),
            AgentAction::Wait { .. } => ActionOutcome::verified("wait_completed"),
            AgentAction::Click { .. }
            | AgentAction::ClickPoint { .. }
            | AgentAction::Type { .. }
            | AgentAction::Press { .. }
            | AgentAction::Scroll { .. }
            | AgentAction::MouseWheel { .. }
            | AgentAction::Select { .. }
            | AgentAction::Evaluate { .. }
            | AgentAction::Back
            | AgentAction::Forward => ActionOutcome::unknown(
                "postcondition_required",
                "the browser operation completed, but its business effect was not verified",
            ),
        }
    }

    pub fn agent_action_to_browser_action(action: &AgentAction) -> Action {
        match action {
            AgentAction::Navigate { url } => Action::Navigate { url: url.clone() },
            AgentAction::Click { selector } => Action::Click {
                selector: selector.clone(),
            },
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
        }
    }

    pub async fn execute(
        &self,
        request: &AgentActionRequest,
        session: &quarry_browser::BrowserSession,
        ctx: &mut ObservationContext,
    ) -> QuarryResult<BrowserObservation> {
        let run_id = &request.run_id;

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
        let policy_denials: Vec<String> = Vec::new();
        let mut current_screenshot: Option<Vec<u8>> = None;

        match &request.action {
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
        let action_outcome = Self::outcome_for_action(&request.action, &ctx.current_url);
        let challenge = Self::classify_challenge(
            &ctx.current_url,
            title.as_deref(),
            dom_summary
                .as_ref()
                .and_then(|summary| summary.text_snippet.as_deref()),
        );
        let mut artifact_ids = Vec::new();
        if let Some(id) = screenshot_artifact_id.clone() {
            artifact_ids.push(id);
        }
        if let Some(id) = visual_observation_artifact_id.clone() {
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
            observed_at: Utc::now(),
        });
        let observation = BrowserObservation {
            run_id: run_id.clone(),
            step: ctx.step,
            url: ctx.current_url.clone(),
            title,
            dom_summary,
            screenshot_artifact_id,
            visual_observation_artifact_id,
            console_summary: console_summary_from_devtools(&devtools_events),
            network_summary: network_summary_from_devtools(&devtools_events),
            policy_denials,
            action_outcome,
            observation_delta: Some(delta),
            challenge,
            extraction_profile,
            extraction_result,
            proof_bundle,
            observed_at: Utc::now(),
        };

        ctx.previous_url = Some(observation.url.clone());
        ctx.previous_title = observation.title.clone();
        ctx.previous_dom_node_count = observation
            .dom_summary
            .as_ref()
            .map(|summary| summary.node_count);
        ctx.previous_page_hash = (!ctx.page_hash.is_empty()).then(|| ctx.page_hash.clone());

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
                    }),
                    format!("{}:observation:{}:ready", run_id, ctx.step),
                )
                .await;
        }

        ctx.step += 1;
        Ok(observation)
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
        let pattern = format!("<{}", tag);
        for (idx, _) in html.match_indices(&pattern) {
            let remaining = &html[idx..];
            let end = remaining.find('>').unwrap_or(remaining.len());
            let element_str = &remaining[..end];

            let text = extract_inner_text(remaining);
            let role = extract_attr(element_str, "role").map(|s| s.to_string());
            let aria_label = extract_attr(element_str, "aria-label").map(|s| s.to_string());
            let test_id = extract_attr(element_str, "data-testid")
                .or_else(|| extract_attr(element_str, "data-test"));
            let id = extract_attr(element_str, "id");
            let name = extract_attr(element_str, "name");
            let mut selectors = Vec::new();
            if let Some(id) = id {
                selectors.push(format!("#{id}"));
            }
            if let Some(test_id) = test_id {
                selectors.push(format!(r#"[data-testid="{test_id}"]"#));
            }
            if let Some(name) = name {
                selectors.push(format!("{tag}[name=\"{name}\"]"));
            }
            if let Some(label) = aria_label.as_deref() {
                selectors.push(format!(r#"{tag}[aria-label="{label}"]"#));
            }
            if let Some(role) = role.as_deref() {
                selectors.push(format!(r#"{tag}[role="{role}"]"#));
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
                text,
                role,
                aria_label,
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
        let html = "<html><body><a href='#' id='link1' aria-label='Open'>Click</a><button data-testid='go'>Go</button></body></html>";
        let summary = build_dom_summary(html);
        assert!(summary.node_count > 0);
        assert!(!summary.interactive_elements.is_empty());
        let link = summary
            .interactive_elements
            .iter()
            .find(|e| e.tag == "a")
            .unwrap();
        assert_eq!(link.selector, "#link1");
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
        let browser_action = ObservationRunner::agent_action_to_browser_action(&action);
        match browser_action {
            Action::Click { selector } => assert_eq!(selector, "#btn"),
            _ => panic!("wrong action type"),
        }
    }

    #[test]
    fn forward_action_converts_to_browser_action() {
        let browser_action =
            ObservationRunner::agent_action_to_browser_action(&AgentAction::Forward);
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
