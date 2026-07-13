//! Observation protocol — executes an AgentActionRequest, produces a BrowserObservation.

use std::{collections::HashMap, sync::Arc};

use chrono::Utc;
use quarry_browser::actions::{Action, ScrollTarget};
use quarry_browser::{BrowserDevtoolsEvent, BrowserDriver};
use quarry_core::contracts::{
    AgentAction, AgentActionRequest, BrowserObservation, ConsoleLine, DomSummary,
    InteractiveElement, NetworkEntry,
};
use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_core::event::EventType;
use quarry_core::ids::kinds::ArtifactKind;
use quarry_core::zdr::{self, WriteKind, ZdrMode};
use serde_json::json;

use crate::artifact_store::ArtifactStore;
use crate::events::EventSink;
use crate::vision::{VisualChangeArtifact, VisualObservationInput, VisualObservationProcessor};

pub struct ObservationRunner {
    pub browser: Arc<dyn BrowserDriver>,
    pub artifacts: Option<Arc<dyn ArtifactStore>>,
    pub events: Option<EventSink>,
    pub visual_processor: Option<Arc<dyn VisualObservationProcessor>>,
}

pub struct ObservationContext {
    pub step: u32,
    pub current_url: String,
    pub page_hash: String,
    pub previous_screenshot: Option<Vec<u8>>,
}

impl ObservationRunner {
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
            observed_at: Utc::now(),
        };

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
        let handle = store.put(run_id, page_hash, kind, body).await?;
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

            let selector = if let Some(id) = extract_attr(element_str, "id") {
                format!("#{id}")
            } else if let Some(name) = extract_attr(element_str, "name") {
                format!("{tag}[name=\"{name}\"]")
            } else {
                tag.to_string()
            };

            let text = extract_inner_text(remaining);
            let role = extract_attr(element_str, "role").map(|s| s.to_string());

            interactive_elements.push(InteractiveElement {
                tag: tag.to_string(),
                selector,
                text,
                role,
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

fn extract_attr<'a>(element: &'a str, attr: &str) -> Option<&'a str> {
    let patterns = [format!(r#"{}=""#, attr), format!("{}='", attr)];
    for pattern in &patterns {
        if let Some(start) = element.find(pattern.as_str()) {
            let val_start = start + pattern.len();
            let quote = element.as_bytes()[start + pattern.len() - 1];
            if let Some(end) = element[val_start..].find(quote as char) {
                return Some(&element[val_start..val_start + end]);
            }
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
    fn dom_summary_counts_nodes() {
        let html = "<html><body><a href='#' id='link1'>Click</a><button>Go</button></body></html>";
        let summary = build_dom_summary(html);
        assert!(summary.node_count > 0);
        assert!(!summary.interactive_elements.is_empty());
        let link = summary
            .interactive_elements
            .iter()
            .find(|e| e.tag == "a")
            .unwrap();
        assert_eq!(link.selector, "#link1");
        assert_eq!(link.text.as_deref(), Some("Click"));
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
        let store = Arc::new(InMemoryStore::with_org("org_a"));
        let runner = ObservationRunner {
            browser: Arc::new(crate::tests::MockBrowserDriver),
            artifacts: Some(store.clone()),
            events: None,
            visual_processor: None,
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
        let store = Arc::new(InMemoryStore::with_org("org_a"));
        let runner = ObservationRunner {
            browser: Arc::new(crate::tests::MockBrowserDriver),
            artifacts: Some(store.clone()),
            events: None,
            visual_processor: None,
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
        let store = Arc::new(InMemoryStore::with_org("org_a"));
        let browser = Arc::new(crate::tests::MockBrowserDriver);
        let runner = ObservationRunner {
            browser: browser.clone(),
            artifacts: Some(store.clone()),
            events: None,
            visual_processor: Some(Arc::new(MockVisualProcessor)),
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
        };
        let mut ctx = ObservationContext {
            step: 1,
            current_url: "https://example.com".into(),
            page_hash: "blake3:page".into(),
            previous_screenshot: Some(b"previous".to_vec()),
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
        let bytes = store.get(&obs_id).await.unwrap();
        let artifact: VisualObservationArtifact = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(artifact.change_ratio, 0.42);
        assert!(artifact.change_artifact_id.is_some());
        assert!(artifact.related_artifacts.contains_key("visual_change"));
        assert!(artifact.related_artifacts.contains_key("rendered_palette"));
    }
}
