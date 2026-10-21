//! Actionability checks and trusted input for ref-targeted actions (WS3.K).
//!
//! A ref action used to run `element.click()` or a value setter inside the page:
//! it reached the element whatever was on top of it, whether it was visible,
//! enabled or still moving, and every event it fired had `isTrusted === false`.
//! With `QUARRY_BROWSER_TRUSTED_INPUT=on` a click, a type and a select on a
//! top-level ref first have to pass this pipeline, which is the one a person's
//! eyes and hands apply:
//!
//! 1. one probe (`ACTIONABILITY_PROBE_JS`) says whether the node is still in the
//!    document, visible, enabled (and editable, for typing), where its box is;
//! 2. the node is scrolled into view and its box has to hold still (two samples
//!    within half a pixel, [`stable`]);
//! 3. the click point is the middle of the part of the box that is inside the
//!    viewport ([`click_point_from_quads`]);
//! 4. a second probe asks `elementFromPoint` (shadow-root aware) what receives
//!    the pointer there. Anything that is not the target, one of its
//!    descendants or its label covers it.
//!
//! The click is then real CDP input (`Input.dispatchMouseEvent`); typing is
//! `Input.insertText` after the field was focused and its content selected, so
//! the page sees `isTrusted` events. A failure is `TARGET_REPAIR_REQUIRED` (409,
//! the same wire code as always) with `details.reason` from the closed list
//! `quarry_core::error::target_repair_reason` and, for a covered target, the
//! covering element's tag name. Nothing else about the page is ever put in an
//! error or a log line: no selector, URL, text or CDP error text.
//!
//! What stays on the synthetic path (the in-page script that was always there):
//! child-frame targets (their hit test would need the frame's own coordinate
//! space and a cross-process iframe has none here), governed downloads and
//! uploads, the legacy selector path, and `wait_for`. Cross-process iframes are
//! WS3.L.
//!
//! The pure pieces (`click_point_from_quads`, `stable`, `decide`, `into_error`)
//! have no browser in them and are unit tested; the live fixtures are in
//! `trusted_input_live.rs` next to the driver tests.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ::chromiumoxide::cdp::browser_protocol::dom::{
    FocusParams, GetContentQuadsParams, ScrollIntoViewIfNeededParams,
};
use ::chromiumoxide::cdp::browser_protocol::input::{
    DispatchMouseEventParams, DispatchMouseEventPointerType, DispatchMouseEventType,
    InsertTextParams, MouseButton,
};
use ::chromiumoxide::cdp::js_protocol::runtime::{
    CallArgument, CallFunctionOnParams, RemoteObjectId,
};
use ::chromiumoxide::Page;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::Mutex;

use quarry_core::contracts::BrowserDialog;
use quarry_core::error::{target_repair_reason as reasons, ErrorCode, QuarryError, QuarryResult};

use crate::deadlines::{cdp_class, deadline, is_context_destroyed, with_deadline, Op};
use crate::VerifiedTargetOperation;

/// How often the probe is repeated while a target is not yet actionable.
const POLL: Duration = Duration::from_millis(100);
/// How often the box is sampled while waiting for it to hold still.
const STABLE_STEP: Duration = Duration::from_millis(25);
/// Two box samples closer than this (every edge) are the same place.
pub(crate) const STABLE_TOLERANCE_PX: f64 = 0.5;
/// A box smaller than this (square CSS pixels) cannot be pointed at.
const MIN_CLICKABLE_AREA: f64 = 1.0;

/// One `Runtime.callFunctionOn` on the resolved element, `this` = the element,
/// argument `{ point: [x, y] | null }`. Returns the state the pipeline decides
/// on. It reads, it never writes, and it never returns page text: only booleans,
/// numbers and the tag name (`localName`) of the element and of whatever covers
/// it.
pub(crate) const ACTIONABILITY_PROBE_JS: &str = r#"function(request) {
    const el = this;
    const out = {
        attached: false, visible: false, enabled: false, editable: false,
        settable: false, tag: '', box: null, viewport: { w: 0, h: 0 }, hit: null
    };
    if (!(el instanceof Element) || !el.isConnected) return out;
    out.attached = true;
    out.tag = String(el.localName || '');
    const doc = el.ownerDocument;
    const win = doc.defaultView;
    out.viewport = { w: win.innerWidth, h: win.innerHeight };
    let visibility = 'visible';
    try { visibility = win.getComputedStyle(el).visibility; } catch (_) {}
    out.visible = el.getClientRects().length > 0 && visibility === 'visible';
    const rect = el.getBoundingClientRect();
    out.box = { x: rect.x, y: rect.y, w: rect.width, h: rect.height };
    let enabled = true;
    try { if (el.matches(':disabled')) enabled = false; } catch (_) {}
    if (enabled && el.closest('[aria-disabled="true"]')) enabled = false;
    out.enabled = enabled;
    if (el instanceof HTMLTextAreaElement) {
        out.settable = !el.readOnly;
        out.editable = !el.readOnly;
    } else if (el instanceof HTMLInputElement) {
        const textual = ['text', 'search', 'url', 'tel', 'email', 'password', 'number'];
        out.settable = !el.readOnly;
        out.editable = !el.readOnly && textual.includes(String(el.type || 'text').toLowerCase());
    } else {
        out.editable = !!el.isContentEditable;
    }
    if (request && Array.isArray(request.point)) {
        const x = request.point[0];
        const y = request.point[1];
        let hit = doc.elementFromPoint(x, y);
        for (let guard = 0; hit && hit.shadowRoot && guard < 32; guard++) {
            const inner = hit.shadowRoot.elementFromPoint(x, y);
            if (!inner || inner === hit) break;
            hit = inner;
        }
        let ok = false;
        if (hit) {
            const label = hit.closest ? hit.closest('label') : null;
            if (label && label.control === el) ok = true;
            for (let node = hit, guard = 0; node && !ok && guard < 256; guard++) {
                if (node === el) { ok = true; break; }
                const up = node.assignedSlot || node.parentNode;
                node = up && up.nodeType === 11 && up.host ? up.host : up;
            }
        }
        out.hit = {
            ok: ok,
            covered_by_tag: ok || !hit ? null : String(hit.localName || '')
        };
    }
    return out;
}"#;

/// Select all of a field's content (or clear it), after `DOM.focus`. Returns
/// `{ ok }` or `{ ok: false, reason }` with an in-page legacy reason.
const SELECT_ALL_JS: &str = r#"function(request) {
    const el = this;
    if (!(el instanceof Element) || !el.isConnected) {
        return { ok: false, reason: 'native_target_not_connected' };
    }
    if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement) {
        el.focus();
        try { el.select(); } catch (_) {}
    } else if (el.isContentEditable) {
        el.focus();
        const doc = el.ownerDocument;
        const selection = doc.defaultView.getSelection();
        const range = doc.createRange();
        range.selectNodeContents(el);
        selection.removeAllRanges();
        selection.addRange(range);
    } else {
        return { ok: false, reason: 'target_not_typeable' };
    }
    if (request && request.clear) el.ownerDocument.execCommand('delete');
    return { ok: true };
}"#;

/// The in-page value setter, for a field that is settable but not a text
/// control (a date, a color, a range): there is no key or text to send it, so
/// it keeps the setter it always had, after the visibility and enabled checks.
const SET_VALUE_JS: &str = r#"function(request) {
    const el = this;
    if (!(el instanceof Element) || !el.isConnected) {
        return { ok: false, reason: 'native_target_not_connected' };
    }
    if (!('value' in el)) return { ok: false, reason: 'target_not_typeable' };
    el.focus();
    const proto = el instanceof HTMLTextAreaElement
        ? HTMLTextAreaElement.prototype
        : HTMLInputElement.prototype;
    const setter = Object.getOwnPropertyDescriptor(proto, 'value')?.set;
    if (!setter) return { ok: false, reason: 'value_setter_unavailable' };
    setter.call(el, request.text);
    el.dispatchEvent(new InputEvent('input', {
        bubbles: true, inputType: 'insertText', data: request.text
    }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
    return { ok: true };
}"#;

/// Choosing an option has no trusted-input equivalent: like any automation
/// library, it sets the option and fires `input` and `change` in the page.
const SELECT_OPTION_JS: &str = r#"function(request) {
    const el = this;
    if (!(el instanceof Element) || !el.isConnected) {
        return { ok: false, reason: 'native_target_not_connected' };
    }
    if (!(el instanceof HTMLSelectElement)) return { ok: false, reason: 'target_not_select' };
    el.value = request.value;
    if (el.value !== request.value) return { ok: false, reason: 'select_value_unavailable' };
    el.dispatchEvent(new Event('input', { bubbles: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
    return { ok: true };
}"#;

/// `QUARRY_BROWSER_TRUSTED_INPUT`: `on` (also `1`, `true`) enables the
/// pipeline, `off`, empty or unset leaves it off. Anything else is ignored with
/// a warning and leaves it off: a typo must not switch real input on.
pub(crate) fn parse_trusted_input(raw: Option<&str>) -> bool {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return false;
    };
    match raw.to_ascii_lowercase().as_str() {
        "on" | "1" | "true" => true,
        "off" | "0" | "false" => false,
        _ => {
            tracing::warn!("QUARRY_BROWSER_TRUSTED_INPUT must be on or off; using off");
            false
        }
    }
}

/// What the action needs from its target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Intent {
    Click,
    Type,
    Select,
}

impl Intent {
    const fn label(self) -> &'static str {
        match self {
            Intent::Click => "click",
            Intent::Type => "type",
            Intent::Select => "select",
        }
    }
}

/// Why a target is not actionable. One variant per entry of the closed list in
/// `quarry_core::error::target_repair_reason`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reason {
    NotAttached,
    NotVisible,
    Disabled,
    Covered,
    Unstable,
    HasNoBox,
    UnreachableAfterScroll,
}

impl Reason {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Reason::NotAttached => reasons::TARGET_NOT_ATTACHED,
            Reason::NotVisible => reasons::TARGET_NOT_VISIBLE,
            Reason::Disabled => reasons::TARGET_DISABLED,
            Reason::Covered => reasons::TARGET_COVERED,
            Reason::Unstable => reasons::TARGET_UNSTABLE,
            Reason::HasNoBox => reasons::TARGET_HAS_NO_BOX,
            Reason::UnreachableAfterScroll => reasons::TARGET_UNREACHABLE_AFTER_SCROLL,
        }
    }

    /// A fixed sentence for the error message: it says what to do next, and
    /// carries nothing from the page.
    const fn message(self) -> &'static str {
        match self {
            Reason::NotAttached => "target is no longer in the page; observe again",
            Reason::NotVisible => {
                "target is not visible; make it visible or pick another target, then observe again"
            }
            Reason::Disabled => "target is disabled or not editable; observe again",
            Reason::Covered => {
                "target is covered by another element; dismiss it or scroll, then observe again"
            }
            Reason::Unstable => {
                "target keeps moving; wait for the page to settle, then observe again"
            }
            Reason::HasNoBox => "target has no area to point at; observe again",
            Reason::UnreachableAfterScroll => {
                "target is outside the viewport even after scrolling; observe again"
            }
        }
    }
}

/// A target that failed a check: the reason, and for `Covered` the tag name of
/// whatever received the hit test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Rejection {
    pub(crate) reason: Reason,
    pub(crate) covered_by_tag: Option<String>,
}

impl Rejection {
    fn of(reason: Reason) -> Self {
        Self {
            reason,
            covered_by_tag: None,
        }
    }
}

/// A box in CSS pixels relative to the viewport.
#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize)]
pub(crate) struct BoxRect {
    #[serde(default)]
    pub(crate) x: f64,
    #[serde(default)]
    pub(crate) y: f64,
    #[serde(default)]
    pub(crate) w: f64,
    #[serde(default)]
    pub(crate) h: f64,
}

impl BoxRect {
    fn same_place(&self, other: &BoxRect) -> bool {
        (self.x - other.x).abs() <= STABLE_TOLERANCE_PX
            && (self.y - other.y).abs() <= STABLE_TOLERANCE_PX
            && (self.w - other.w).abs() <= STABLE_TOLERANCE_PX
            && (self.h - other.h).abs() <= STABLE_TOLERANCE_PX
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize)]
pub(crate) struct ViewportSize {
    #[serde(default)]
    pub(crate) w: f64,
    #[serde(default)]
    pub(crate) h: f64,
}

/// What the hit test found at the click point.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub(crate) struct Hit {
    #[serde(default)]
    pub(crate) ok: bool,
    #[serde(default)]
    pub(crate) covered_by_tag: Option<String>,
}

/// What `ACTIONABILITY_PROBE_JS` returned.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub(crate) struct Probe {
    #[serde(default)]
    pub(crate) attached: bool,
    #[serde(default)]
    pub(crate) visible: bool,
    #[serde(default)]
    pub(crate) enabled: bool,
    /// A text control or a content-editable element that is not read-only.
    #[serde(default)]
    pub(crate) editable: bool,
    /// An input or textarea that is not read-only: `editable`, or a control
    /// (date, color, range) that takes a value but no typed text.
    #[serde(default)]
    pub(crate) settable: bool,
    #[serde(default)]
    pub(crate) tag: String,
    #[serde(default, rename = "box")]
    pub(crate) bbox: Option<BoxRect>,
    #[serde(default)]
    pub(crate) viewport: ViewportSize,
    #[serde(default)]
    pub(crate) hit: Option<Hit>,
}

/// Why no click point could be found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PointError {
    /// Every quad is empty or has no area.
    NoBox,
    /// There is a box, but none of it is inside the viewport.
    Unreachable,
}

fn polygon_area(points: &[(f64, f64)]) -> f64 {
    let mut twice = 0.0;
    for (index, (x1, y1)) in points.iter().enumerate() {
        let (x2, y2) = points[(index + 1) % points.len()];
        twice += x1 * y2 - x2 * y1;
    }
    (twice / 2.0).abs()
}

/// Area centroid of a simple polygon; the vertex mean when it has no area.
fn polygon_centroid(points: &[(f64, f64)]) -> (f64, f64) {
    let mut twice_area = 0.0;
    let (mut cx, mut cy) = (0.0, 0.0);
    for (index, (x1, y1)) in points.iter().enumerate() {
        let (x2, y2) = points[(index + 1) % points.len()];
        let cross = x1 * y2 - x2 * y1;
        twice_area += cross;
        cx += (x1 + x2) * cross;
        cy += (y1 + y2) * cross;
    }
    if twice_area.abs() < f64::EPSILON {
        let n = points.len().max(1) as f64;
        return (
            points.iter().map(|p| p.0).sum::<f64>() / n,
            points.iter().map(|p| p.1).sum::<f64>() / n,
        );
    }
    (cx / (3.0 * twice_area), cy / (3.0 * twice_area))
}

/// Sutherland-Hodgman against one half-plane: keep the points where
/// `inside(point)` holds, adding the crossing of every edge that leaves or
/// enters it. `crossing(a, b)` is the point of segment `ab` on the boundary.
fn clip_half_plane(
    polygon: &[(f64, f64)],
    inside: impl Fn((f64, f64)) -> bool,
    crossing: impl Fn((f64, f64), (f64, f64)) -> (f64, f64),
) -> Vec<(f64, f64)> {
    let mut out = Vec::with_capacity(polygon.len() + 2);
    for (index, current) in polygon.iter().enumerate() {
        let previous = polygon[(index + polygon.len() - 1) % polygon.len()];
        match (inside(previous), inside(*current)) {
            (true, true) => out.push(*current),
            (true, false) => out.push(crossing(previous, *current)),
            (false, true) => {
                out.push(crossing(previous, *current));
                out.push(*current);
            }
            (false, false) => {}
        }
    }
    out
}

/// `polygon` cut to the rectangle `0..=width` by `0..=height`.
fn clip_to_viewport(polygon: &[(f64, f64)], width: f64, height: f64) -> Vec<(f64, f64)> {
    let at_x = |(x1, y1): (f64, f64), (x2, y2): (f64, f64), x: f64| {
        let t = (x - x1) / (x2 - x1);
        (x, y1 + t * (y2 - y1))
    };
    let at_y = |(x1, y1): (f64, f64), (x2, y2): (f64, f64), y: f64| {
        let t = (y - y1) / (y2 - y1);
        (x1 + t * (x2 - x1), y)
    };
    let mut points = polygon.to_vec();
    points = clip_half_plane(&points, |p| p.0 >= 0.0, |a, b| at_x(a, b, 0.0));
    if points.len() >= 3 {
        points = clip_half_plane(&points, |p| p.0 <= width, |a, b| at_x(a, b, width));
    }
    if points.len() >= 3 {
        points = clip_half_plane(&points, |p| p.1 >= 0.0, |a, b| at_y(a, b, 0.0));
    }
    if points.len() >= 3 {
        points = clip_half_plane(&points, |p| p.1 <= height, |a, b| at_y(a, b, height));
    }
    points
}

/// The point to click: the centre of the first quad (an inline element can have
/// several) that still has an area once it is cut to the viewport. Each quad is
/// eight numbers, `x1 y1 ... x4 y4`, clockwise, in CSS pixels relative to the
/// viewport (what `DOM.getContentQuads` returns). A rotated box is cut as the
/// parallelogram it is, so the point is always inside what is visible of it.
pub(crate) fn click_point_from_quads(
    quads: &[Vec<f64>],
    viewport_width: f64,
    viewport_height: f64,
) -> Result<(f64, f64), PointError> {
    let mut has_area = false;
    for quad in quads {
        if quad.len() < 8 || quad.iter().take(8).any(|value| !value.is_finite()) {
            continue;
        }
        let polygon: Vec<(f64, f64)> = (0..4).map(|i| (quad[2 * i], quad[2 * i + 1])).collect();
        if polygon_area(&polygon) < MIN_CLICKABLE_AREA {
            continue;
        }
        has_area = true;
        let visible = clip_to_viewport(&polygon, viewport_width, viewport_height);
        if visible.len() >= 3 && polygon_area(&visible) >= MIN_CLICKABLE_AREA {
            return Ok(polygon_centroid(&visible));
        }
    }
    Err(if has_area {
        PointError::Unreachable
    } else {
        PointError::NoBox
    })
}

/// Whether a box has held still: the last two samples agree to within
/// [`STABLE_TOLERANCE_PX`] on every edge. One sample proves nothing.
pub(crate) fn stable(samples: &[BoxRect]) -> bool {
    match samples {
        [.., previous, last] => previous.same_place(last),
        _ => false,
    }
}

/// Apply the checks to a probe. `Ok` means the target can take this action
/// now; otherwise the single most basic thing that is wrong, in the order a
/// person would notice it: gone, not shown, no area, switched off, covered.
pub(crate) fn decide(probe: &Probe, intent: Intent) -> Result<(), Rejection> {
    if !probe.attached {
        return Err(Rejection::of(Reason::NotAttached));
    }
    if !probe.visible {
        return Err(Rejection::of(Reason::NotVisible));
    }
    match probe.bbox {
        Some(rect) if rect.w * rect.h >= MIN_CLICKABLE_AREA => {}
        _ => return Err(Rejection::of(Reason::HasNoBox)),
    }
    let usable = match intent {
        Intent::Click | Intent::Select => probe.enabled,
        Intent::Type => probe.enabled && (probe.editable || probe.settable),
    };
    if !usable {
        return Err(Rejection::of(Reason::Disabled));
    }
    if let Some(hit) = &probe.hit {
        if !hit.ok {
            return Err(match &hit.covered_by_tag {
                Some(tag) => Rejection {
                    reason: Reason::Covered,
                    covered_by_tag: Some(tag.clone()),
                },
                None => Rejection::of(Reason::UnreachableAfterScroll),
            });
        }
    }
    Ok(())
}

/// A tag name as it may leave this crate: lower case letters, digits and
/// hyphens, starting with a letter, at most 32 characters. Anything else (a
/// page that smuggled text into a name is not a tag name) is dropped.
fn sanitize_tag(raw: &str) -> Option<String> {
    let tag = raw.trim().to_ascii_lowercase();
    let valid = !tag.is_empty()
        && tag.len() <= 32
        && tag.starts_with(|c: char| c.is_ascii_lowercase())
        && tag
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    valid.then_some(tag)
}

/// The error for a rejected target: 409 `TARGET_REPAIR_REQUIRED`, a fixed
/// sentence, and `details` with the reason and (covered only) the tag name.
pub(crate) fn into_error(rejection: &Rejection) -> QuarryError {
    let mut details = json!({ "reason": rejection.reason.as_str() });
    if rejection.reason == Reason::Covered {
        if let Some(tag) = rejection.covered_by_tag.as_deref().and_then(sanitize_tag) {
            details["covered_by_tag"] = serde_json::Value::String(tag);
        }
    }
    QuarryError::new(ErrorCode::TargetRepairRequired, rejection.reason.message())
        .with_details(details)
}

/// A step either rejects the target (a verdict about the page) or fails (a
/// driver or protocol problem the caller sees as it is).
enum Fail {
    Reject(Rejection),
    Error(QuarryError),
}

impl From<QuarryError> for Fail {
    fn from(error: QuarryError) -> Self {
        Fail::Error(error)
    }
}

/// A CDP failure as a typed error without its text: the text of a protocol
/// error can quote the page it was about.
fn cdp_failed(what: &'static str, error: &impl std::fmt::Display) -> QuarryError {
    QuarryError::new(ErrorCode::DriverFailed, what)
        .with_details(json!({ "class": cdp_class(error) }))
}

/// Whether Chromium says the node or its object handle is gone. Used only to
/// choose between two reasons; the text never leaves this function.
fn node_is_gone(text: &str) -> bool {
    is_context_destroyed(text)
        || text.contains("Could not find object")
        || text.contains("Cannot find object")
        || text.contains("detached")
        || text.contains("No node with given id")
}

/// A DOM command failed on the target: gone, or (the default) it has no box.
fn dom_failure(error: &impl std::fmt::Display) -> Fail {
    let reason = if node_is_gone(&error.to_string()) {
        Reason::NotAttached
    } else {
        Reason::HasNoBox
    };
    Fail::Reject(Rejection::of(reason))
}

/// A request builder refused its own arguments: a bug here, never page data.
fn build_error(what: &'static str, _builder_message: String) -> QuarryError {
    QuarryError::new(ErrorCode::Internal, what).with_details(json!({ "class": "build" }))
}

async fn probe(
    page: &Page,
    object_id: &RemoteObjectId,
    point: Option<(f64, f64)>,
) -> Result<Probe, Fail> {
    let request = json!({ "point": point.map(|(x, y)| [x, y]) });
    let call = CallFunctionOnParams::builder()
        .function_declaration(ACTIONABILITY_PROBE_JS)
        .object_id(object_id.clone())
        .argument(CallArgument::builder().value(request).build())
        .return_by_value(true)
        .build()
        .map_err(|error| build_error("build actionability probe failed", error))?;
    let result = with_deadline(Op::Actionability, async {
        match page.execute(call).await {
            Ok(result) => Ok(Ok(result)),
            Err(error) if node_is_gone(&error.to_string()) => Ok(Err(error.to_string())),
            Err(error) => Err(cdp_failed("actionability probe failed", &error)),
        }
    })
    .await?;
    let result = match result {
        Ok(result) => result,
        Err(_) => return Err(Fail::Reject(Rejection::of(Reason::NotAttached))),
    };
    if result.exception_details.is_some() {
        return Err(Fail::Error(
            QuarryError::new(
                ErrorCode::DriverFailed,
                "actionability probe raised an exception",
            )
            .with_details(json!({ "class": "page_exception" })),
        ));
    }
    let value = result
        .result
        .result
        .value
        .unwrap_or(serde_json::Value::Null);
    serde_json::from_value::<Probe>(value).map_err(|_| {
        Fail::Error(
            QuarryError::new(
                ErrorCode::DriverFailed,
                "actionability probe returned an unreadable result",
            )
            .with_details(json!({ "class": "decode" })),
        )
    })
}

async fn scroll_into_view(page: &Page, object_id: &RemoteObjectId) -> Result<(), Fail> {
    let params = ScrollIntoViewIfNeededParams::builder()
        .object_id(object_id.clone())
        .build();
    match with_deadline(Op::Actionability, async {
        Ok::<_, QuarryError>(
            page.execute(params)
                .await
                .map_err(|error| dom_failure(&error)),
        )
    })
    .await
    {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(fail)) => Err(fail),
        Err(error) => Err(Fail::Error(error)),
    }
}

async fn content_quads(page: &Page, object_id: &RemoteObjectId) -> Result<Vec<Vec<f64>>, Fail> {
    let params = GetContentQuadsParams::builder()
        .object_id(object_id.clone())
        .build();
    match with_deadline(Op::Actionability, async {
        Ok::<_, QuarryError>(
            page.execute(params)
                .await
                .map_err(|error| dom_failure(&error)),
        )
    })
    .await
    {
        Ok(Ok(result)) => Ok(result
            .result
            .quads
            .iter()
            .map(|quad| quad.inner().clone())
            .collect()),
        Ok(Err(fail)) => Err(fail),
        Err(error) => Err(Fail::Error(error)),
    }
}

/// Sample the box until two consecutive samples agree, for at most
/// `Op::StablePoll`. Returns the last probe (the freshest viewport and box).
async fn wait_stable(page: &Page, object_id: &RemoteObjectId) -> Result<Probe, Fail> {
    let window = deadline(Op::StablePoll);
    let started = Instant::now();
    let mut samples: Vec<BoxRect> = Vec::new();
    loop {
        let current = probe(page, object_id, None).await?;
        decide(&current, Intent::Click).map_err(Fail::Reject)?;
        if let Some(rect) = current.bbox {
            samples.push(rect);
        }
        if stable(&samples) {
            return Ok(current);
        }
        if started.elapsed() + STABLE_STEP >= window {
            return Err(Fail::Reject(Rejection::of(Reason::Unstable)));
        }
        tokio::time::sleep(STABLE_STEP).await;
    }
}

/// A target that passed every check for its intent.
struct Settled {
    /// Where to click (click only).
    point: Option<(f64, f64)>,
    probe: Probe,
}

async fn attempt(page: &Page, object_id: &RemoteObjectId, intent: Intent) -> Result<Settled, Fail> {
    let first = probe(page, object_id, None).await?;
    decide(&first, intent).map_err(Fail::Reject)?;
    scroll_into_view(page, object_id).await?;
    if intent != Intent::Click {
        return Ok(Settled {
            point: None,
            probe: first,
        });
    }
    let steady = wait_stable(page, object_id).await?;
    let quads = content_quads(page, object_id).await?;
    let point =
        click_point_from_quads(&quads, steady.viewport.w, steady.viewport.h).map_err(|error| {
            Fail::Reject(Rejection::of(match error {
                PointError::NoBox => Reason::HasNoBox,
                PointError::Unreachable => Reason::UnreachableAfterScroll,
            }))
        })?;
    let covered = probe(page, object_id, Some(point)).await?;
    decide(&covered, intent).map_err(Fail::Reject)?;
    Ok(Settled {
        point: Some(point),
        probe: covered,
    })
}

/// Poll [`attempt`] until the target is actionable or `Op::Actionability` is
/// spent. A node that left the document is final at once; every other reason
/// can pass (an animation ends, a spinner goes), so the last one seen is what
/// the caller is told when the time runs out.
async fn settle(page: &Page, object_id: &RemoteObjectId, intent: Intent) -> QuarryResult<Settled> {
    let limit = deadline(Op::Actionability);
    let started = Instant::now();
    loop {
        match attempt(page, object_id, intent).await {
            Ok(settled) => return Ok(settled),
            Err(Fail::Error(error)) => return Err(error),
            Err(Fail::Reject(rejection)) => {
                if rejection.reason == Reason::NotAttached || started.elapsed() + POLL >= limit {
                    // The rate per reason, for the soak before the flag goes on.
                    tracing::info!(
                        reason = rejection.reason.as_str(),
                        intent = intent.label(),
                        "quarry.browser.actionability_rejected"
                    );
                    return Err(into_error(&rejection));
                }
                tokio::time::sleep(POLL).await;
            }
        }
    }
}

/// Wait until `target` can be clicked and return where to click.
pub(crate) async fn prepare_click(
    page: &Page,
    object_id: &RemoteObjectId,
) -> QuarryResult<(f64, f64)> {
    let settled = settle(page, object_id, Intent::Click).await?;
    settled.point.ok_or_else(|| {
        QuarryError::new(ErrorCode::Internal, "actionability produced no click point")
    })
}

/// How a typed value reaches the field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TypeRoute {
    /// Focus, select all, `Input.insertText`: trusted `input` events.
    InsertText,
    /// A settable control that is not a text field: the in-page setter.
    SetValue,
}

/// Wait until the field can take text and say how to give it.
pub(crate) async fn prepare_type(
    page: &Page,
    object_id: &RemoteObjectId,
) -> QuarryResult<TypeRoute> {
    let settled = settle(page, object_id, Intent::Type).await?;
    Ok(if settled.probe.editable {
        TypeRoute::InsertText
    } else {
        TypeRoute::SetValue
    })
}

/// Wait until the `<select>` is visible and enabled.
pub(crate) async fn prepare_select(page: &Page, object_id: &RemoteObjectId) -> QuarryResult<()> {
    settle(page, object_id, Intent::Select).await.map(|_| ())
}

/// The three mouse events of a left click at `(x, y)`: move, press, release.
/// Chromium treats them as real input (`isTrusted`). Shared by the
/// `click_point` trait method and the ref pipeline.
pub(crate) async fn dispatch_click(page: &Page, x: f64, y: f64) -> QuarryResult<()> {
    let base = DispatchMouseEventParams::builder()
        .x(x)
        .y(y)
        .button(MouseButton::Left)
        .click_count(1)
        .pointer_type(DispatchMouseEventPointerType::Mouse);

    let moved = base
        .clone()
        .r#type(DispatchMouseEventType::MouseMoved)
        .buttons(0)
        .build()
        .map_err(|e| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                format!("click_point build move failed: {e}"),
            )
        })?;
    let pressed = base
        .clone()
        .r#type(DispatchMouseEventType::MousePressed)
        .buttons(1)
        .build()
        .map_err(|e| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                format!("click_point build press failed: {e}"),
            )
        })?;
    let released = base
        .r#type(DispatchMouseEventType::MouseReleased)
        .buttons(0)
        .build()
        .map_err(|e| {
            QuarryError::new(
                ErrorCode::DriverFailed,
                format!("click_point build release failed: {e}"),
            )
        })?;

    with_deadline(Op::Input, async {
        page.execute(moved).await.map_err(|e| {
            QuarryError::new(ErrorCode::DriverFailed, "click_point move failed")
                .with_details(json!({ "x": x, "y": y, "error": e.to_string() }))
        })
    })
    .await?;
    with_deadline(Op::Input, async {
        page.execute(pressed).await.map_err(|e| {
            QuarryError::new(ErrorCode::DriverFailed, "click_point press failed")
                .with_details(json!({ "x": x, "y": y, "error": e.to_string() }))
        })
    })
    .await?;
    with_deadline(Op::Input, async {
        page.execute(released).await.map_err(|e| {
            QuarryError::new(ErrorCode::DriverFailed, "click_point release failed")
                .with_details(json!({ "x": x, "y": y, "error": e.to_string() }))
        })
    })
    .await?;
    Ok(())
}

/// The driver's list of open JavaScript dialogs for one session. A click that
/// opens an `alert` does not return until the dialog is answered (the page's
/// main thread is blocked inside the handler), so the click is watched against
/// this list and counts as delivered once a dialog that was not there before it
/// appears. The dialog stays pending for `respond_dialog`.
#[derive(Clone)]
pub(crate) struct DialogWatch {
    pub(crate) dialogs: Arc<Mutex<HashMap<String, Vec<BrowserDialog>>>>,
    pub(crate) session_key: String,
}

impl DialogWatch {
    async fn open_ids(&self) -> Vec<String> {
        self.dialogs
            .lock()
            .await
            .get(&self.session_key)
            .map(|open| open.iter().map(|dialog| dialog.dialog_id.clone()).collect())
            .unwrap_or_default()
    }

    async fn opened_since(&self, before: &[String]) -> bool {
        self.dialogs
            .lock()
            .await
            .get(&self.session_key)
            .is_some_and(|open| {
                open.iter()
                    .any(|dialog| !before.contains(&dialog.dialog_id))
            })
    }
}

async fn click_watching_dialogs(
    page: &Page,
    x: f64,
    y: f64,
    watch: &DialogWatch,
) -> QuarryResult<()> {
    let before = watch.open_ids().await;
    let dispatch = dispatch_click(page, x, y);
    tokio::pin!(dispatch);
    let mut tick = tokio::time::interval(Duration::from_millis(25));
    loop {
        tokio::select! {
            biased;
            result = &mut dispatch => return result,
            _ = tick.tick() => {
                if watch.opened_since(&before).await {
                    tracing::debug!("quarry.browser.click_returned_with_dialog_pending");
                    return Ok(());
                }
            }
        }
    }
}

/// Run one in-page function on the target and turn `{ ok: false, reason }`
/// into the legacy `TARGET_REPAIR_REQUIRED` the synthetic path always raised.
async fn call_in_page(
    page: &Page,
    object_id: &RemoteObjectId,
    function: &'static str,
    argument: serde_json::Value,
) -> QuarryResult<()> {
    let call = CallFunctionOnParams::builder()
        .function_declaration(function)
        .object_id(object_id.clone())
        .argument(CallArgument::builder().value(argument).build())
        .return_by_value(true)
        .user_gesture(true)
        .build()
        .map_err(|error| build_error("build native target action failed", error))?;
    let result = with_deadline(Op::Input, async {
        page.execute(call)
            .await
            .map_err(|error| cdp_failed("native target action failed", &error))
    })
    .await?;
    if result.exception_details.is_some() {
        return Err(QuarryError::new(
            ErrorCode::TargetRepairRequired,
            "native target action raised a page exception; observe again",
        ));
    }
    let value = result
        .result
        .result
        .value
        .unwrap_or(serde_json::Value::Null);
    if value.get("ok").and_then(serde_json::Value::as_bool) == Some(true) {
        Ok(())
    } else {
        Err(QuarryError::new(
            ErrorCode::TargetRepairRequired,
            "native accessibility target changed or rejected the action",
        )
        .with_details(value))
    }
}

/// Replace a text control's content with `text` as input a person typed or
/// pasted: focus it, select what is in it, then `Input.insertText` (an empty
/// text deletes the selection instead).
pub(crate) async fn insert_text(
    page: &Page,
    object_id: &RemoteObjectId,
    text: &str,
) -> QuarryResult<()> {
    let focus = FocusParams::builder().object_id(object_id.clone()).build();
    with_deadline(Op::Input, async {
        page.execute(focus)
            .await
            .map_err(|error| cdp_failed("native target focus failed", &error))
    })
    .await?;
    call_in_page(
        page,
        object_id,
        SELECT_ALL_JS,
        json!({ "clear": text.is_empty() }),
    )
    .await?;
    if text.is_empty() {
        return Ok(());
    }
    with_deadline(Op::Input, async {
        page.execute(InsertTextParams::new(text))
            .await
            .map(|_| ())
            .map_err(|error| cdp_failed("native target insert text failed", &error))
    })
    .await
}

/// Whether the pipeline handles this operation. `wait_for` has nothing to
/// click or type: the target resolving at all is its answer.
pub(crate) fn handles(operation: &VerifiedTargetOperation) -> bool {
    matches!(
        operation,
        VerifiedTargetOperation::Click
            | VerifiedTargetOperation::Type { .. }
            | VerifiedTargetOperation::Select { .. }
    )
}

/// Run `operation` on the resolved element through the pipeline.
pub(crate) async fn run(
    page: &Page,
    object_id: &RemoteObjectId,
    operation: &VerifiedTargetOperation,
    watch: &DialogWatch,
) -> QuarryResult<()> {
    match operation {
        VerifiedTargetOperation::Click => {
            let (x, y) = prepare_click(page, object_id).await?;
            click_watching_dialogs(page, x, y, watch).await
        }
        VerifiedTargetOperation::Type { text } => match prepare_type(page, object_id).await? {
            TypeRoute::InsertText => insert_text(page, object_id, text).await,
            TypeRoute::SetValue => {
                call_in_page(page, object_id, SET_VALUE_JS, json!({ "text": text })).await
            }
        },
        VerifiedTargetOperation::Select { value } => {
            prepare_select(page, object_id).await?;
            call_in_page(page, object_id, SELECT_OPTION_JS, json!({ "value": value })).await
        }
        VerifiedTargetOperation::WaitFor { .. } => Err(QuarryError::new(
            ErrorCode::Internal,
            "wait_for does not use the actionability pipeline",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Vec<f64> {
        vec![x, y, x + w, y, x + w, y + h, x, y + h]
    }

    fn near(actual: (f64, f64), expected: (f64, f64)) {
        assert!(
            (actual.0 - expected.0).abs() < 1e-6 && (actual.1 - expected.1).abs() < 1e-6,
            "{actual:?} != {expected:?}"
        );
    }

    #[test]
    fn click_point_from_quads_handles_rect_rotated_offscreen_and_zero_area() {
        // A plain rectangle: its centre.
        near(
            click_point_from_quads(&[rect(100.0, 50.0, 80.0, 20.0)], 800.0, 600.0).unwrap(),
            (140.0, 60.0),
        );
        // A square turned 45 degrees (diamond), centre (200, 200).
        let diamond = vec![200.0, 150.0, 250.0, 200.0, 200.0, 250.0, 150.0, 200.0];
        near(
            click_point_from_quads(&[diamond], 800.0, 600.0).unwrap(),
            (200.0, 200.0),
        );
        // Half off the left edge: the centre of what is visible, not of the box.
        near(
            click_point_from_quads(&[rect(-60.0, 10.0, 100.0, 20.0)], 800.0, 600.0).unwrap(),
            (20.0, 20.0),
        );
        // Cut by the bottom edge as well.
        near(
            click_point_from_quads(&[rect(700.0, 580.0, 200.0, 100.0)], 800.0, 600.0).unwrap(),
            (750.0, 590.0),
        );
        // A rotated box cut by the viewport keeps the point inside the visible part.
        let turned = vec![-20.0, 100.0, 40.0, 40.0, 100.0, 100.0, 40.0, 160.0];
        let point = click_point_from_quads(&[turned], 800.0, 600.0).unwrap();
        assert!(point.0 >= 0.0 && point.0 <= 100.0 && point.1 > 40.0 && point.1 < 160.0);
        // Entirely outside the viewport: there is a box, it is out of reach.
        assert_eq!(
            click_point_from_quads(&[rect(900.0, 10.0, 50.0, 50.0)], 800.0, 600.0),
            Err(PointError::Unreachable)
        );
        assert_eq!(
            click_point_from_quads(&[rect(10.0, -200.0, 50.0, 50.0)], 800.0, 600.0),
            Err(PointError::Unreachable)
        );
        // No quads, a degenerate quad (a line, a point), a sub-pixel box, a short
        // or non-finite quad: no area to point at.
        assert_eq!(
            click_point_from_quads(&[], 800.0, 600.0),
            Err(PointError::NoBox)
        );
        assert_eq!(
            click_point_from_quads(&[rect(10.0, 10.0, 0.0, 0.0)], 800.0, 600.0),
            Err(PointError::NoBox)
        );
        assert_eq!(
            click_point_from_quads(&[rect(10.0, 10.0, 100.0, 0.0)], 800.0, 600.0),
            Err(PointError::NoBox)
        );
        assert_eq!(
            click_point_from_quads(&[rect(10.0, 10.0, 0.5, 0.5)], 800.0, 600.0),
            Err(PointError::NoBox)
        );
        assert_eq!(
            click_point_from_quads(&[vec![1.0, 2.0, 3.0]], 800.0, 600.0),
            Err(PointError::NoBox)
        );
        assert_eq!(
            click_point_from_quads(&[vec![f64::NAN; 8]], 800.0, 600.0),
            Err(PointError::NoBox)
        );
        // An inline element wrapped over two lines: the first quad with an area
        // inside the viewport wins; an empty first quad is skipped.
        near(
            click_point_from_quads(
                &[
                    rect(5.0, 5.0, 0.0, 0.0),
                    rect(10.0, 100.0, 40.0, 10.0),
                    rect(10.0, 120.0, 40.0, 10.0),
                ],
                800.0,
                600.0,
            )
            .unwrap(),
            (30.0, 105.0),
        );
        // An unknown viewport reaches nothing.
        assert_eq!(
            click_point_from_quads(&[rect(10.0, 10.0, 50.0, 50.0)], 0.0, 0.0),
            Err(PointError::Unreachable)
        );
    }

    #[test]
    fn stable_requires_two_samples_within_half_a_pixel() {
        let at = |x: f64| BoxRect {
            x,
            y: 10.0,
            w: 80.0,
            h: 20.0,
        };
        assert!(!stable(&[]));
        assert!(!stable(&[at(10.0)]), "one sample proves nothing");
        assert!(stable(&[at(10.0), at(10.0)]));
        assert!(stable(&[at(10.0), at(10.5)]), "half a pixel is still");
        assert!(!stable(&[at(10.0), at(10.6)]));
        // Only the last two count: it moved, then stopped.
        assert!(stable(&[at(10.0), at(40.0), at(70.0), at(70.2)]));
        assert!(!stable(&[at(10.0), at(10.0), at(70.0)]));
        // Size counts as much as position.
        let grown = BoxRect {
            h: 21.0,
            ..at(10.0)
        };
        assert!(!stable(&[at(10.0), grown]));
    }

    fn ready() -> Probe {
        Probe {
            attached: true,
            visible: true,
            enabled: true,
            editable: true,
            settable: true,
            tag: "button".into(),
            bbox: Some(BoxRect {
                x: 10.0,
                y: 10.0,
                w: 80.0,
                h: 20.0,
            }),
            viewport: ViewportSize { w: 800.0, h: 600.0 },
            hit: None,
        }
    }

    fn reason_of(probe: &Probe, intent: Intent) -> Option<Reason> {
        decide(probe, intent)
            .err()
            .map(|rejection| rejection.reason)
    }

    #[test]
    fn decide_maps_each_failure_to_one_reason() {
        assert_eq!(decide(&ready(), Intent::Click), Ok(()));
        assert_eq!(decide(&ready(), Intent::Type), Ok(()));
        assert_eq!(decide(&ready(), Intent::Select), Ok(()));

        let mut gone = ready();
        gone.attached = false;
        gone.visible = false; // a detached node is also "not visible": gone wins
        assert_eq!(reason_of(&gone, Intent::Click), Some(Reason::NotAttached));

        let mut hidden = ready();
        hidden.visible = false;
        assert_eq!(reason_of(&hidden, Intent::Click), Some(Reason::NotVisible));

        let mut flat = ready();
        flat.bbox = Some(BoxRect::default());
        assert_eq!(reason_of(&flat, Intent::Click), Some(Reason::HasNoBox));
        flat.bbox = None;
        assert_eq!(reason_of(&flat, Intent::Click), Some(Reason::HasNoBox));

        let mut off = ready();
        off.enabled = false;
        for intent in [Intent::Click, Intent::Type, Intent::Select] {
            assert_eq!(reason_of(&off, intent), Some(Reason::Disabled));
        }

        // Typing needs a field that takes text; clicking does not.
        let mut readonly = ready();
        readonly.editable = false;
        readonly.settable = false;
        assert_eq!(reason_of(&readonly, Intent::Type), Some(Reason::Disabled));
        assert_eq!(decide(&readonly, Intent::Click), Ok(()));
        // A date or colour input is settable but not editable: it can be typed into.
        let mut date = ready();
        date.editable = false;
        assert_eq!(decide(&date, Intent::Type), Ok(()));

        // The hit test: covered carries the tag, no element at the point is unreachable.
        let mut covered = ready();
        covered.hit = Some(Hit {
            ok: false,
            covered_by_tag: Some("div".into()),
        });
        let rejection = decide(&covered, Intent::Click).unwrap_err();
        assert_eq!(rejection.reason, Reason::Covered);
        assert_eq!(rejection.covered_by_tag.as_deref(), Some("div"));
        let mut nothing = ready();
        nothing.hit = Some(Hit {
            ok: false,
            covered_by_tag: None,
        });
        assert_eq!(
            reason_of(&nothing, Intent::Click),
            Some(Reason::UnreachableAfterScroll)
        );
        let mut fine = ready();
        fine.hit = Some(Hit {
            ok: true,
            covered_by_tag: None,
        });
        assert_eq!(decide(&fine, Intent::Click), Ok(()));

        // Order: the more basic fault is the one reported.
        let mut several = covered.clone();
        several.enabled = false;
        several.visible = false;
        assert_eq!(reason_of(&several, Intent::Click), Some(Reason::NotVisible));

        // Every reason the pipeline can produce is on the closed list, once.
        let produced = [
            Reason::NotAttached,
            Reason::NotVisible,
            Reason::Disabled,
            Reason::Covered,
            Reason::Unstable,
            Reason::HasNoBox,
            Reason::UnreachableAfterScroll,
        ]
        .map(Reason::as_str);
        assert_eq!(produced, reasons::ALL);
    }

    #[test]
    fn into_error_is_409_with_reason_and_never_carries_page_text() {
        let covered = into_error(&Rejection {
            reason: Reason::Covered,
            covered_by_tag: Some("dialog".into()),
        });
        assert_eq!(covered.code, ErrorCode::TargetRepairRequired);
        assert_eq!(covered.code.http_status(), 409);
        assert!(!covered.retryable);
        assert_eq!(
            covered.details.as_ref().unwrap(),
            &json!({ "reason": "target_covered", "covered_by_tag": "dialog" })
        );
        assert_eq!(covered.target_repair_reason(), Some("target_covered"));

        // A custom element name is a tag name; text that is not one is dropped.
        let custom = into_error(&Rejection {
            reason: Reason::Covered,
            covered_by_tag: Some("My-Overlay".into()),
        });
        assert_eq!(custom.details.unwrap()["covered_by_tag"], "my-overlay");
        for smuggled in [
            "Accept all cookies and continue to https://example.test/consent",
            "a".repeat(33).as_str(),
            "9lives",
            "",
            "div onclick=1",
            "<script>",
        ] {
            let error = into_error(&Rejection {
                reason: Reason::Covered,
                covered_by_tag: Some(smuggled.to_owned()),
            });
            let details = error.details.unwrap();
            assert!(details.get("covered_by_tag").is_none(), "{smuggled:?}");
            assert_eq!(details, json!({ "reason": "target_covered" }));
        }

        // Only a covered target has a tag; every reason has a fixed, text-free sentence.
        for reason in [
            Reason::NotAttached,
            Reason::NotVisible,
            Reason::Disabled,
            Reason::Unstable,
            Reason::HasNoBox,
            Reason::UnreachableAfterScroll,
        ] {
            let error = into_error(&Rejection {
                reason,
                covered_by_tag: Some("div".into()),
            });
            assert_eq!(error.code, ErrorCode::TargetRepairRequired);
            assert_eq!(error.details.unwrap(), json!({ "reason": reason.as_str() }));
            assert!(!error.message.contains("http"), "{}", error.message);
            assert!(
                error.message.ends_with("observe again"),
                "{}",
                error.message
            );
        }
    }

    #[test]
    fn trusted_input_flag_parses_on_and_off_and_ignores_garbage() {
        assert!(!parse_trusted_input(None));
        assert!(!parse_trusted_input(Some("")));
        assert!(!parse_trusted_input(Some("  ")));
        assert!(!parse_trusted_input(Some("off")));
        assert!(parse_trusted_input(Some("on")));
        assert!(parse_trusted_input(Some(" ON ")));
        assert!(parse_trusted_input(Some("true")));
        assert!(parse_trusted_input(Some("1")));
        for bad in ["yes", "enabled", "2", "onn"] {
            assert!(!parse_trusted_input(Some(bad)), "{bad:?}");
        }
    }

    #[test]
    fn the_probe_script_returns_no_page_text() {
        // The probe reads state and a tag name; it must not read text, values,
        // attributes or URLs, because its result is parsed into typed fields
        // and any of them could end up in an error.
        for forbidden in [
            "textContent",
            "innerText",
            "innerHTML",
            "outerHTML",
            ".value",
            "getAttribute",
            ".href",
            ".src",
            "location",
        ] {
            assert!(
                !ACTIONABILITY_PROBE_JS.contains(forbidden),
                "probe mentions {forbidden}"
            );
        }
        // And it is a function declaration taking the request, as callFunctionOn needs.
        assert!(ACTIONABILITY_PROBE_JS.starts_with("function(request)"));
    }
}
