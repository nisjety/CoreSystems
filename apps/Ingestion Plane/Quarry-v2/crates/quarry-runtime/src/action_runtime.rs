//! Action runtime — executes an `ActionScript` against an optional `BrowserDriver`.
//!
//! When no driver is attached, actions are traced and produce empty payloads (dry-run).
//! When a driver + session are attached, `Navigate`, `Screenshot`, and `Pdf` invoke the
//! driver; other actions return `QuarryError::unsupported_action` until full CDP wiring lands.

use std::sync::Arc;

use std::time::{Duration, Instant};

use quarry_browser::{
    actions::{Action, ActionScript, OnError, ScrollTarget},
    BrowserDriver, BrowserEgressPolicy, BrowserSession,
};
use quarry_core::{
    capture::{CaptureLimits, ImageFormat},
    error::{ErrorCode, QuarryError, QuarryResult},
    event::EventType,
    ids::kinds::{ArtifactKind, RunKind},
    lease::BrowserLease,
};
use serde_json::json;
use quarry_core::scrape_actions::{
    ActionError, ActionOnError, ActionOutcome, ActionOutput, ActionStatus, ScrapeAction, ScrollDirection,
    ScrollTo, ValidatedActions,
};
use tokio_util::sync::CancellationToken;
use tracing::{trace, warn};

use crate::{artifact_store::ArtifactStore, capture::ActionOutputBytes, events::EventSink};

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
    /// Network authority for every browser page this runtime can create. It
    /// starts deny-all: ActionRuntime is used outside the agent-route request
    /// type, so it must not silently invent a broader grant from an action
    /// script's URLs.
    egress_policy: BrowserEgressPolicy,
    /// Verified org of the owning run — stamped onto every screenshot/PDF this
    /// runtime stores so the bytes are only readable back by that tenant. Set
    /// via [`ActionRuntime::with_run_context`].
    org_id: String,
    /// WS13.5: `false` for a runtime attached to a session another fetch holds.
    /// Such a runtime never acquires or releases that session.
    owns_session: bool,
    /// WS13.5: the point a direction scroll wheels at (CSS pixels).
    viewport_centre: (f64, f64),
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
            egress_policy: BrowserEgressPolicy::deny_all(),
            org_id: String::new(),
            owns_session: true,
            viewport_centre: DEFAULT_VIEWPORT_CENTRE,
        }
    }

    pub fn with_browser(browser: Arc<dyn BrowserDriver>) -> Self {
        Self {
            browser: Some(browser),
            ..Self::new()
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

    /// Supply the already-authorized, canonical host policy for this action
    /// run. Without this explicit call, browser acquisition remains deny-all
    /// and a driver that cannot install the boundary is rejected before any
    /// navigation can occur.
    pub fn with_egress_policy(mut self, policy: BrowserEgressPolicy) -> Self {
        self.egress_policy = policy;
        self
    }

    /// `org_id` must be the verified org claim of the run — it becomes the
    /// tenant of record for every artifact this runtime writes.
    pub fn with_run_context(
        mut self,
        org_id: impl Into<String>,
        run_id: RunKind,
        page_hash: impl Into<String>,
    ) -> Self {
        self.org_id = org_id.into();
        self.run_id = Some(run_id);
        self.page_hash = Some(page_hash.into());
        self
    }

    pub async fn acquire(&mut self, lease: &BrowserLease) -> QuarryResult<()> {
        if !self.owns_session {
            return Err(QuarryError::new(
                ErrorCode::BadRequest,
                "an attached runtime does not acquire a session",
            )
            .with_details(serde_json::json!({ "reason": "attached_runtime" })));
        }
        if let Some(driver) = &self.browser {
            let session = driver.acquire(lease).await?;
            if let Err(error) = driver
                .configure_egress_policy(&session, self.egress_policy.clone())
                .await
            {
                // Do not leave a session around after a boundary-install
                // failure; a future caller must acquire a clean context.
                let _ = driver.release(session).await;
                return Err(error);
            }
            self.session = Some(session);
        }
        Ok(())
    }

    pub async fn release(&mut self) -> QuarryResult<()> {
        if !self.owns_session {
            return Ok(());
        }
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
                        warn!(action_index = index, error_class = err.code.wire_name(), "action failed, continuing");
                        ActionResult::err(index, err.to_string())
                    }
                    OnError::Retry => {
                        warn!(action_index = index, error_class = err.code.wire_name(), "action failed, retrying once");
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
            // WS12.S2: an action's error text can echo page content; the event is
            // kept beyond the caller, so it passes the Durable rules.
            let error = result.error.as_deref().map(|error| {
                quarry_core::redaction::redact_text(
                    error,
                    quarry_core::redaction::Profile::Durable,
                    &mut quarry_core::redaction::Tally::new(),
                )
                .into_owned()
            });
            let payload = json!({
                "action_index": result.index,
                "error": error,
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
                    trace!(action_index = index, "navigate (dry-run)");
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
                            .put(
                                &self.org_id,
                                run_id,
                                page_hash,
                                "screenshot",
                                payload.clone(),
                            )
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
                        let handle = store
                            .put(&self.org_id, run_id, page_hash, "pdf", payload.clone())
                            .await?;
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
                    trace!(action_index = index, timeout_ms = *timeout_ms, "wait_for (dry-run)");
                }
                None
            }
            Action::Click { selector } => {
                if let (Some(driver), Some(session)) =
                    (self.browser.as_ref(), self.session.as_ref())
                {
                    driver.click(session, selector).await?;
                } else {
                    trace!(action_index = index, "click (dry-run)");
                }
                None
            }
            Action::ClickPoint { x, y } => {
                if let (Some(driver), Some(session)) =
                    (self.browser.as_ref(), self.session.as_ref())
                {
                    driver.click_point(session, *x, *y).await?;
                } else {
                    trace!(
                        action_index = index,
                        x = *x,
                        y = *y,
                        "click_point (dry-run)"
                    );
                }
                None
            }
            Action::Type { selector, text } => {
                if let (Some(driver), Some(session)) =
                    (self.browser.as_ref(), self.session.as_ref())
                {
                    driver.type_text(session, selector, text).await?;
                } else {
                    trace!(action_index = index, chars = text.len(), "type (dry-run)");
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
                            trace!(action_index = index, to = "top", "scroll (dry-run)")
                        }
                        ScrollTarget::Bottom => {
                            trace!(action_index = index, to = "bottom", "scroll (dry-run)")
                        }
                        ScrollTarget::Selector(_) => {
                            trace!(action_index = index, to = "selector", "scroll (dry-run)")
                        }
                    }
                }
                None
            }
            Action::MouseWheel {
                x,
                y,
                delta_x,
                delta_y,
            } => {
                if let (Some(driver), Some(session)) =
                    (self.browser.as_ref(), self.session.as_ref())
                {
                    driver
                        .mouse_wheel(session, *x, *y, *delta_x, *delta_y)
                        .await?;
                } else {
                    trace!(
                        action_index = index,
                        x = *x,
                        y = *y,
                        delta_x = *delta_x,
                        delta_y = *delta_y,
                        "mouse_wheel (dry-run)"
                    );
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
            Action::Evaluate { .. } => {
                // Refused outright, dry-run included: nothing consumes a script's
                // return value here, and the read-only isolated-world evaluator is
                // a separate action that arrives with scrape-time actions, never a
                // relaxed `Evaluate`.
                return Err(QuarryError::raw_script_denied());
            }
            Action::Select { selector, value } => {
                if let (Some(driver), Some(session)) =
                    (self.browser.as_ref(), self.session.as_ref())
                {
                    driver.select(session, selector, value).await?;
                } else {
                    trace!(action_index = index, "select (dry-run)");
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
            Action::Forward => {
                if let (Some(driver), Some(session)) =
                    (self.browser.as_ref(), self.session.as_ref())
                {
                    driver.forward(session).await?;
                } else {
                    trace!(action_index = index, "forward (dry-run)");
                }
                None
            }
            Action::Reload { ignore_cache } => {
                if let (Some(driver), Some(session)) =
                    (self.browser.as_ref(), self.session.as_ref())
                {
                    driver.reload(session, *ignore_cache).await?;
                } else {
                    trace!(action_index = index, "reload (dry-run)");
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

// WS13.5: scrape-time actions on the page a scrape has already opened.

/// The longest one action may hold the driver, before the run's deadline.
const PER_ACTION_TIMEOUT: Duration = Duration::from_secs(30);

/// The viewport centre (CSS pixels) a direction scroll wheels at when the lease
/// names no viewport.
const DEFAULT_VIEWPORT_CENTRE: (f64, f64) = (640.0, 400.0);

/// What one scrape-time action run may spend. The caller sets the deadline (the
/// request's budget less the time already used) and the wait caps.
#[derive(Debug, Clone)]
pub struct ActionBudget {
    pub deadline: Instant,
    pub max_total_wait_ms: u64,
    pub max_single_wait_ms: u64,
    pub default_wait_for_ms: u64,
    pub capture_limits: CaptureLimits,
}

/// The outcome of a scrape-time action run, in request order.
#[derive(Debug, Clone, Default)]
pub struct ScrapeActionRun {
    pub outcomes: Vec<ActionOutcome>,
    /// Output bytes per action, index-aligned with `outcomes`.
    pub outputs: Vec<Option<ActionOutputBytes>>,
    /// The action whose failure stopped the run under `abort`.
    pub aborted_at: Option<usize>,
}

/// One action's successful result.
struct StepDone {
    meta: Option<ActionOutput>,
    bytes: Option<ActionOutputBytes>,
}

impl StepDone {
    fn nothing() -> Self {
        Self {
            meta: None,
            bytes: None,
        }
    }
}

fn reason_error(code: ErrorCode, reason: &str) -> QuarryError {
    QuarryError::new(code, reason).with_details(json!({ "reason": reason }))
}

/// The typed reason of a failure: its `details.reason` when it has one, else the
/// error class. Never the driver's message, which can echo page text.
fn failure_reason(error: &QuarryError) -> String {
    error
        .details
        .as_ref()
        .and_then(|details| details.get("reason"))
        .and_then(serde_json::Value::as_str)
        .map_or_else(|| error.code.wire_name().to_owned(), str::to_owned)
}

fn selector_of(selector: &Option<String>) -> QuarryResult<&str> {
    selector
        .as_deref()
        .ok_or_else(|| reason_error(ErrorCode::BadRequest, "selector_required"))
}

/// Charge one wait against the run's budget. A single wait over its cap, or a
/// run whose waits would pass the total, is a timeout.
fn charge_wait(budget: &ActionBudget, waited_ms: &mut u64, ms: u64) -> QuarryResult<()> {
    if ms > budget.max_single_wait_ms {
        return Err(reason_error(ErrorCode::Timeout, "wait_too_long"));
    }
    if waited_ms.saturating_add(ms) > budget.max_total_wait_ms {
        return Err(reason_error(ErrorCode::Timeout, "wait_budget_exhausted"));
    }
    *waited_ms = waited_ms.saturating_add(ms);
    Ok(())
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn outcome(
    index: usize,
    kind: &str,
    status: ActionStatus,
    duration_ms: u64,
    error: Option<ActionError>,
    output: Option<ActionOutput>,
) -> ActionOutcome {
    ActionOutcome {
        index,
        kind: kind.to_owned(),
        status,
        duration_ms,
        error,
        output,
    }
}

/// Lower one validated action to driver calls and run it. Nothing here retries.
async fn lower_and_run(
    driver: &dyn BrowserDriver,
    session: &BrowserSession,
    action: &ScrapeAction,
    budget: &ActionBudget,
    waited_ms: &mut u64,
    viewport_centre: (f64, f64),
) -> QuarryResult<StepDone> {
    match action {
        ScrapeAction::Wait {
            milliseconds: Some(ms),
            ..
        } => {
            charge_wait(budget, waited_ms, *ms)?;
            tokio::time::sleep(Duration::from_millis(*ms)).await;
            Ok(StepDone::nothing())
        }
        ScrapeAction::Wait {
            selector: Some(selector),
            timeout_ms,
            ..
        } => {
            let ms = timeout_ms.unwrap_or(budget.default_wait_for_ms);
            charge_wait(budget, waited_ms, ms)?;
            let timeout = u32::try_from(ms).unwrap_or(u32::MAX);
            driver.wait_for(session, selector, timeout).await?;
            Ok(StepDone::nothing())
        }
        ScrapeAction::Wait { .. } => Err(reason_error(ErrorCode::BadRequest, "wait_needs_a_target")),
        ScrapeAction::Click { selector } => {
            driver.click(session, selector_of(selector)?).await?;
            Ok(StepDone::nothing())
        }
        ScrapeAction::Write { selector, text } => {
            driver
                .type_text(session, selector_of(selector)?, text)
                .await?;
            Ok(StepDone::nothing())
        }
        ScrapeAction::Press { key } => {
            driver.press(session, key).await?;
            Ok(StepDone::nothing())
        }
        ScrapeAction::Scroll {
            to,
            selector,
            direction,
            pixels,
        } => {
            match (to, selector, direction) {
                (Some(ScrollTo::Top), None, None) => {
                    driver.scroll(session, &ScrollTarget::Top).await?;
                }
                (Some(ScrollTo::Bottom), None, None) => {
                    driver.scroll(session, &ScrollTarget::Bottom).await?;
                }
                (None, Some(target), None) => {
                    driver
                        .scroll(session, &ScrollTarget::Selector(target.clone()))
                        .await?;
                }
                (None, None, Some(direction)) => {
                    let pixels = f64::from(pixels.unwrap_or(0));
                    let delta_y = match direction {
                        ScrollDirection::Up => -pixels,
                        ScrollDirection::Down => pixels,
                    };
                    let (x, y) = viewport_centre;
                    driver.mouse_wheel(session, x, y, 0.0, delta_y).await?;
                }
                _ => return Err(reason_error(ErrorCode::BadRequest, "scroll_needs_one_target")),
            }
            Ok(StepDone::nothing())
        }
        ScrapeAction::Select { selector, value } => {
            driver
                .select(session, selector_of(selector)?, value)
                .await?;
            Ok(StepDone::nothing())
        }
        ScrapeAction::Screenshot(options) => {
            let image = driver
                .capture_screenshot(session, options, &budget.capture_limits)
                .await?;
            let content_type = match image.format {
                ImageFormat::Png => "image/png",
                ImageFormat::Jpeg => "image/jpeg",
            };
            let meta = ActionOutput {
                kind: "screenshot".to_owned(),
                content_type: content_type.to_owned(),
                bytes: u64::try_from(image.bytes.len()).unwrap_or(u64::MAX),
                width: Some(image.width_px),
                height: Some(image.height_px),
            };
            Ok(StepDone {
                meta: Some(meta),
                bytes: Some(ActionOutputBytes {
                    content_type: content_type.to_owned(),
                    bytes: image.bytes,
                }),
            })
        }
        ScrapeAction::Pdf(options) => {
            let pdf = driver
                .capture_pdf(session, options, &budget.capture_limits)
                .await?;
            let meta = ActionOutput {
                kind: "pdf".to_owned(),
                content_type: "application/pdf".to_owned(),
                bytes: u64::try_from(pdf.bytes.len()).unwrap_or(u64::MAX),
                width: None,
                height: None,
            };
            Ok(StepDone {
                meta: Some(meta),
                bytes: Some(ActionOutputBytes {
                    content_type: "application/pdf".to_owned(),
                    bytes: pdf.bytes,
                }),
            })
        }
    }
}

impl ActionRuntime {
    /// WS13.5: a runtime over a session that a scrape already holds. It never
    /// acquires or releases the session (the fetch that opened it does), and it
    /// has no artifact store, event sink or run context.
    pub fn attached(driver: Arc<dyn BrowserDriver>, session: BrowserSession) -> Self {
        let viewport_centre = session
            .lease
            .viewport
            .as_ref()
            .map_or(DEFAULT_VIEWPORT_CENTRE, |viewport| {
                (f64::from(viewport.width) / 2.0, f64::from(viewport.height) / 2.0)
            });
        Self {
            browser: Some(driver),
            session: Some(session),
            owns_session: false,
            viewport_centre,
            ..Self::new()
        }
    }

    /// WS13.5: run a request's validated actions in order on the attached page.
    ///
    /// A missing driver or session is an `Err`, never a dry run. Each action
    /// runs under the per-action timeout and the run's deadline, is never
    /// retried, and reports a reason class (never the driver's message, which
    /// can echo page text or the text typed). Under `abort` the first failure
    /// stops the run and every later action is `skipped`; under `continue` the
    /// run goes on. A cancelled token or an expired deadline stops the run before
    /// the next action.
    pub async fn run_scrape(
        &self,
        plan: &ValidatedActions,
        budget: &ActionBudget,
        cancel: &CancellationToken,
    ) -> QuarryResult<ScrapeActionRun> {
        let (Some(driver), Some(session)) = (self.browser.as_ref(), self.session.as_ref()) else {
            return Err(reason_error(ErrorCode::DriverFailed, "no_browser_session"));
        };
        let mut run = ScrapeActionRun::default();
        let mut waited_ms = 0_u64;
        for (index, action) in plan.actions.iter().enumerate() {
            let kind = action.type_name();
            if run.aborted_at.is_some() {
                run.outcomes
                    .push(outcome(index, kind, ActionStatus::Skipped, 0, None, None));
                run.outputs.push(None);
                continue;
            }
            let started = Instant::now();
            let step: QuarryResult<StepDone> = if cancel.is_cancelled() {
                Err(reason_error(ErrorCode::Timeout, "actions_cancelled"))
            } else if started >= budget.deadline {
                Err(reason_error(ErrorCode::Timeout, "actions_deadline"))
            } else {
                let remaining = budget.deadline.saturating_duration_since(started);
                let work = lower_and_run(
                    driver.as_ref(),
                    session,
                    action,
                    budget,
                    &mut waited_ms,
                    self.viewport_centre,
                );
                tokio::select! {
                    biased;
                    () = cancel.cancelled() => {
                        Err(reason_error(ErrorCode::Timeout, "actions_cancelled"))
                    }
                    done = tokio::time::timeout(remaining.min(PER_ACTION_TIMEOUT), work) => {
                        done.unwrap_or_else(|_| {
                            Err(reason_error(ErrorCode::Timeout, "actions_deadline"))
                        })
                    }
                }
            };
            match step {
                Ok(done) => {
                    run.outcomes.push(outcome(
                        index,
                        kind,
                        ActionStatus::Ok,
                        elapsed_ms(started),
                        None,
                        done.meta,
                    ));
                    run.outputs.push(done.bytes);
                }
                Err(error) => {
                    let error_view = ActionError {
                        code: error.code.wire_name().to_owned(),
                        message: failure_reason(&error),
                    };
                    run.outcomes.push(outcome(
                        index,
                        kind,
                        ActionStatus::Failed,
                        elapsed_ms(started),
                        Some(error_view),
                        None,
                    ));
                    run.outputs.push(None);
                    if plan.on_error == ActionOnError::Abort {
                        run.aborted_at = Some(index);
                    }
                }
            }
        }
        Ok(run)
    }
}

#[cfg(test)]
mod scrape_run_tests {
    use super::*;
    use async_trait::async_trait;
    use bytes::Bytes;
    use quarry_browser::SessionInner;
    use quarry_core::capture::{CapturedImage, ScreenshotOptions};
    use quarry_core::ids::kinds;
    use quarry_core::lease::{BrowserViewport, ProxyAffinity};
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex as StdMutex;
    use tokio::sync::Mutex;

    /// Records each call by kind and target, never by its text. Fails the
    /// calls aimed at `fail_selector` with a typed reason.
    #[derive(Default)]
    struct Recorder {
        calls: StdMutex<Vec<String>>,
        acquires: AtomicUsize,
        releases: AtomicUsize,
        fail_selector: Option<String>,
    }

    impl Recorder {
        fn failing(selector: &str) -> Self {
            Self {
                fail_selector: Some(selector.to_owned()),
                ..Self::default()
            }
        }

        fn log(&self, call: String) {
            self.calls.lock().unwrap().push(call);
        }

        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }

        fn check(&self, selector: &str) -> QuarryResult<()> {
            if self.fail_selector.as_deref() == Some(selector) {
                return Err(reason_error(ErrorCode::DriverFailed, "element_not_found"));
            }
            Ok(())
        }
    }

    #[async_trait]
    impl BrowserDriver for Recorder {
        async fn acquire(&self, _lease: &BrowserLease) -> QuarryResult<BrowserSession> {
            self.acquires.fetch_add(1, Ordering::SeqCst);
            Ok(session(None))
        }

        async fn release(&self, _session: BrowserSession) -> QuarryResult<()> {
            self.releases.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        async fn goto(&self, _session: &BrowserSession, _url: &str) -> QuarryResult<()> {
            Ok(())
        }

        async fn content(&self, _session: &BrowserSession) -> QuarryResult<Bytes> {
            Ok(Bytes::new())
        }

        async fn screenshot(
            &self,
            _session: &BrowserSession,
            _full_page: bool,
        ) -> QuarryResult<Bytes> {
            Ok(Bytes::from_static(&[1, 2, 3]))
        }

        async fn pdf(&self, _session: &BrowserSession) -> QuarryResult<Bytes> {
            Ok(Bytes::from_static(&[4, 5]))
        }

        async fn click(&self, _session: &BrowserSession, selector: &str) -> QuarryResult<()> {
            self.log(format!("click {selector}"));
            self.check(selector)
        }

        async fn type_text(
            &self,
            _session: &BrowserSession,
            selector: &str,
            _text: &str,
        ) -> QuarryResult<()> {
            self.log(format!("type {selector}"));
            self.check(selector)
        }

        async fn wait_for(
            &self,
            _session: &BrowserSession,
            selector: &str,
            timeout_ms: u32,
        ) -> QuarryResult<()> {
            self.log(format!("wait_for {selector} {timeout_ms}"));
            self.check(selector)
        }

        async fn press(&self, _session: &BrowserSession, key: &str) -> QuarryResult<()> {
            self.log(format!("press {key}"));
            Ok(())
        }

        async fn select(
            &self,
            _session: &BrowserSession,
            selector: &str,
            value: &str,
        ) -> QuarryResult<()> {
            self.log(format!("select {selector} {value}"));
            self.check(selector)
        }

        async fn scroll(
            &self,
            _session: &BrowserSession,
            target: &ScrollTarget,
        ) -> QuarryResult<()> {
            self.log(format!("scroll {target:?}"));
            Ok(())
        }

        async fn mouse_wheel(
            &self,
            _session: &BrowserSession,
            x: f64,
            y: f64,
            delta_x: f64,
            delta_y: f64,
        ) -> QuarryResult<()> {
            self.log(format!("wheel {x} {y} {delta_x} {delta_y}"));
            Ok(())
        }

        async fn capture_screenshot(
            &self,
            _session: &BrowserSession,
            _options: &ScreenshotOptions,
            _limits: &CaptureLimits,
        ) -> QuarryResult<CapturedImage> {
            self.log("capture screenshot".to_owned());
            Ok(CapturedImage {
                bytes: vec![9, 9],
                format: ImageFormat::Png,
                width_px: 4,
                height_px: 3,
                tiles: 1,
                stitched: false,
                truncated: false,
            })
        }
    }

    fn session(viewport: Option<BrowserViewport>) -> BrowserSession {
        BrowserSession {
            lease: BrowserLease {
                lease_id: kinds::LeaseKind::new(),
                profile_id: kinds::ProfileKind::new(),
                session_affinity_key: "test".into(),
                proxy_affinity: ProxyAffinity {
                    pool: "default".into(),
                    sticky_key: None,
                },
                ttl_s: 30,
                capabilities: vec![],
                artifact_bucket: "default".into(),
                persist_profile: false,
                viewport,
                org_id: "org_test".into(),
            },
            inner: Arc::new(Mutex::new(SessionInner::default())),
            tab_pin: None,
        }
    }

    fn viewport(width: u32, height: u32) -> Option<BrowserViewport> {
        serde_json::from_value(json!({ "width": width, "height": height })).ok()
    }

    fn plan(actions: Vec<ScrapeAction>, on_error: ActionOnError) -> ValidatedActions {
        ValidatedActions {
            actions,
            on_error,
            total_wait_ms: 0,
            capture_count: 0,
            type_counts: BTreeMap::new(),
        }
    }

    fn click(selector: &str) -> ScrapeAction {
        ScrapeAction::Click {
            selector: Some(selector.to_owned()),
        }
    }

    fn wait_ms(ms: u64) -> ScrapeAction {
        ScrapeAction::Wait {
            milliseconds: Some(ms),
            selector: None,
            timeout_ms: None,
        }
    }

    fn budget(max_total_wait_ms: u64) -> ActionBudget {
        ActionBudget {
            deadline: Instant::now() + Duration::from_secs(30),
            max_total_wait_ms,
            max_single_wait_ms: 30_000,
            default_wait_for_ms: 5_000,
            capture_limits: CaptureLimits::default(),
        }
    }

    fn attached(recorder: &Arc<Recorder>, viewport: Option<BrowserViewport>) -> ActionRuntime {
        let driver: Arc<dyn BrowserDriver> = recorder.clone();
        ActionRuntime::attached(driver, session(viewport))
    }

    #[tokio::test]
    async fn attached_runtime_never_acquires_or_releases_the_session() {
        let recorder = Arc::new(Recorder::default());
        let mut runtime = attached(&recorder, None);
        let lease = session(None).lease;
        assert!(runtime.acquire(&lease).await.is_err());
        runtime.release().await.unwrap();
        assert_eq!(recorder.acquires.load(Ordering::SeqCst), 0);
        assert_eq!(recorder.releases.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn run_scrape_runs_actions_in_order_and_records_ordered_outcomes() {
        let recorder = Arc::new(Recorder::default());
        let runtime = attached(&recorder, None);
        let actions = plan(
            vec![
                click("#swap"),
                wait_ms(1),
                ScrapeAction::Press {
                    key: "Enter".into(),
                },
            ],
            ActionOnError::Abort,
        );
        let run = runtime
            .run_scrape(&actions, &budget(60_000), &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(recorder.calls(), ["click #swap", "press Enter"]);
        assert!(run.outcomes.iter().all(|o| o.status == ActionStatus::Ok));
        let indices: Vec<usize> = run.outcomes.iter().map(|o| o.index).collect();
        assert_eq!(indices, vec![0, 1, 2]);
        assert_eq!(run.aborted_at, None);
    }

    #[tokio::test]
    async fn abort_marks_later_actions_skipped() {
        let recorder = Arc::new(Recorder::failing("#missing"));
        let runtime = attached(&recorder, None);
        let actions = plan(vec![click("#missing"), click("#later")], ActionOnError::Abort);
        let run = runtime
            .run_scrape(&actions, &budget(60_000), &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(run.aborted_at, Some(0));
        assert_eq!(run.outcomes[0].status, ActionStatus::Failed);
        assert_eq!(
            run.outcomes[0].error.as_ref().unwrap().message,
            "element_not_found"
        );
        assert_eq!(run.outcomes[1].status, ActionStatus::Skipped);
        assert_eq!(recorder.calls(), ["click #missing"]);
    }

    #[tokio::test]
    async fn continue_records_failure_and_keeps_going() {
        let recorder = Arc::new(Recorder::failing("#missing"));
        let runtime = attached(&recorder, None);
        let actions = plan(
            vec![click("#missing"), click("#ok")],
            ActionOnError::Continue,
        );
        let run = runtime
            .run_scrape(&actions, &budget(60_000), &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(run.aborted_at, None);
        assert_eq!(run.outcomes[0].status, ActionStatus::Failed);
        assert_eq!(run.outcomes[1].status, ActionStatus::Ok);
        assert_eq!(recorder.calls(), ["click #missing", "click #ok"]);
    }

    #[tokio::test]
    async fn no_retry_on_failure() {
        let recorder = Arc::new(Recorder::failing("#submit"));
        let runtime = attached(&recorder, None);
        let actions = plan(vec![click("#submit")], ActionOnError::Continue);
        runtime
            .run_scrape(&actions, &budget(60_000), &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(recorder.calls(), ["click #submit"]);
    }

    #[tokio::test]
    async fn scroll_direction_maps_to_mouse_wheel_at_viewport_centre() {
        let recorder = Arc::new(Recorder::default());
        let runtime = attached(&recorder, viewport(1000, 800));
        let actions = plan(
            vec![ScrapeAction::Scroll {
                to: None,
                selector: None,
                direction: Some(ScrollDirection::Down),
                pixels: Some(300),
            }],
            ActionOnError::Abort,
        );
        runtime
            .run_scrape(&actions, &budget(60_000), &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(recorder.calls(), ["wheel 500 400 0 300"]);
    }

    #[tokio::test]
    async fn wait_budget_and_deadline_stop_the_run_with_timeout() {
        let recorder = Arc::new(Recorder::default());
        let runtime = attached(&recorder, None);
        let actions = plan(vec![wait_ms(40), wait_ms(40)], ActionOnError::Abort);
        let run = runtime
            .run_scrape(&actions, &budget(50), &CancellationToken::new())
            .await
            .unwrap();
        let failed = run.outcomes[1].error.as_ref().unwrap();
        assert_eq!(failed.code, "TIMEOUT");
        assert_eq!(failed.message, "wait_budget_exhausted");

        let expired = ActionBudget {
            deadline: Instant::now(),
            ..budget(60_000)
        };
        let run = runtime
            .run_scrape(&plan(vec![click("#a")], ActionOnError::Abort), &expired, &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(
            run.outcomes[0].error.as_ref().unwrap().message,
            "actions_deadline"
        );
    }

    #[tokio::test]
    async fn cancel_token_stops_between_actions() {
        let recorder = Arc::new(Recorder::default());
        let runtime = attached(&recorder, None);
        let cancel = CancellationToken::new();
        cancel.cancel();
        let actions = plan(vec![click("#a")], ActionOnError::Abort);
        let run = runtime
            .run_scrape(&actions, &budget(60_000), &cancel)
            .await
            .unwrap();
        assert_eq!(
            run.outcomes[0].error.as_ref().unwrap().message,
            "actions_cancelled"
        );
        assert!(recorder.calls().is_empty());
    }

    #[tokio::test]
    async fn error_messages_never_contain_write_text() {
        let recorder = Arc::new(Recorder::failing("#q"));
        let runtime = attached(&recorder, None);
        let actions = plan(
            vec![ScrapeAction::Write {
                selector: Some("#q".into()),
                text: "SECRET_TEXT_XYZ".into(),
            }],
            ActionOnError::Continue,
        );
        let run = runtime
            .run_scrape(&actions, &budget(60_000), &CancellationToken::new())
            .await
            .unwrap();
        let serialized = serde_json::to_string(&run.outcomes).unwrap();
        assert!(!serialized.contains("SECRET_TEXT_XYZ"));
    }

    #[tokio::test]
    async fn missing_driver_or_session_is_an_error_not_dry_run() {
        let runtime = ActionRuntime::new();
        let actions = plan(vec![click("#a")], ActionOnError::Abort);
        let result = runtime
            .run_scrape(&actions, &budget(60_000), &CancellationToken::new())
            .await;
        assert!(result.is_err());
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
                Action::ClickPoint { x: 320.0, y: 240.0 },
                Action::Type {
                    selector: "#input".into(),
                    text: "hi".into(),
                },
                Action::Scroll {
                    to: ScrollTarget::Bottom,
                },
                Action::MouseWheel {
                    x: 320.0,
                    y: 240.0,
                    delta_x: 0.0,
                    delta_y: 480.0,
                },
                Action::Press {
                    key: "Enter".into(),
                },
                Action::Back,
                Action::Forward,
            ],
            OnError::Abort,
        );
        let results = runtime.run(&s).await.unwrap();
        assert_eq!(results.len(), 11);
        assert!(results.iter().all(|r| r.success));
    }

    #[tokio::test]
    async fn reload_action_runs_in_dry_run_mode() {
        let runtime = ActionRuntime::new();
        let s = script(
            vec![
                Action::Reload { ignore_cache: false },
                Action::Reload { ignore_cache: true },
            ],
            OnError::Abort,
        );
        let results = runtime.run(&s).await.unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.success));
    }

    #[tokio::test]
    async fn evaluate_action_is_refused() {
        let runtime = ActionRuntime::new();
        let s = script(
            vec![Action::Evaluate {
                script: "window.x = 1".into(),
            }],
            OnError::Abort,
        );
        let error = runtime.run(&s).await.expect_err("evaluate must be refused");
        assert_eq!(error.code, quarry_core::error::ErrorCode::Forbidden);
        assert_eq!(error.details.unwrap()["reason"], "raw_script_denied");
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
