//! Observation protocol — executes an AgentActionRequest, produces a BrowserObservation.

use std::sync::Arc;

use chrono::Utc;
use quarry_browser::actions::{Action, ScrollTarget};
use quarry_browser::BrowserDriver;
use quarry_core::contracts::{
    AgentAction, AgentActionRequest, BrowserObservation, DomSummary, InteractiveElement,
};
use quarry_core::error::QuarryResult;
use quarry_core::event::EventType;
use quarry_core::ids::kinds::ArtifactKind;
use serde_json::json;

use crate::artifact_store::ArtifactStore;
use crate::events::EventSink;

pub struct ObservationRunner {
    pub browser: Arc<dyn BrowserDriver>,
    pub artifacts: Option<Arc<dyn ArtifactStore>>,
    pub events: Option<EventSink>,
}

pub struct ObservationContext {
    pub step: u32,
    pub current_url: String,
    pub page_hash: String,
}

impl ObservationRunner {
    pub fn agent_action_to_browser_action(action: &AgentAction) -> Action {
        match action {
            AgentAction::Navigate { url } => Action::Navigate { url: url.clone() },
            AgentAction::Click { selector } => Action::Click {
                selector: selector.clone(),
            },
            AgentAction::Type { selector, text } => Action::Type {
                selector: selector.clone(),
                text: text.clone(),
            },
            AgentAction::Press { key } => Action::Press { key: key.clone() },
            AgentAction::Scroll { target } => Action::Scroll {
                to: ScrollTarget::Selector(target.clone()),
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
                .emit(
                    run_id.clone(),
                    EventType::ActionStarted,
                    json!({ "step": ctx.step, "action": serde_json::to_value(&request.action).ok() }),
                    format!("{}:action:{}:started", run_id, ctx.step),
                )
                .await;
        }

        let mut screenshot_artifact_id: Option<ArtifactKind> = None;
        let policy_denials: Vec<String> = Vec::new();

        match &request.action {
            AgentAction::Navigate { url } => {
                self.browser.goto(session, url).await?;
                ctx.current_url = url.clone();
            }
            AgentAction::Click { selector } => {
                self.browser.click(session, selector).await?;
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
                if let Some(store) = &self.artifacts {
                    let handle = store
                        .put(run_id, &ctx.page_hash, "screenshot", bytes.to_vec())
                        .await?;
                    screenshot_artifact_id = Some(handle.artifact_id);
                }
            }
            AgentAction::Pdf => {
                let bytes = self.browser.pdf(session).await?;
                if let Some(store) = &self.artifacts {
                    let _handle = store
                        .put(run_id, &ctx.page_hash, "pdf", bytes.to_vec())
                        .await?;
                }
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
                if let Some(store) = &self.artifacts {
                    if let Ok(handle) = store
                        .put(run_id, &ctx.page_hash, "screenshot", bytes.to_vec())
                        .await
                    {
                        screenshot_artifact_id = Some(handle.artifact_id);
                    }
                }
            }
        }

        let observation = BrowserObservation {
            run_id: run_id.clone(),
            step: ctx.step,
            url: ctx.current_url.clone(),
            title,
            dom_summary,
            screenshot_artifact_id,
            console_summary: vec![],
            network_summary: vec![],
            policy_denials,
            observed_at: Utc::now(),
        };

        if let Some(events) = &self.events {
            events
                .emit(
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

#[derive(Default)]
struct PageState {
    url: Option<String>,
    title: Option<String>,
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
}
