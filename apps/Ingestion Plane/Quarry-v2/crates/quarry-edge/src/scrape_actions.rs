//! WS13.4b: scrape-time captures (`screenshot` and `pdf` formats) on
//! `POST /v1/scrape`.
//!
//! Off by default. `QUARRY_EDGE__SCRAPE_CAPTURE` is read once: only `on`
//! lets a request ask for the formats; anything else keeps them refused with a
//! field error (`feature_disabled`), so a typo never widens the endpoint. A
//! capture always runs in the browser (the plan is a browser fetch with no
//! fallback chain), never reads or writes the page cache, and under Zero Data
//! Retention its bytes travel inline under
//! `QUARRY_EDGE__SCRAPE_CAPTURE_INLINE_MAX_BYTES`.
//!
//! The contract checks (`ResolvedScrapeOptions::resolve`) and the browser
//! capability check (501 `browser_capability_unavailable`, refused before the
//! pool is touched) live in the contract and the runtime. This module only
//! chooses the plan and reads the environment.

use std::sync::OnceLock;

use quarry_core::capture::CaptureLimits;
use quarry_core::scrape::ResolvedScrapeOptions;
use quarry_runtime::driver::RenderHints;
use quarry_runtime::driver_plan::{plan_from_signals, DriverSignals};
use quarry_runtime::DriverPlan;

/// The switch for the capture formats. Only `on` enables them.
pub const ENV_SCRAPE_CAPTURE: &str = "QUARRY_EDGE__SCRAPE_CAPTURE";
/// The inline cap for captures returned under ZDR, in bytes.
pub const ENV_SCRAPE_CAPTURE_INLINE_MAX_BYTES: &str = "QUARRY_EDGE__SCRAPE_CAPTURE_INLINE_MAX_BYTES";

/// Whether the capture formats are accepted on this endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureMode {
    Off,
    On,
}

impl CaptureMode {
    pub fn enabled(self) -> bool {
        self == Self::On
    }
}

/// Only the exact value `on` enables captures. Anything else is off.
pub fn parse_capture_mode(value: Option<&str>) -> CaptureMode {
    match value.map(|raw| raw.trim().to_ascii_lowercase()).as_deref() {
        Some("on") => CaptureMode::On,
        _ => CaptureMode::Off,
    }
}

/// The process switch, read once.
pub fn capture_mode() -> CaptureMode {
    static MODE: OnceLock<CaptureMode> = OnceLock::new();
    *MODE.get_or_init(|| parse_capture_mode(std::env::var(ENV_SCRAPE_CAPTURE).ok().as_deref()))
}

/// The inline cap, parsed. The default is 6 MiB; a value that does not parse
/// or is zero is the default; a larger value is clamped to the 16 MiB ceiling.
pub fn inline_max_bytes_from(value: Option<&str>) -> usize {
    use quarry_runtime::capture::{DEFAULT_INLINE_CAPTURE_MAX_BYTES, MAX_INLINE_CAPTURE_BYTES};
    value
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|bytes| *bytes > 0)
        .unwrap_or(DEFAULT_INLINE_CAPTURE_MAX_BYTES)
        .min(MAX_INLINE_CAPTURE_BYTES)
}

/// The process inline cap, read once.
pub fn inline_max_bytes() -> usize {
    static BYTES: OnceLock<usize> = OnceLock::new();
    *BYTES.get_or_init(|| {
        inline_max_bytes_from(
            std::env::var(ENV_SCRAPE_CAPTURE_INLINE_MAX_BYTES)
                .ok()
                .as_deref(),
        )
    })
}

/// The driver plan for a scrape. A capture needs the browser, so the plan is a
/// browser fetch with no fallback chain. Otherwise the request's signals decide,
/// as they always have.
pub fn plan_for(resolved: &ResolvedScrapeOptions, signals: DriverSignals) -> DriverPlan {
    if resolved.capture.is_empty() {
        plan_from_signals(signals)
    } else {
        DriverPlan::browser_fetch("screenshot or pdf requires the browser")
    }
}

/// Put the request's captures on the render hints the runner takes. Nothing is
/// set when the request captures nothing, so the default path is unchanged.
pub fn apply_capture(
    render: &mut RenderHints,
    resolved: &ResolvedScrapeOptions,
    limits: CaptureLimits,
) {
    if resolved.capture.is_empty() {
        return;
    }
    render.capture = Some(resolved.capture);
    render.capture_limits = limits;
    render.capture_inline_max_bytes = Some(inline_max_bytes());
}

/// Whether a raw `formats` value asks for a capture: a `screenshot` or `pdf`
/// entry, as a string or as an object with a `type`. Used by the stream route,
/// which serves no captures.
pub fn requests_capture(formats: Option<&serde_json::Value>) -> bool {
    formats
        .and_then(serde_json::Value::as_array)
        .is_some_and(|items| {
            items.iter().any(|item| {
                let name = match item {
                    serde_json::Value::String(name) => Some(name.as_str()),
                    serde_json::Value::Object(map) => {
                        map.get("type").and_then(serde_json::Value::as_str)
                    }
                    _ => None,
                };
                matches!(name, Some("screenshot") | Some("pdf"))
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_exact_value_on_enables_captures() {
        assert_eq!(parse_capture_mode(None), CaptureMode::Off);
        assert_eq!(parse_capture_mode(Some("off")), CaptureMode::Off);
        assert_eq!(parse_capture_mode(Some("1")), CaptureMode::Off);
        assert_eq!(parse_capture_mode(Some("yes")), CaptureMode::Off);
        assert_eq!(parse_capture_mode(Some(" ON ")), CaptureMode::On);
        assert!(!CaptureMode::Off.enabled());
        assert!(CaptureMode::On.enabled());
    }

    #[test]
    fn inline_cap_defaults_and_is_clamped_to_the_ceiling() {
        assert_eq!(inline_max_bytes_from(None), 6_291_456);
        assert_eq!(inline_max_bytes_from(Some("0")), 6_291_456);
        assert_eq!(inline_max_bytes_from(Some("nope")), 6_291_456);
        assert_eq!(inline_max_bytes_from(Some("1048576")), 1_048_576);
        assert_eq!(inline_max_bytes_from(Some("999999999999")), 16_777_216);
    }

    #[test]
    fn requests_capture_reads_strings_and_typed_objects_only() {
        let json = |raw: &str| serde_json::from_str::<serde_json::Value>(raw).unwrap();
        assert!(requests_capture(Some(&json(r#"["markdown","pdf"]"#))));
        assert!(requests_capture(Some(&json(
            r#"[{"type":"screenshot","fullPage":true}]"#
        ))));
        assert!(!requests_capture(Some(&json(r#"["markdown",{"type":"html"}]"#))));
        assert!(!requests_capture(None));
    }

    #[test]
    fn a_capture_request_plans_the_browser_with_no_chain() {
        let resolved = ResolvedScrapeOptions {
            capture: quarry_core::capture::CapturePlan {
                screenshot: Some(Default::default()),
                pdf: None,
            },
            ..empty_resolved()
        };
        let plan = plan_for(&resolved, DriverSignals::default());
        assert_eq!(plan.driver, quarry_core::output::DriverKind::Browser);
        assert!(plan.fallback_chain.is_empty());

        let mut render = RenderHints::default();
        apply_capture(&mut render, &resolved, CaptureLimits::default());
        assert!(render.needs_browser());
        assert_eq!(
            render.capture_inline_max_bytes,
            Some(inline_max_bytes())
        );
    }

    #[test]
    fn a_request_without_captures_leaves_the_render_hints_alone() {
        let resolved = empty_resolved();
        let mut render = RenderHints::default();
        apply_capture(&mut render, &resolved, CaptureLimits::default());
        assert!(render.capture.is_none());
        assert!(render.capture_inline_max_bytes.is_none());
    }

    fn empty_resolved() -> ResolvedScrapeOptions {
        quarry_core::scrape::ScrapeOptions::default()
            .resolve(false, &quarry_core::scrape::ScrapeLimits::default())
            .expect("default options resolve")
    }
}
