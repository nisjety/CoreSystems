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
//! chooses the plan and reads the environment. E4.4-a's page emulation (locale,
//! timezone, location) takes the same browser plan (`apply_page_emulation`).

use std::sync::OnceLock;

use axum::extract::Request;
use axum::http::{Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use quarry_core::capture::CaptureLimits;
use quarry_core::error::{ErrorCode, QuarryError};
use quarry_core::page_emulation::PageEmulation;
use quarry_core::scrape::{FieldError, ResolvedScrapeOptions};
use quarry_runtime::driver::RenderHints;
use quarry_runtime::driver_plan::{plan_from_signals, DriverSignals};
use quarry_runtime::{DriverPlan, PageRunner};

use crate::auth::{Claims, SCOPE_SCRAPE_INTERACT};

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
    // WS13.5: actions run in the browser too, so they take the same plan. E4.4-a: so does a page
    // emulation, which only a browser can set. E4.4-b: and a pinned profile, which only a browser
    // can restore.
    if resolved.capture.is_empty()
        && resolved.actions.is_none()
        && resolved.page_emulation.is_none()
        && resolved.profile_id.is_none()
    {
        plan_from_signals(signals)
    } else {
        DriverPlan::browser_fetch(
            "screenshot, pdf, actions, page emulation or a profile pin require the browser",
        )
    }
}

/// E4.4-a: put the request's locale, timezone and location on the render hints the runner takes.
/// Nothing is set when the request asks for none, so the default path is unchanged.
pub fn apply_page_emulation(render: &mut RenderHints, resolved: &ResolvedScrapeOptions) {
    render.page_emulation = resolved.page_emulation.clone();
}

/// E4.4-a: the refusal for a page emulation when no browser driver is wired: one field error per
/// emulated field, each `unsupported_option`, on a 400 (an option this edge does not offer).
pub fn page_emulation_unsupported(emulation: &PageEmulation) -> Vec<FieldError> {
    let asked = [
        ("locale", emulation.locale.is_some()),
        ("timezone", emulation.timezone.is_some()),
        ("location", emulation.location.is_some()),
    ];
    asked
        .into_iter()
        .filter(|(_, set)| *set)
        .map(|(path, _)| {
            FieldError::new(
                path,
                "unsupported_option",
                format!("{path} needs a browser, and this edge has none to emulate it on"),
            )
        })
        .collect()
}

/// E4.4-b: put the request's pinned profile on the render hints the runner takes, as the
/// canonical `prof_<ulid>` the contract checked. Nothing is set when the request pins none, so the
/// default path is unchanged. A value that does not parse sets nothing; the scrape route refuses
/// such a request before it gets here, so the run is never unpinned by accident.
pub fn apply_profile_pin(render: &mut RenderHints, resolved: &ResolvedScrapeOptions) {
    render.profile_pin = resolved
        .profile_id
        .as_deref()
        .and_then(|raw| raw.parse::<quarry_core::ids::kinds::ProfileKind>().ok());
}

/// E4.4-b: the refusal for a profile pin when no browser driver is wired: one field error at
/// `profile_id`, `unsupported_option`, on a 400 (an option this edge does not offer). A profile is
/// restored only into a browser, and the default driver would run the page without it.
pub fn profile_pin_unsupported() -> Vec<FieldError> {
    vec![FieldError::new(
        "profile_id",
        "unsupported_option",
        "profile_id needs a browser, and this edge has none to restore a profile into",
    )]
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

/// WS13.4b: the refusal for a capture when no browser driver is wired. A
/// browser plan with no browser registered would otherwise reach the default
/// driver, which ignores the capture, so the page would come back without it.
pub fn browser_unavailable() -> QuarryError {
    QuarryError::new(
        ErrorCode::Unsupported,
        "screenshot, pdf and actions need a browser, and none is available on this endpoint",
    )
    .with_details(serde_json::json!({ "reason": "browser_capability_unavailable" }))
}

/// WS13.5: the switch for scrape-time `actions[]`, read once. `off` (the default)
/// keeps `actions` refused with a field error (`feature_disabled`). `user` lets a
/// user token send them and nothing else (dev only). `on` also lets a service
/// principal that holds `scrape:interact` send them. Anything else is off.
pub const ENV_SCRAPE_ACTIONS: &str = "QUARRY_EDGE__SCRAPE_ACTIONS";

/// The handler's bound on a run that carries actions (WS13.5), in milliseconds.
pub const ACTIONS_RUN_DEADLINE_MS: u64 = 115_000;

/// The router's deadline for `POST /v1/scrape` while actions are on (WS13.5).
pub const ACTIONS_ROUTE_DEADLINE: std::time::Duration = std::time::Duration::from_secs(125);

/// The router's deadline on every other route (unchanged since WS5).
pub const DEFAULT_ROUTE_DEADLINE: std::time::Duration = std::time::Duration::from_secs(60);

/// Who may send scrape-time actions, from the switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionsMode {
    Off,
    User,
    On,
}

impl ActionsMode {
    pub fn enabled(self) -> bool {
        self != Self::Off
    }
}

/// Only `on` and `user` (ignoring case and surrounding space) turn actions on.
pub fn parse_actions_mode(value: Option<&str>) -> ActionsMode {
    match value.map(|raw| raw.trim().to_ascii_lowercase()).as_deref() {
        Some("on") => ActionsMode::On,
        Some("user") => ActionsMode::User,
        _ => ActionsMode::Off,
    }
}

/// The process switch, read once.
pub fn actions_mode() -> ActionsMode {
    static MODE: OnceLock<ActionsMode> = OnceLock::new();
    *MODE.get_or_init(|| parse_actions_mode(std::env::var(ENV_SCRAPE_ACTIONS).ok().as_deref()))
}

/// WS13.5: whether this principal may send `actions`. The check only tightens:
/// a request labelled `initiated_by: "agent"` (a Model-proposed action relayed
/// under the user's bearer) is refused whatever the token, a service principal
/// needs `scrape:interact`, and the `user` mode admits user tokens only. Nothing
/// here widens what a principal may already do.
pub fn check_principal(
    claims: &Claims,
    initiated_by: Option<&str>,
    mode: ActionsMode,
) -> Result<(), QuarryError> {
    let refused = initiated_by == Some("agent")
        || (claims.is_service() && !claims.has_scope(SCOPE_SCRAPE_INTERACT))
        || (mode == ActionsMode::User && claims.is_service());
    if refused {
        return Err(QuarryError::new(
            ErrorCode::Forbidden,
            "scrape actions need a user request, or a service principal with the scrape:interact scope",
        )
        .with_details(serde_json::json!({ "reason": "scrape_interact_scope_required" })));
    }
    Ok(())
}

/// WS13.5: put the request's actions on the render hints. Nothing is set when the
/// request has none, so the default path is unchanged. An action that takes a
/// screenshot or a PDF runs under the same capture limits, and under ZDR its bytes
/// travel inline under the same cap, as a final capture's do.
pub fn apply_actions(
    render: &mut RenderHints,
    resolved: &ResolvedScrapeOptions,
    limits: CaptureLimits,
) {
    if let Some(actions) = resolved.actions.as_ref() {
        render.actions = Some(std::sync::Arc::new(actions.clone()));
        render.actions_deadline = Some(std::time::Duration::from_millis(ACTIONS_RUN_DEADLINE_MS));
        if actions.capture_count > 0 && render.capture_inline_max_bytes.is_none() {
            render.capture_limits = limits;
            render.capture_inline_max_bytes = Some(inline_max_bytes());
        }
    }
}

/// WS13.5: the safe-mode runner. A page an action changed is not evidence of its
/// URL, so nothing that indexes, titles, re-renders, registers or ingests it may
/// run for a request with actions. Every optional hop on [`PageRunner`] that can
/// persist or derive knowledge from the page is cleared here, in one place, and
/// the test `safe_mode_clears_every_evidence_hop` names each field.
pub fn safe_mode_runner(mut runner: PageRunner, actions: bool) -> PageRunner {
    if actions {
        runner.ingest = None;
        runner.local_index = None;
        runner.page_renderer = None;
        runner.source_registrar = None;
        runner.title_enricher = None;
        // Q3-E3.6-7: a changed page's document is not sent to the OCR lane either.
        runner.document_lanes = None;
    }
    runner
}

/// WS13.5: the handler's bound on the run. With actions it is 115 s; otherwise the
/// caller's `timeout_ms`, already capped by WS5.
pub fn run_deadline_ms(has_actions: bool, timeout_ms: u64) -> u64 {
    if has_actions {
        ACTIONS_RUN_DEADLINE_MS
    } else {
        timeout_ms
    }
}

/// WS13.5: the details of the handler's timeout. The `actions_deadline` reason
/// is added only for a run that carried actions, so the default body is unchanged.
pub fn run_timeout_details(has_actions: bool, deadline_ms: u64) -> serde_json::Value {
    if has_actions {
        serde_json::json!({
            "op": "scrape",
            "deadline_ms": deadline_ms,
            "reason": "actions_deadline",
        })
    } else {
        serde_json::json!({
            "op": "scrape",
            "deadline_ms": deadline_ms,
        })
    }
}

/// The router's deadline for a request. `POST /v1/scrape` gets 125 s while
/// actions are on (WS13.5) or the AI formats are on (INNH-1: the fetch keeps its
/// own cap and the model phase has 45 s of its own, so 60 s would cut an AI
/// request); everything else keeps 60 s.
pub fn route_deadline(
    scrape_route: bool,
    actions_enabled: bool,
    ai_formats_enabled: bool,
) -> std::time::Duration {
    if scrape_route && (actions_enabled || ai_formats_enabled) {
        ACTIONS_ROUTE_DEADLINE
    } else {
        DEFAULT_ROUTE_DEADLINE
    }
}

/// WS13.5: the router's deadline, path-aware. `POST /v1/scrape` gets 125 s while
/// actions are on, so the handler's 115 s bound answers first; every other route
/// keeps its 60 s. The refusal is the empty 408 that `TimeoutLayer` gives.
pub async fn path_deadline(request: Request, next: Next) -> Response {
    let actions_route = request.method() == Method::POST && request.uri().path() == "/v1/scrape";
    let limit = route_deadline(
        actions_route,
        actions_mode().enabled(),
        crate::scrape_contract::ai_formats::enabled(),
    );
    match tokio::time::timeout(limit, next.run(request)).await {
        Ok(response) => response,
        Err(_) => StatusCode::REQUEST_TIMEOUT.into_response(),
    }
}

#[cfg(test)]
mod actions_tests {
    use super::*;

    fn claims(principal: &str, scopes: &[&str]) -> Claims {
        serde_json::from_value(serde_json::json!({
            "sub": "subject_test",
            "iss": "issuer_test",
            "exp": 4_102_444_800_i64,
            "org_id": "org_test",
            "principal_type": principal,
            "scopes": scopes,
        }))
        .expect("claims")
    }

    #[test]
    fn actions_are_off_unless_the_switch_names_a_mode() {
        assert_eq!(parse_actions_mode(None), ActionsMode::Off);
        assert_eq!(parse_actions_mode(Some("off")), ActionsMode::Off);
        assert_eq!(parse_actions_mode(Some("1")), ActionsMode::Off);
        assert_eq!(parse_actions_mode(Some(" USER ")), ActionsMode::User);
        assert_eq!(parse_actions_mode(Some("on")), ActionsMode::On);
        assert!(!ActionsMode::Off.enabled());
    }

    #[test]
    fn agent_labelled_user_token_cannot_send_actions() {
        let user = claims("user", &["scrape:read"]);
        let error = check_principal(&user, Some("agent"), ActionsMode::On).expect_err("refused");
        assert_eq!(error.code, ErrorCode::Forbidden);
        assert_eq!(error.details.unwrap()["reason"], "scrape_interact_scope_required");
        assert!(check_principal(&user, None, ActionsMode::On).is_ok());
    }

    #[test]
    fn service_principal_needs_scrape_interact() {
        let bare = claims("service", &["scrape:read"]);
        assert!(check_principal(&bare, None, ActionsMode::On).is_err());
        let granted = claims("service", &["scrape:read", SCOPE_SCRAPE_INTERACT]);
        assert!(check_principal(&granted, None, ActionsMode::On).is_ok());
        assert!(check_principal(&granted, None, ActionsMode::User).is_err());
    }

    #[test]
    fn deadlines_are_125_for_the_route_and_115_for_the_run_with_actions() {
        assert_eq!(run_deadline_ms(true, 55_000), 115_000);
        assert_eq!(run_deadline_ms(false, 55_000), 55_000);
        assert_eq!(ACTIONS_ROUTE_DEADLINE.as_secs(), 125);
        assert_eq!(DEFAULT_ROUTE_DEADLINE.as_secs(), 60);
        let details = run_timeout_details(true, 115_000);
        assert_eq!(details["reason"], "actions_deadline");
        assert!(run_timeout_details(false, 55_000).get("reason").is_none());
    }

    #[test]
    fn the_scrape_route_gets_125_seconds_when_actions_or_ai_formats_are_on() {
        let secs = |scrape, actions, ai| route_deadline(scrape, actions, ai).as_secs();
        assert_eq!(secs(true, false, false), 60, "both off: unchanged");
        assert_eq!(secs(true, true, false), 125);
        assert_eq!(secs(true, false, true), 125, "INNH-1: the AI phase needs the room");
        assert_eq!(secs(true, true, true), 125);
        assert_eq!(secs(false, true, true), 60, "no other route is lengthened");
        assert_eq!(secs(false, false, false), 60);
        // 55 s fetch cap + 45 s model phase fits under the router's deadline.
        assert!(55 + quarry_runtime::ai_scrape::DEFAULT_PHASE_DEADLINE.as_secs() < 125);
    }
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

    #[test]
    fn a_page_emulation_plans_the_browser_and_reaches_the_render_hints() {
        let limits = quarry_core::scrape::ScrapeLimits {
            per_page_emulation_enabled: true,
            ..quarry_core::scrape::ScrapeLimits::default()
        };
        let resolved = quarry_core::scrape::ScrapeOptions::from_value(
            &serde_json::json!({ "locale": "nb-NO" }),
        )
        .expect("the options parse")
        .resolve(false, &limits)
        .expect("the emulation resolves");
        let plan = plan_for(&resolved, DriverSignals::default());
        assert_eq!(plan.driver, quarry_core::output::DriverKind::Browser);
        assert!(plan.fallback_chain.is_empty());

        let mut render = RenderHints::default();
        apply_page_emulation(&mut render, &resolved);
        assert!(render.needs_browser());
        assert_eq!(
            render.page_emulation.and_then(|emulation| emulation.locale).as_deref(),
            Some("nb_NO")
        );
    }

    #[test]
    fn a_request_without_an_emulation_leaves_the_plan_and_the_hints_alone() {
        let resolved = empty_resolved();
        let mut render = RenderHints::default();
        apply_page_emulation(&mut render, &resolved);
        assert!(render.page_emulation.is_none());
        assert!(!render.needs_browser());
        assert_eq!(
            plan_for(&resolved, DriverSignals::default()).driver,
            plan_from_signals(DriverSignals::default()).driver
        );
    }

    #[test]
    fn a_page_emulation_without_a_browser_names_each_field_it_asked_for() {
        let emulation = PageEmulation {
            locale: Some("nb_NO".to_owned()),
            timezone: Some("Europe/Oslo".to_owned()),
            location: None,
            send_accept_language: false,
        };
        let named: Vec<(String, String)> = page_emulation_unsupported(&emulation)
            .into_iter()
            .map(|field| (field.path, field.code))
            .collect();
        assert_eq!(
            named,
            vec![
                ("locale".to_owned(), "unsupported_option".to_owned()),
                ("timezone".to_owned(), "unsupported_option".to_owned()),
            ]
        );
    }

    fn empty_resolved() -> ResolvedScrapeOptions {
        quarry_core::scrape::ScrapeOptions::default()
            .resolve(false, &quarry_core::scrape::ScrapeLimits::default())
            .expect("default options resolve")
    }

    fn resolved_with(actions: serde_json::Value) -> ResolvedScrapeOptions {
        let options = quarry_core::scrape::ScrapeOptions::from_value(&serde_json::json!({
            "actions": actions
        }))
        .expect("the options parse");
        let limits = quarry_core::scrape::ScrapeLimits {
            actions_enabled: true,
            ..quarry_core::scrape::ScrapeLimits::default()
        };
        options.resolve(false, &limits).expect("the actions resolve")
    }

    #[test]
    fn actions_with_a_capture_take_the_capture_limits_and_the_inline_cap() {
        let resolved = resolved_with(serde_json::json!([{ "type": "screenshot" }]));
        let limits = CaptureLimits::default();
        let mut render = RenderHints::default();
        apply_actions(&mut render, &resolved, limits);
        assert!(render.actions.is_some());
        assert_eq!(render.capture_limits, limits);
        assert_eq!(render.capture_inline_max_bytes, Some(inline_max_bytes()));
        assert!(render.capture.is_none(), "an action capture is not a final capture");
    }

    #[test]
    fn actions_without_a_capture_leave_the_capture_hints_alone() {
        let resolved = resolved_with(serde_json::json!([{ "type": "press", "key": "Enter" }]));
        let mut render = RenderHints::default();
        apply_actions(&mut render, &resolved, CaptureLimits::default());
        assert!(render.actions.is_some());
        assert!(render.capture_inline_max_bytes.is_none());
    }

    /// A runner with every optional hop empty, to be filled by a test.
    fn bare_runner() -> PageRunner {
        use quarry_core::privacy::PrivacyPolicy;
        use quarry_core::zdr::ZdrMode;
        use std::sync::Arc;
        let (tx, _rx) = tokio::sync::mpsc::channel(4);
        PageRunner {
            driver: Arc::new(
                quarry_runtime::fetch::StaticDriver::new(std::time::Duration::from_secs(1), "test")
                    .expect("static driver"),
            ),
            security: Arc::new(
                quarry_security::preflight::DefaultEngine::new().with_allow_private_hosts(true),
            ),
            artifacts: Arc::new(quarry_runtime::artifact_store::InMemoryStore::new()),
            event_sink: quarry_runtime::EventSink::new(tx),
            zdr: ZdrMode::Off,
            ingest: None,
            org_id: Some("org_safe".into()),
            user_id: None,
            privacy: PrivacyPolicy::default(),
            cancel_token: None,
            local_index: None,
            policy: quarry_runtime::RunPolicy::default(),
            scheduler: None,
            autoscale: None,
            render: RenderHints::default(),
            page_renderer: None,
            source_registrar: None,
            title_enricher: None,
            doc_routing: quarry_runtime::DocRouting::Off,
            robots: None,
            document_lanes: None,
        }
    }

    struct NoIngest;

    #[async_trait::async_trait]
    impl quarry_runtime::ingest_client::DataPlaneIngest for NoIngest {
        async fn ingest(
            &self,
            _request: &quarry_core::contracts::DataPlaneIngestRequest,
        ) -> quarry_core::QuarryResult<quarry_core::contracts::DataPlaneIngestResponse> {
            Err(QuarryError::new(ErrorCode::Internal, "not reached"))
        }
    }

    struct NoRegistrar;

    #[async_trait::async_trait]
    impl quarry_runtime::source_registrar::SourceRegistrar for NoRegistrar {
        async fn register_source(
            &self,
            _registration: quarry_runtime::source_registrar::SourceRegistration<'_>,
        ) -> quarry_core::QuarryResult<()> {
            Ok(())
        }
    }

    /// D2-025. Safe mode is the absence of five optional hops. A field that is
    /// added to `PageRunner` and not cleared would leave safe mode a silent no-op
    /// for that hop, so this test (1) fills every hop it can construct and proves
    /// each is cleared, (2) proves a request without actions keeps them all, and
    /// (3) destructures `PageRunner` with every field named, so a new field fails
    /// to compile here until someone decides which side of safe mode it is on.
    /// `page_renderer` needs a NATS JetStream context and cannot be built in a unit
    /// test: its assignment is covered by the destructure and by the evidence
    /// step (a live actions request leaves the page-image index untouched).
    #[tokio::test]
    async fn safe_mode_clears_every_evidence_hop() {
        use std::sync::Arc;
        let filled = || {
            let mut runner = bare_runner();
            runner.ingest = Some(Arc::new(NoIngest));
            runner.local_index = Some(
                quarry_runtime::local_index::TantivyLocalIndex::in_memory().expect("local index"),
            );
            runner.source_registrar = Some(Arc::new(NoRegistrar));
            runner.title_enricher = Some(Arc::new(quarry_runtime::page_extract::PageTitleEnricher::new(
                Arc::new(
                    quarry_runtime::mp_client::ModelPlaneClient::new("http://127.0.0.1:9")
                        .expect("model plane client"),
                ),
            )));
            runner.document_lanes = Some(Arc::new(
                quarry_runtime::mp_client::ModelPlaneClient::new("http://127.0.0.1:9")
                    .expect("model plane client"),
            ));
            runner
        };

        // A request without actions keeps every hop it was given.
        let kept = safe_mode_runner(filled(), false);
        assert!(kept.ingest.is_some(), "ingest stays without actions");
        assert!(kept.local_index.is_some(), "local_index stays without actions");
        assert!(kept.source_registrar.is_some(), "source_registrar stays without actions");
        assert!(kept.title_enricher.is_some(), "title_enricher stays without actions");
        assert!(kept.document_lanes.is_some(), "document_lanes stays without actions");

        // A request with actions loses them all, whatever it carried.
        let PageRunner {
            driver: _,
            security: _,
            artifacts: _,
            event_sink: _,
            zdr: _,
            ingest,
            org_id,
            user_id: _,
            privacy: _,
            cancel_token: _,
            local_index,
            policy: _,
            scheduler: _,
            autoscale: _,
            render: _,
            page_renderer,
            source_registrar,
            title_enricher,
            doc_routing: _,
            robots: _,
            document_lanes,
        } = safe_mode_runner(filled(), true);
        assert!(document_lanes.is_none(), "document_lanes is cleared by safe mode");
        assert!(ingest.is_none(), "ingest is cleared by safe mode");
        assert!(local_index.is_none(), "local_index is cleared by safe mode");
        assert!(page_renderer.is_none(), "page_renderer is cleared by safe mode");
        assert!(source_registrar.is_none(), "source_registrar is cleared by safe mode");
        assert!(title_enricher.is_none(), "title_enricher is cleared by safe mode");
        assert_eq!(org_id.as_deref(), Some("org_safe"), "safe mode keeps the org stamp");
    }
}
