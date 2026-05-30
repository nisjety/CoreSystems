//! Action runtime — executes an `ActionScript` against an optional `BrowserDriver`.
//!
//! When no driver is attached, actions are traced and produce empty payloads (dry-run).
//! When a driver + session are attached, `Navigate`, `Screenshot`, and `Pdf` invoke the
//! driver; other actions return `QuarryError::unsupported_action` until full CDP wiring lands.

use std::sync::Arc;

use std::time::Duration;

use quarry_browser::{
    actions::{Action, ActionScript, OnError, ScrollTarget},
    BrowserDriver, BrowserSession,
};
use quarry_core::{
    error::QuarryResult,
    event::EventType,
    ids::kinds::{ArtifactKind, RunKind},
    lease::BrowserLease,
};
use serde_json::json;
use tracing::{trace, warn};

use crate::{artifact_store::ArtifactStore, events::EventSink};

#[derive(Debug, Clone)]
pub struct ActionResult {
    pub index: usize,
    pub success: bool,
    pub error: Option<String>,
    pub output: Option<Vec<u8>>,
    pub artifact_id: Option<ArtifactKind>,
}

impl ActionResult {
    pub fn ok(index: usize, output: Option<Vec<u8>>) -> Self {
        Self {
            index,
            success: true,
            error: None,
            output,
            artifact_id: None,
        }
    }

    pub fn err(index: usize, message: impl Into<String>) -> Self {
        Self {
            index,
            success: false,
            error: Some(message.into()),
            output: None,
            artifact_id: None,
        }
    }

    pub fn with_artifact(mut self, id: ArtifactKind) -> Self {
        self.artifact_id = Some(id);
        self
    }
}

pub struct ActionRuntime {
    browser: Option<Arc<dyn BrowserDriver>>,
    session: Option<BrowserSession>,
    artifact_store: Option<Arc<dyn ArtifactStore>>,
    events: Option<EventSink>,
    run_id: Option<RunKind>,
    page_hash: Option<String>,
}

impl Default for ActionRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl ActionRuntime {
    pub fn new() -> Self {
        Self {
            browser: None,
            session: None,
            artifact_store: None,
            events: None,
            run_id: None,
            page_hash: None,
        }
    }

    pub fn with_browser(browser: Arc<dyn BrowserDriver>) -> Self {
        Self {
            browser: Some(browser),
            session: None,
            artifact_store: None,
            events: None,
            run_id: None,
            page_hash: None,
        }
    }

    pub fn with_artifact_store(mut self, store: Arc<dyn ArtifactStore>) -> Self {
        self.artifact_store = Some(store);
        self
    }

    pub fn with_event_sink(mut self, events: EventSink) -> Self {
        self.events = Some(events);
        self
    }

    pub fn with_run_context(mut self, run_id: RunKind, page_hash: impl Into<String>) -> Self {
        self.run_id = Some(run_id);
        self.page_hash = Some(page_hash.into());
        self
    }

    pub async fn acquire(&mut self, lease: &BrowserLease) -> QuarryResult<()> {
        if let Some(driver) = &self.browser {
            self.session = Some(driver.acquire(lease).await?);
        }
        Ok(())
    }

    pub async fn release(&mut self) -> QuarryResult<()> {
        if let (Some(driver), Some(session)) = (self.browser.as_ref(), self.session.take()) {
            driver.release(session).await?;
        }
        Ok(())
    }

    pub async fn run(&self, script: &ActionScript) -> QuarryResult<Vec<ActionResult>> {
        let mut results = Vec::with_capacity(script.actions.len());
        for (index, action) in script.actions.iter().enumerate() {
            let result = match self.execute_action(index, action).await {
                Ok(result) => result,
                Err(err) => match script.on_error {
                    OnError::Abort => {
                        return Err(err);
                    }
                    OnError::Continue => {
                        warn!(action_index = index, error = %err, "action failed, continuing");
                        ActionResult::err(index, err.to_string())
                    }
                    OnError::Retry => {
                        warn!(action_index = index, error = %err, "action failed, retrying once");
                        match self.execute_action(index, action).await {
                            Ok(result) => result,
                            Err(retry_err) => ActionResult::err(index, retry_err.to_string()),
                        }
                    }
                },
            };
            self.emit_action_event(&result).await;
            results.push(result);
        }
        Ok(results)
    }

    async fn emit_action_event(&self, result: &ActionResult) {
        if let (Some(events), Some(run_id)) = (self.events.as_ref(), self.run_id.as_ref()) {
            let event_type = if result.success {
                EventType::PageFetched
            } else {
                EventType::PageFailed
            };
            let payload = json!({
                "action_index": result.index,
                "error": result.error,
                "artifact_id": result.artifact_id.as_ref().map(|a| a.to_string()),
            });
            let idempotency_key = format!("{}:{}", run_id, result.index);
            events
                .emit(run_id.clone(), event_type, payload, idempotency_key)
                .await;
        }
    }

    async fn execute_action(&self, index: usize, action: &Action) -> QuarryResult<ActionResult> {
        let mut artifact_id: Option<ArtifactKind> = None;
        let output: Option<Vec<u8>> = match action {
            Action::Navigate { url } => {
                if let (Some(driver), Some(session)) =
                    (self.browser.as_ref(), self.session.as_ref())
                {
                    driver.goto(session, url).await?;
                } else {
                    trace!(action_index = index, url = %url, "navigate (dry-run)");
                }
                None
            }
            Action::Screenshot { full_page } => {
                if let (Some(driver), Some(session)) =
                    (self.browser.as_ref(), self.session.as_ref())
                {
                    let bytes = driver.screenshot(session, *full_page).await?;
                    let payload = bytes.to_vec();
                    if let (Some(store), Some(run_id), Some(page_hash)) = (
                        self.artifact_store.as_ref(),
                        self.run_id.as_ref(),
                        self.page_hash.as_ref(),
                    ) {
                        let handle = store
                            .put(run_id, page_hash, "screenshot", payload.clone())
                            .await?;
                        artifact_id = Some(handle.artifact_id);
                    }
                    Some(payload)
                } else {
                    Some(Vec::new())
                }
            }
            Action::Pdf => {
                if let (Some(driver), Some(session)) =
                    (self.browser.as_ref(), self.session.as_ref())
                {
                    let bytes = driver.pdf(session).await?;
                    let payload = bytes.to_vec();
                    if let (Some(store), Some(run_id), Some(page_hash)) = (
                        self.artifact_store.as_ref(),
                        self.run_id.as_ref(),
                        self.page_hash.as_ref(),
                    ) {
                        let handle = store.put(run_id, page_hash, "pdf", payload.clone()).await?;
                        artifact_id = Some(handle.artifact_id);
                    }
                    Some(payload)
                } else {
                    Some(Vec::new())
                }
            }
            Action::Wait { ms } => {
                // Pure sleep; no driver dependency.
                tokio::time::sleep(Duration::from_millis(*ms as u64)).await;
                trace!(action_index = index, ms = *ms, "wait");
                None
            }
            Action::WaitFor {
                selector,
                timeout_ms,
            } => {
                if let (Some(driver), Some(session)) =
                    (self.browser.as_ref(), self.session.as_ref())
                {
                    driver.wait_for(session, selector, *timeout_ms).await?;
                } else {
                    trace!(action_index = index, selector = %selector, timeout_ms = *timeout_ms, "wait_for (dry-run)");
                }
                None
            }
            Action::Click { selector } => {
                if let (Some(driver), Some(session)) =
                    (self.browser.as_ref(), self.session.as_ref())
                {
                    driver.click(session, selector).await?;
                } else {
                    trace!(action_index = index, selector = %selector, "click (dry-run)");
                }
                None
            }
            Action::Type { selector, text } => {
                if let (Some(driver), Some(session)) =
                    (self.browser.as_ref(), self.session.as_ref())
                {
                    driver.type_text(session, selector, text).await?;
                } else {
                    trace!(action_index = index, selector = %selector, chars = text.len(), "type (dry-run)");
                }
                None
            }
            Action::Scroll { to } => {
                if let (Some(driver), Some(session)) =
                    (self.browser.as_ref(), self.session.as_ref())
                {
                    driver.scroll(session, to).await?;
                } else {
                    match to {
                        ScrollTarget::Top => {
                            trace!(action_index = index, target = "top", "scroll (dry-run)")
                        }
                        ScrollTarget::Bottom => {
                            trace!(action_index = index, target = "bottom", "scroll (dry-run)")
                        }
                        ScrollTarget::Selector(sel) => {
                            trace!(action_index = index, target = %sel, "scroll (dry-run)")
                        }
                    }
                }
                None
            }
            Action::Press { key } => {
                if let (Some(driver), Some(session)) =
                    (self.browser.as_ref(), self.session.as_ref())
                {
                    driver.press(session, key).await?;
                } else {
                    trace!(action_index = index, key = %key, "press (dry-run)");
                }
                None
            }
            Action::Evaluate { script } => {
                if let (Some(driver), Some(session)) =
                    (self.browser.as_ref(), self.session.as_ref())
                {
                    let value = driver.evaluate(session, script).await?;
                    Some(serde_json::to_vec(&value).unwrap_or_default())
                } else {
                    trace!(
                        action_index = index,
                        script_len = script.len(),
                        "evaluate (dry-run)"
                    );
                    None
                }
            }
            Action::Select { selector, value } => {
                if let (Some(driver), Some(session)) =
                    (self.browser.as_ref(), self.session.as_ref())
                {
                    driver.select(session, selector, value).await?;
                } else {
                    trace!(action_index = index, selector = %selector, value = %value, "select (dry-run)");
                }
                None
            }
            Action::Back => {
                if let (Some(driver), Some(session)) =
                    (self.browser.as_ref(), self.session.as_ref())
                {
                    driver.back(session).await?;
                } else {
                    trace!(action_index = index, "back (dry-run)");
                }
                None
            }
        };

        let mut result = ActionResult::ok(index, output);
        if let Some(id) = artifact_id {
            result = result.with_artifact(id);
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarry_browser::actions::{Action, ActionScript, OnError, ScrollTarget};

    fn script(actions: Vec<Action>, on_error: OnError) -> ActionScript {
        ActionScript { actions, on_error }
    }

    #[tokio::test]
    async fn empty_script_returns_empty_results() {
        let runtime = ActionRuntime::new();
        let s = script(vec![], OnError::Abort);
        let results = runtime.run(&s).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn all_actions_succeed() {
        let runtime = ActionRuntime::new();
        let s = script(
            vec![
                Action::Navigate {
                    url: "https://example.com".into(),
                },
                Action::Wait { ms: 10 },
                Action::WaitFor {
                    selector: "#ready".into(),
                    timeout_ms: 1000,
                },
                Action::Click {
                    selector: "#btn".into(),
                },
                Action::Type {
                    selector: "#input".into(),
                    text: "hi".into(),
                },
                Action::Scroll {
                    to: ScrollTarget::Bottom,
                },
                Action::Press {
                    key: "Enter".into(),
                },
                Action::Evaluate {
                    script: "window.x = 1".into(),
                },
            ],
            OnError::Abort,
        );
        let results = runtime.run(&s).await.unwrap();
        assert_eq!(results.len(), 8);
        assert!(results.iter().all(|r| r.success));
    }

    #[tokio::test]
    async fn screenshot_produces_output_payload() {
        let runtime = ActionRuntime::new();
        let s = script(vec![Action::Screenshot { full_page: true }], OnError::Abort);
        let results = runtime.run(&s).await.unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].success);
        assert!(results[0].output.is_some());
    }

    #[tokio::test]
    async fn pdf_produces_output_payload() {
        let runtime = ActionRuntime::new();
        let s = script(vec![Action::Pdf], OnError::Abort);
        let results = runtime.run(&s).await.unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].success);
        assert!(results[0].output.is_some());
    }

    #[tokio::test]
    async fn results_carry_correct_indices() {
        let runtime = ActionRuntime::new();
        let s = script(
            vec![
                Action::Click {
                    selector: "#a".into(),
                },
                Action::Click {
                    selector: "#b".into(),
                },
                Action::Click {
                    selector: "#c".into(),
                },
            ],
            OnError::Abort,
        );
        let results = runtime.run(&s).await.unwrap();
        for (i, r) in results.iter().enumerate() {
            assert_eq!(r.index, i);
            assert!(r.success);
        }
    }
}
