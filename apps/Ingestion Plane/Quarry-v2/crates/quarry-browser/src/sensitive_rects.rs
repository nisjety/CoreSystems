//! WS12.S5: where the sensitive fields of a page are on the screen.
//!
//! One read-only script walks every editable control of the page (open shadow
//! roots and same-origin iframes included, with the frame offsets added up) and
//! hands back facts and rectangles. The Rust classifier
//! (`quarry_core::redaction::field::classify`) is the authority on which of them
//! are sensitive: the script decides nothing.
//!
//! The script never reads a value out of the page except for two things: a flag
//! that says whether a control holds anything, and (for a control that is not a
//! password field) at most 40 characters, which the classifier needs to spot a
//! card number. Neither is logged or stored: `ScanField` has no `Debug` that
//! prints a value, and only counts and rectangles leave this module.
//!
//! Rules (plan WS12.S5):
//! - a sensitive field that is rendered and has a usable rectangle is **masked**;
//! - a sensitive field that is rendered but has no usable rectangle (a zero-size
//!   box, a probe error, a page cut off by the node cap) is **unresolved**, and
//!   the screenshot is withheld;
//! - a field that is not rendered (`display:none`, `visibility:hidden`,
//!   `type=hidden`) is ignored;
//! - a cross-origin iframe cannot be looked into: it is counted (`oopif_frames`)
//!   and its rectangle is kept so strict mode can blank it.

// Without the Chromium feature nothing calls the live scan, so the pure parts are
// exercised only by the unit tests.
#![cfg_attr(not(feature = "chromiumoxide"), allow(dead_code))]

use std::fmt;
use std::time::Duration;

use quarry_core::redaction::field::{classify, FieldFacts};
use serde::Deserialize;
use serde_json::Value;

/// Wall-clock budget of one scan (the script plus the CDP round trip).
pub const SCAN_BUDGET: Duration = Duration::from_millis(1500);

/// A rectangle in CSS pixels of the top frame's viewport (client coordinates).
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct CssRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl CssRect {
    fn is_usable(&self) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && self.w.is_finite()
            && self.h.is_finite()
            && self.w > 0.0
            && self.h > 0.0
    }
}

/// What the scan found, after classification.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RectScan {
    /// Sensitive fields that are rendered and have a usable rectangle.
    pub rects: Vec<CssRect>,
    /// Sensitive fields that are rendered but have no usable rectangle, plus one
    /// when the walk was cut short by the node cap.
    pub unresolved_visible: u32,
    /// Rendered iframes whose document the script could not read (cross-origin).
    pub oopif_frames: u32,
    /// The rectangles of those iframes, for strict mode.
    pub oopif_rects: Vec<CssRect>,
    /// Sensitive rendered fields that hold a non-empty value (the PDF rule).
    pub sensitive_with_value: u32,
    /// The top window's scroll offset when the scan ran.
    pub scroll_x: f64,
    pub scroll_y: f64,
    /// `window.devicePixelRatio` when the scan ran.
    pub dpr: f64,
    /// The top window's inner size in CSS pixels.
    pub viewport_w: f64,
    pub viewport_h: f64,
}

impl RectScan {
    /// Nothing sensitive was seen and nothing is unresolved.
    pub fn is_clean(&self) -> bool {
        self.rects.is_empty() && self.unresolved_visible == 0
    }
}

/// One control as the script reports it.
#[derive(Deserialize)]
#[serde(default)]
struct ScanField {
    facts: FieldFacts,
    has_value: bool,
    rect: Option<CssRect>,
}

impl Default for ScanField {
    fn default() -> Self {
        Self {
            facts: FieldFacts::default(),
            has_value: false,
            rect: None,
        }
    }
}

impl fmt::Debug for ScanField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScanField")
            .field("has_value", &self.has_value)
            .field("rect", &self.rect)
            .finish()
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawScan {
    ok: bool,
    truncated: bool,
    scroll: [f64; 2],
    viewport: [f64; 2],
    dpr: f64,
    fields: Vec<ScanField>,
    oopif: Vec<CssRect>,
}

/// Read the script's answer. `None` when it is not the shape the script returns
/// (the caller treats that as a failed scan and withholds).
pub fn parse_scan(value: &Value) -> Option<RectScan> {
    let raw: RawScan = serde_json::from_value(value.clone()).ok()?;
    if !raw.ok || !(raw.dpr.is_finite() && raw.dpr > 0.0) {
        return None;
    }
    let mut scan = RectScan {
        scroll_x: raw.scroll[0],
        scroll_y: raw.scroll[1],
        dpr: raw.dpr,
        viewport_w: raw.viewport[0],
        viewport_h: raw.viewport[1],
        ..RectScan::default()
    };
    for field in raw.fields {
        if !classify(&field.facts).kind.is_sensitive() {
            continue;
        }
        if field.has_value {
            scan.sensitive_with_value = scan.sensitive_with_value.saturating_add(1);
        }
        match field.rect {
            Some(rect) if rect.is_usable() => scan.rects.push(rect),
            _ => scan.unresolved_visible = scan.unresolved_visible.saturating_add(1),
        }
    }
    for frame in raw.oopif {
        scan.oopif_frames = scan.oopif_frames.saturating_add(1);
        if frame.is_usable() {
            scan.oopif_rects.push(frame);
        }
    }
    if raw.truncated {
        // The walk stopped early: there may be sensitive fields it never saw.
        scan.unresolved_visible = scan.unresolved_visible.saturating_add(1);
    }
    Some(scan)
}

/// Do two scroll offsets name the same position (half a CSS pixel apart at most)?
pub fn same_scroll(a: (f64, f64), b: (f64, f64)) -> bool {
    (a.0 - b.0).abs() <= 0.5 && (a.1 - b.1).abs() <= 0.5
}

/// Read the scroll answer of [`SCROLL_SCRIPT`].
pub fn parse_scroll(value: &Value) -> Option<(f64, f64)> {
    let pair = value.as_array()?;
    let x = pair.first()?.as_f64()?;
    let y = pair.get(1)?.as_f64()?;
    (x.is_finite() && y.is_finite()).then_some((x, y))
}

/// The scroll position, read again after the capture to see whether the page
/// moved while the screenshot was taken.
pub const SCROLL_SCRIPT: &str = "[window.scrollX, window.scrollY]";

/// The walk. Read-only: it reads attributes, layout and (bounded) values, writes
/// nothing, and returns plain data. Caps: 20,000 nodes, 512 fields, 8 nested
/// documents; past a cap `truncated` is set and the scan counts as unresolved.
pub const SCAN_SCRIPT: &str = r#"(() => {
  const MAX_NODES = 20000, MAX_FIELDS = 512, MAX_DEPTH = 8, TEXT = 200, VALUE = 40;
  const out = { ok: true, truncated: false, fields: [], oopif: [] };
  let nodes = 0;
  const attr = (el, name) => {
    const v = el.getAttribute(name);
    return v === null ? null : String(v).slice(0, TEXT);
  };
  const rendered = (el, win) => {
    try {
      if (el.getClientRects().length === 0) return false;
      const cs = win.getComputedStyle(el);
      return cs.display !== 'none' && cs.visibility !== 'hidden';
    } catch (e) { return true; }
  };
  const labelText = (el) => {
    const parts = [];
    try {
      if (el.labels) for (const l of el.labels) parts.push(l.textContent || '');
      const by = el.getAttribute('aria-labelledby');
      if (by) {
        const root = el.getRootNode();
        for (const id of by.split(/\s+/)) {
          const n = root && root.getElementById ? root.getElementById(id) : null;
          if (n) parts.push(n.textContent || '');
        }
      }
    } catch (e) {}
    const text = parts.join(' ').slice(0, TEXT);
    return text === '' ? null : text;
  };
  const addField = (el, win, ox, oy) => {
    if (out.fields.length >= MAX_FIELDS) { out.truncated = true; return; }
    const tag = el.tagName;
    const isFormControl = tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT';
    if (!isFormControl && !(el.isContentEditable && el.getAttribute('contenteditable') !== null)) return;
    const type = tag === 'INPUT' ? (attr(el, 'type') || '').trim().toLowerCase() : '';
    if (type === 'hidden') return;
    if (!rendered(el, win)) return;
    let value = null, hasValue = false;
    try {
      if (isFormControl) {
        const v = el.value;
        hasValue = typeof v === 'string' && v.length > 0;
        if (hasValue && type !== 'password') value = v.slice(0, VALUE);
      } else {
        const t = (el.textContent || '').trim();
        hasValue = t.length > 0;
        if (hasValue) value = t.slice(0, VALUE);
      }
    } catch (e) { hasValue = true; }
    let rect = null;
    try {
      const r = el.getBoundingClientRect();
      if ([r.left, r.top, r.width, r.height].every(Number.isFinite) && r.width > 0 && r.height > 0) {
        rect = { x: ox + r.left, y: oy + r.top, w: r.width, h: r.height };
      }
    } catch (e) {}
    out.fields.push({
      facts: {
        input_type: tag === 'INPUT' ? attr(el, 'type') : null,
        autocomplete: attr(el, 'autocomplete'),
        name_attr: attr(el, 'name'),
        id_attr: attr(el, 'id'),
        aria_label: attr(el, 'aria-label'),
        placeholder: attr(el, 'placeholder'),
        accessible_name: labelText(el),
        value: value
      },
      has_value: hasValue,
      rect: rect
    });
  };
  const addFrame = (el, win, ox, oy, depth) => {
    if (!rendered(el, win)) return;
    const r = el.getBoundingClientRect();
    let doc = null, inner = null;
    try { doc = el.contentDocument; inner = el.contentWindow; } catch (e) { doc = null; }
    if (!doc || !inner) {
      out.oopif.push({ x: ox + r.left, y: oy + r.top, w: r.width, h: r.height });
      return;
    }
    if (!(r.width > 0 && r.height > 0)) return;
    const cs = win.getComputedStyle(el);
    const dx = (parseFloat(cs.borderLeftWidth) || 0) + (parseFloat(cs.paddingLeft) || 0);
    const dy = (parseFloat(cs.borderTopWidth) || 0) + (parseFloat(cs.paddingTop) || 0);
    walk(doc, inner, ox + r.left + dx, oy + r.top + dy, depth + 1);
  };
  const walk = (root, win, ox, oy, depth) => {
    if (depth > MAX_DEPTH) { out.truncated = true; return; }
    let all;
    try { all = root.querySelectorAll('*'); } catch (e) { out.truncated = true; return; }
    for (const el of all) {
      if (++nodes > MAX_NODES) { out.truncated = true; return; }
      const tag = el.tagName;
      if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || el.hasAttribute('contenteditable')) {
        addField(el, win, ox, oy);
      }
      if (el.shadowRoot) walk(el.shadowRoot, win, ox, oy, depth + 1);
      if (tag === 'IFRAME' || tag === 'FRAME') addFrame(el, win, ox, oy, depth);
    }
  };
  try {
    walk(document, window, 0, 0, 0);
  } catch (e) { out.ok = false; }
  out.scroll = [window.scrollX, window.scrollY];
  out.viewport = [window.innerWidth, window.innerHeight];
  out.dpr = window.devicePixelRatio;
  return out;
})()"#;

/// Run the scan against a live page. `None` is a failed scan (the page did not
/// answer in time, the script threw, or the answer was not the expected shape):
/// the caller withholds. The result is never logged.
#[cfg(feature = "chromiumoxide")]
pub async fn scan_page(page: &::chromiumoxide::Page) -> Option<RectScan> {
    let evaluated = tokio::time::timeout(SCAN_BUDGET, page.evaluate(SCAN_SCRIPT))
        .await
        .ok()?
        .ok()?;
    let value: Value = evaluated.into_value().ok()?;
    parse_scan(&value)
}

/// The scroll position now. `None` when the page does not answer in time.
#[cfg(feature = "chromiumoxide")]
pub async fn scroll_now(page: &::chromiumoxide::Page) -> Option<(f64, f64)> {
    let evaluated = tokio::time::timeout(SCAN_BUDGET, page.evaluate(SCROLL_SCRIPT))
        .await
        .ok()?
        .ok()?;
    let value: Value = evaluated.into_value().ok()?;
    parse_scroll(&value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn field(input_type: &str, name: &str, rect: Option<Value>, has_value: bool) -> Value {
        json!({
            "facts": { "input_type": input_type, "name_attr": name },
            "has_value": has_value,
            "rect": rect,
        })
    }

    fn scan_of(fields: Vec<Value>, oopif: Vec<Value>, truncated: bool) -> Value {
        json!({
            "ok": true, "truncated": truncated, "scroll": [0.0, 120.0],
            "viewport": [1280.0, 800.0], "dpr": 2.0,
            "fields": fields, "oopif": oopif,
        })
    }

    #[test]
    fn a_password_field_with_a_rectangle_is_a_rect_and_a_plain_field_is_not() {
        let rect = json!({ "x": 10.0, "y": 20.0, "w": 200.0, "h": 30.0 });
        let value = scan_of(
            vec![
                field("password", "pw", Some(rect.clone()), true),
                field("text", "city", Some(rect), true),
            ],
            vec![],
            false,
        );
        let scan = parse_scan(&value).expect("a scan");
        assert_eq!(scan.rects.len(), 1);
        assert_eq!(scan.unresolved_visible, 0);
        assert_eq!(scan.sensitive_with_value, 1);
        assert_eq!((scan.scroll_x, scan.scroll_y, scan.dpr), (0.0, 120.0, 2.0));
        assert!(!scan.is_clean());
    }

    #[test]
    fn a_sensitive_field_without_a_usable_rectangle_is_unresolved() {
        let zero = json!({ "x": 0.0, "y": 0.0, "w": 0.0, "h": 12.0 });
        let value = scan_of(
            vec![
                field("password", "a", None, false),
                field("password", "b", Some(zero), false),
            ],
            vec![],
            false,
        );
        let scan = parse_scan(&value).expect("a scan");
        assert!(scan.rects.is_empty());
        assert_eq!(scan.unresolved_visible, 2);
    }

    #[test]
    fn a_walk_cut_short_is_unresolved() {
        let scan = parse_scan(&scan_of(vec![], vec![], true)).expect("a scan");
        assert_eq!(scan.unresolved_visible, 1);
    }

    #[test]
    fn cross_origin_frames_are_counted_and_their_rectangles_kept() {
        let frame = json!({ "x": 5.0, "y": 6.0, "w": 300.0, "h": 150.0 });
        let empty = json!({ "x": 5.0, "y": 6.0, "w": 0.0, "h": 0.0 });
        let scan = parse_scan(&scan_of(vec![], vec![frame, empty], false)).expect("a scan");
        assert_eq!(scan.oopif_frames, 2);
        assert_eq!(scan.oopif_rects.len(), 1);
        assert!(
            scan.is_clean(),
            "an iframe alone does not unresolve the scan"
        );
    }

    #[test]
    fn a_failed_or_malformed_answer_is_no_scan() {
        assert!(parse_scan(&json!({ "ok": false, "dpr": 1.0 })).is_none());
        assert!(parse_scan(&json!({ "ok": true, "dpr": 0.0 })).is_none());
        assert!(parse_scan(&json!("not an object")).is_none());
        assert!(parse_scan(&Value::Null).is_none());
    }

    #[test]
    fn scroll_comparison_allows_half_a_pixel() {
        assert!(same_scroll((0.0, 100.0), (0.4, 100.4)));
        assert!(!same_scroll((0.0, 100.0), (0.0, 101.0)));
        assert_eq!(parse_scroll(&json!([3.0, 4.5])), Some((3.0, 4.5)));
        assert_eq!(parse_scroll(&json!([3.0])), None);
    }

    #[test]
    fn the_debug_of_a_scan_field_never_prints_a_value() {
        let mut f = ScanField::default();
        f.facts.value = Some("S3cretP@ss".to_owned());
        let text = format!("{f:?}");
        assert!(!text.contains("S3cretP@ss"), "{text}");
    }
}
