//! Usage attributes for the browser and crawl metrics (WS6.C-e, the emission half).
//!
//! Three things live here: the closed set of surfaces a request can come from, the
//! metadata a usage event may carry (a closed allowlist, and a smaller one under ZDR),
//! and the gate that decides what the seven browser metrics do in each mode.
//!
//! Nothing here puts a URL, a host (except off ZDR, for map), a title or a fingerprint
//! into billing metadata under ZDR. Event ids are `quarry:<part>:...:<metric>`, where each
//! part is a run id, a segment label or a short hash, never caller text.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;
use quarry_runtime::usage_metrics as names;
use quarry_runtime::{BrowserMetricsMode, UsageEvent, UsageMeter};
use serde_json::{json, Map, Value};

use crate::auth::Claims;
use crate::budget::{RunClass, SegmentUsage};

/// The header a service stamps with the surface a request came from.
pub const SURFACE_HEADER: &str = "x-verevon-surface";

/// The header a service principal may stamp with the id of the parent run it acts for (E4.6 062a).
/// Advisory: it is a usage attribute only and is never authority.
pub const PARENT_RUN_HEADER: &str = "x-verevon-parent-run";

/// The environment switch for the parent-run usage attribute. Unset means off; `on`, `1` or `true`
/// turns it on.
pub const ENV_PARENT_RUN: &str = "QUARRY_EDGE__USAGE_PARENT_RUN";

/// The environment switch for the seven browser metrics. Unset means `off`.
pub const ENV_BROWSER_METRICS: &str = "QUARRY_EDGE__USAGE_BROWSER_METRICS";

/// The keys a ZDR usage event may carry. Any other key is never written under ZDR.
pub const ZDR_ALLOWLIST: [&str; 8] = [
    "zdr",
    "outcome",
    "class",
    "surface",
    "driver",
    "estimated",
    "counts",
    "user_id",
];

/// Where a request came from. A closed set: a header value outside it is not kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    Chat,
    BrowserUi,
    Space,
    Api,
    Schedule,
    Mcp,
    Internal,
}

impl Surface {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::BrowserUi => "browser_ui",
            Self::Space => "space",
            Self::Api => "api",
            Self::Schedule => "schedule",
            Self::Mcp => "mcp",
            Self::Internal => "internal",
        }
    }

    /// An exact match against the closed set. Anything else is `None`.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "chat" => Some(Self::Chat),
            "browser_ui" => Some(Self::BrowserUi),
            "space" => Some(Self::Space),
            "api" => Some(Self::Api),
            "schedule" => Some(Self::Schedule),
            "mcp" => Some(Self::Mcp),
            "internal" => Some(Self::Internal),
            _ => None,
        }
    }
}

/// The surface of a request. A user token gets `Api`: a person who reaches the edge with their
/// own token is calling the API, and the header is not read for it. A service token names its
/// surface in the header; an absent or unknown value is `Internal`.
pub fn surface_for(claims: &Claims, header: Option<&str>) -> Surface {
    if !claims.is_service() {
        return Surface::Api;
    }
    header.and_then(Surface::parse).unwrap_or(Surface::Internal)
}

/// Whether `QUARRY_EDGE__USAGE_PARENT_RUN` is on in this process. Read once.
pub fn parent_run_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var(ENV_PARENT_RUN)
            .map(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "on" | "1" | "true"
                )
            })
            .unwrap_or(false)
    })
}

/// The parent run id of a request (E4.6 062a). `Some` only when the switch is on, the principal is a
/// service and the header is a well-formed id (1 to 128 bytes of `[A-Za-z0-9._:-]`). A user token
/// never gets one and the header is not read for it. The value is advisory: it is written to usage
/// metadata and is never read by auth, scopes, budget, grants, ZDR, policy or rate limits.
pub fn parent_run_for(claims: &Claims, header: Option<&str>, enabled: bool) -> Option<String> {
    if !enabled || !claims.is_service() {
        return None;
    }
    header
        .filter(|value| crate::request_id::is_valid_inbound(value))
        .map(str::to_owned)
}

tokio::task_local! {
    /// The parent run id of the request being served, set by [`parent_run_layer`].
    static PARENT_RUN: Option<String>;
}

/// The parent run id of the request in scope, if the layer set one. It does not follow
/// `tokio::spawn`: work that leaves the request task must carry the value itself.
pub fn current_parent_run() -> Option<String> {
    PARENT_RUN.try_with(Clone::clone).ok().flatten()
}

/// Reads the advisory parent-run header of a service principal and scopes it to the request.
/// It must sit inside `require_auth`, which inserts the claims. A request with no claims, a user
/// token, an absent or malformed header, or the switch off runs with no parent run. It never
/// refuses a request.
pub async fn parent_run_layer(req: Request, next: Next) -> Response {
    let parent = req.extensions().get::<Claims>().and_then(|claims| {
        let header = req
            .headers()
            .get(PARENT_RUN_HEADER)
            .and_then(|value| value.to_str().ok());
        parent_run_for(claims, header, parent_run_enabled())
    });
    PARENT_RUN.scope(parent, next.run(req)).await
}

/// How an ended unit of usage finished. `Error`, `Refused`, `Cancelled` and `Timeout` are part of the
/// closed set that the shutdown drain (WS6.J) and the refusal paths emit; until those land only `Ok`
/// and `Expired` are constructed.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Ok,
    Error,
    Refused,
    Cancelled,
    Timeout,
    BudgetExhausted,
    Expired,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Error => "error",
            Self::Refused => "refused",
            Self::Cancelled => "cancelled",
            Self::Timeout => "timeout",
            Self::BudgetExhausted => "budget_exhausted",
            Self::Expired => "expired",
        }
    }
}

/// The attributes one usage event may carry. Built by the call site, turned into
/// metadata by [`metadata`], which decides what survives under ZDR.
#[derive(Debug, Clone)]
pub struct UsageAttrs {
    pub outcome: Outcome,
    pub surface: Surface,
    pub class: Option<&'static str>,
    pub driver: Option<&'static str>,
    pub user_id: Option<String>,
    pub estimated: bool,
    /// Counts of the work, for example `{"max_pages": 5}`. Allowed under ZDR.
    pub counts: Option<Value>,
    /// The seed host of a map call. Written only off ZDR.
    pub host: Option<String>,
    /// Written only off ZDR.
    pub paid_provider: Option<bool>,
    /// Written only off ZDR.
    pub llm_format: Option<&'static str>,
    /// The advisory id of the parent run a service acts for (E4.6 062a). Written only off ZDR. When
    /// `None`, [`current_parent_run`] supplies the request's value.
    pub parent_run: Option<String>,
}

impl UsageAttrs {
    pub fn new(outcome: Outcome, surface: Surface) -> Self {
        Self {
            outcome,
            surface,
            class: None,
            driver: None,
            user_id: None,
            estimated: false,
            counts: None,
            host: None,
            paid_provider: None,
            llm_format: None,
            parent_run: None,
        }
    }
}

/// The metadata of one event. Off ZDR it is the closed set plus the non-ZDR keys; under ZDR it
/// is the [`ZDR_ALLOWLIST`] only, so no URL, host, title or fingerprint can reach billing.
pub fn metadata(attrs: &UsageAttrs, zdr: bool) -> Value {
    let mut map = Map::new();
    map.insert("outcome".into(), json!(attrs.outcome.as_str()));
    map.insert("surface".into(), json!(attrs.surface.as_str()));
    map.insert("zdr".into(), json!(zdr));
    map.insert("estimated".into(), json!(attrs.estimated));
    if let Some(class) = attrs.class {
        map.insert("class".into(), json!(class));
    }
    if let Some(driver) = attrs.driver {
        map.insert("driver".into(), json!(driver));
    }
    if let Some(user) = attrs.user_id.as_deref().filter(|user| !user.is_empty()) {
        map.insert("user_id".into(), json!(user));
    }
    if let Some(counts) = &attrs.counts {
        map.insert("counts".into(), counts.clone());
    }
    if !zdr {
        if let Some(host) = &attrs.host {
            map.insert("host".into(), json!(host));
        }
        if let Some(paid) = attrs.paid_provider {
            map.insert("paid_provider".into(), json!(paid));
        }
        if let Some(format) = attrs.llm_format {
            map.insert("llm_format".into(), json!(format));
        }
        // Advisory attribution only; ZDR_ALLOWLIST is unchanged, so under ZDR the id is dropped.
        if let Some(parent) = attrs.parent_run.clone().or_else(current_parent_run) {
            map.insert("parent_run".into(), json!(parent));
        }
    }
    Value::Object(map)
}

/// Whether the corrected rules for the existing metrics are in force (the mode is `on`).
/// Call sites that change what an existing metric counts check this, so the legacy rule
/// keeps running in `off` and `shadow`.
pub fn corrected_rules(meter: &dyn UsageMeter) -> bool {
    meter.browser_metrics_mode() == BrowserMetricsMode::On
}

/// Emits one event with an id built from `parts`. Returns whether an event was handed to the
/// meter. A zero, negative or non-finite quantity is never emitted, and neither is an id that
/// billing-core would refuse (the refusal is logged by its class, not by its text).
pub async fn emit(
    meter: &dyn UsageMeter,
    parts: &[&str],
    org_id: &str,
    metric: &'static str,
    quantity: f64,
    attrs: &UsageAttrs,
    zdr: bool,
) -> bool {
    if !(quantity.is_finite() && quantity > 0.0) {
        return false;
    }
    match UsageEvent::keyed(parts, org_id, metric, quantity, metadata(attrs, zdr)) {
        Ok(event) => {
            meter.meter(event).await;
            true
        }
        Err(error) => {
            // zdr-log-ok: the id error variant and the closed metric name; the id itself (run ids and hashes) is never logged
            tracing::warn!(?error, metric, "usage event id refused");
            false
        }
    }
}

/// The usage of one run segment (WS6.C-e). An agent segment emits its browser seconds, one
/// `agent.run` and its completed steps; a person's session emits its browser seconds only.
/// Seconds are whole and a zero is skipped. The ids are `quarry:<run_id>:s<segment>:<metric>`,
/// so a resumed run's segments never share an id. A latched bound overrides the outcome.
/// The segment carries the run's own parent run (E4.6 062a), never the closing request's.
#[allow(clippy::too_many_arguments)] // one call site; the arguments are the segment's identity
pub async fn emit_run_segment(
    meter: &dyn UsageMeter,
    run_id: &str,
    org_id: &str,
    user_id: &str,
    zdr: bool,
    usage: &SegmentUsage,
    outcome: Outcome,
) {
    // The segment is emitted by whichever request closes the run, so that request's task-local is
    // in scope here. `metadata` falls back to the task-local when an attribute is unset, so the
    // task-local is set to the run's value: a closing request's header never reaches the segment.
    PARENT_RUN
        .scope(
            usage.parent_run.clone(),
            emit_segment_events(meter, run_id, org_id, user_id, zdr, usage, outcome),
        )
        .await
}

/// The events of one run segment, emitted under the run's parent run by [`emit_run_segment`].
#[allow(clippy::too_many_arguments)] // see emit_run_segment
async fn emit_segment_events(
    meter: &dyn UsageMeter,
    run_id: &str,
    org_id: &str,
    user_id: &str,
    zdr: bool,
    usage: &SegmentUsage,
    outcome: Outcome,
) {
    let segment = format!("s{}", usage.segment);
    let parts = [run_id, segment.as_str()];
    let outcome = if usage.exhausted {
        Outcome::BudgetExhausted
    } else {
        outcome
    };
    let mut attrs = UsageAttrs::new(outcome, usage.surface);
    attrs.class = Some(usage.class.label());
    attrs.user_id = Some(user_id.to_owned());
    attrs.estimated = true;
    attrs.parent_run = usage.parent_run.clone();
    let whole_seconds = (usage.active_ms / 1_000) as f64;
    match usage.class {
        RunClass::Agent => {
            emit(
                meter,
                &parts,
                org_id,
                names::BROWSER_SECOND_AGENT,
                whole_seconds,
                &attrs,
                zdr,
            )
            .await;
            emit(meter, &parts, org_id, names::AGENT_RUN, 1.0, &attrs, zdr).await;
            emit(
                meter,
                &parts,
                org_id,
                names::AGENT_STEP,
                f64::from(usage.steps),
                &attrs,
                zdr,
            )
            .await;
        }
        RunClass::Interactive => {
            emit(
                meter,
                &parts,
                org_id,
                names::BROWSER_SECOND_INTERACTIVE,
                whole_seconds,
                &attrs,
                zdr,
            )
            .await;
        }
    }
}

/// The gate in front of the usage meter (WS6.C-e). It applies `QUARRY_EDGE__USAGE_BROWSER_METRICS`
/// to the seven browser metrics and passes every other event through unchanged:
/// `off` drops them, `shadow` counts them in `quarry_usage_shadow_events_total` and publishes
/// nothing, `on` publishes them.
pub struct ShadowMeter {
    inner: Arc<dyn UsageMeter>,
    mode: BrowserMetricsMode,
}

impl ShadowMeter {
    pub fn new(inner: Arc<dyn UsageMeter>, mode: BrowserMetricsMode) -> Self {
        Self { inner, mode }
    }
}

#[async_trait]
impl UsageMeter for ShadowMeter {
    async fn meter(&self, event: UsageEvent) {
        if names::BROWSER_METRICS.contains(&event.metric.as_str()) {
            match self.mode {
                BrowserMetricsMode::Off => return,
                BrowserMetricsMode::Shadow => {
                    ::metrics::counter!(
                        "quarry_usage_shadow_events_total",
                        "metric" => event.metric.clone()
                    )
                    .increment(1);
                    return;
                }
                BrowserMetricsMode::On => {}
            }
        }
        self.inner.meter(event).await;
    }

    async fn flush(&self, deadline: Duration) {
        self.inner.flush(deadline).await;
    }

    fn browser_metrics_mode(&self) -> BrowserMetricsMode {
        self.mode
    }
}

/// The mode the environment selects. Unset is `off`. A value that is not `off`, `shadow` or `on`
/// is also `off`, and is logged once at boot.
pub fn browser_metrics_mode_from_env() -> BrowserMetricsMode {
    match std::env::var(ENV_BROWSER_METRICS) {
        Ok(raw) => BrowserMetricsMode::parse(&raw).unwrap_or_else(|| {
            tracing::warn!(
                "QUARRY_EDGE__USAGE_BROWSER_METRICS is not off, shadow or on; using off"
            );
            BrowserMetricsMode::Off
        }),
        Err(_) => BrowserMetricsMode::Off,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::Claims;
    use crate::budget::{RunBudget, RunClass};
    use proptest::prelude::*;
    use quarry_runtime::usage_metrics as metric_names;
    use std::sync::Mutex;

    fn claims(principal: Option<&str>, service: bool) -> Claims {
        let is_service = service;
        Claims {
            sub: if is_service {
                "service:browser".into()
            } else {
                "user_1".into()
            },
            iss: "auth-core".into(),
            exp: 4_102_444_800,
            org_id: "org_a".into(),
            user_id: if is_service {
                String::new()
            } else {
                "user_1".into()
            },
            principal_type: principal.map(str::to_owned),
            service_id: if is_service {
                Some("service:browser".into())
            } else {
                None
            },
            nbf: None,
            aud: None,
            scopes: vec![],
            zdr: None,
            web_policy: None,
            key_id: None,
            acting_user_id: None,
        }
    }

    /// Records every event it is given, so a test can read what reached the meter.
    #[derive(Default)]
    struct Recording(Mutex<Vec<UsageEvent>>);

    #[async_trait]
    impl UsageMeter for Recording {
        async fn meter(&self, event: UsageEvent) {
            self.0.lock().unwrap().push(event);
        }
    }

    fn billing_id_ok(id: &str) -> bool {
        let mut chars = id.chars();
        chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
            && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '-'))
            && id.len() <= 128
    }

    #[test]
    fn surface_is_derived_from_the_principal_for_user_tokens() {
        // A user token is the API surface whatever the header says, and a legacy token with no
        // principal type counts as a user.
        assert_eq!(
            surface_for(&claims(Some("user"), false), Some("chat")),
            Surface::Api
        );
        assert_eq!(surface_for(&claims(None, false), None), Surface::Api);
    }

    #[test]
    fn header_is_ignored_unless_service_token_and_closed_enum() {
        let svc = claims(Some("service"), true);
        assert_eq!(surface_for(&svc, Some("chat")), Surface::Chat);
        assert_eq!(surface_for(&svc, Some("browser_ui")), Surface::BrowserUi);
        // Not an exact member of the closed set: kept as internal, never reflected.
        assert_eq!(surface_for(&svc, Some("chat\n")), Surface::Internal);
        assert_eq!(surface_for(&svc, Some("Chat")), Surface::Internal);
        assert_eq!(surface_for(&svc, None), Surface::Internal);
    }

    #[test]
    fn zdr_metadata_is_an_allowlist_without_url_host_or_fingerprint() {
        let mut attrs = UsageAttrs::new(Outcome::Ok, Surface::Api);
        attrs.class = Some("agent");
        attrs.driver = Some("chromium");
        attrs.user_id = Some("user_1".into());
        attrs.counts = Some(json!({"items": 2}));
        attrs.host = Some("example.com".into());
        attrs.paid_provider = Some(true);
        attrs.llm_format = Some("json");
        let zdr = metadata(&attrs, true);
        let object = zdr.as_object().unwrap();
        for key in object.keys() {
            assert!(
                ZDR_ALLOWLIST.contains(&key.as_str()),
                "{key} is not allowlisted"
            );
        }
        for banned in [
            "url",
            "host",
            "fingerprint",
            "title",
            "paid_provider",
            "llm_format",
        ] {
            assert!(
                !object.contains_key(banned),
                "{banned} must not reach ZDR metadata"
            );
        }
        // Off ZDR the same attributes keep the non-ZDR keys.
        let plain = metadata(&attrs, false);
        assert_eq!(plain["host"], "example.com");
        assert_eq!(plain["paid_provider"], true);
    }

    proptest! {
        #[test]
        fn event_id_fits_the_billing_pattern_and_length(
            run in "[A-Za-z0-9][A-Za-z0-9_-]{0,80}",
            segment in 0u32..100_000,
        ) {
            let label = format!("s{segment}");
            let event = UsageEvent::keyed(
                &[run.as_str(), label.as_str()],
                "org_a",
                metric_names::AGENT_RUN,
                1.0,
                json!({}),
            );
            prop_assert!(event.is_ok());
            let event = event.unwrap();
            prop_assert!(billing_id_ok(&event.event_id));
        }
    }

    #[test]
    fn event_id_refuses_an_over_long_part() {
        let long = "a".repeat(130);
        assert!(UsageEvent::keyed(
            &[long.as_str()],
            "org_a",
            metric_names::AGENT_RUN,
            1.0,
            json!({})
        )
        .is_err());
    }

    #[test]
    fn resume_segments_produce_distinct_ids() {
        let first = UsageEvent::keyed(
            &["run_x", "s0"],
            "org_a",
            metric_names::AGENT_STEP,
            1.0,
            json!({}),
        )
        .unwrap();
        let resumed = UsageEvent::keyed(
            &["run_x", "s1"],
            "org_a",
            metric_names::AGENT_STEP,
            1.0,
            json!({}),
        )
        .unwrap();
        assert_ne!(first.event_id, resumed.event_id);
        assert!(billing_id_ok(&first.event_id) && billing_id_ok(&resumed.event_id));
    }

    #[tokio::test]
    async fn zero_quantity_is_not_emitted() {
        let recording = Recording::default();
        let attrs = UsageAttrs::new(Outcome::Ok, Surface::Api);
        assert!(
            !emit(
                &recording,
                &["run_x"],
                "org_a",
                metric_names::EXTRACT_URL,
                0.0,
                &attrs,
                false
            )
            .await
        );
        assert!(
            !emit(
                &recording,
                &["run_x"],
                "org_a",
                metric_names::EXTRACT_URL,
                f64::NAN,
                &attrs,
                false
            )
            .await
        );
        assert!(
            !emit(
                &recording,
                &["run_x"],
                "org_a",
                metric_names::EXTRACT_URL,
                -1.0,
                &attrs,
                false
            )
            .await
        );
        assert!(
            emit(
                &recording,
                &["run_x"],
                "org_a",
                metric_names::EXTRACT_URL,
                2.0,
                &attrs,
                false
            )
            .await
        );
        assert_eq!(recording.0.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn shadow_gate_drops_or_forwards_by_mode_and_passes_existing_metrics() {
        let attrs = UsageAttrs::new(Outcome::Ok, Surface::Api);
        for (mode, forwarded_browser) in [
            (BrowserMetricsMode::Off, 0usize),
            (BrowserMetricsMode::Shadow, 0),
            (BrowserMetricsMode::On, 1),
        ] {
            let inner = Arc::new(Recording::default());
            let gate = ShadowMeter::new(inner.clone(), mode);
            assert_eq!(gate.browser_metrics_mode(), mode);
            emit(
                &gate,
                &["run_x"],
                "org_a",
                metric_names::AGENT_RUN,
                1.0,
                &attrs,
                false,
            )
            .await;
            // The scrape metric is not gated, whatever the mode.
            gate.meter(UsageEvent::new(
                "req_1",
                "org_a",
                metric_names::SCRAPE_PAGE,
                1.0,
                json!({}),
            ))
            .await;
            let seen = inner.0.lock().unwrap();
            assert_eq!(seen.len(), 1 + forwarded_browser, "mode {}", mode.label());
            assert!(seen
                .iter()
                .any(|event| event.metric == metric_names::SCRAPE_PAGE));
        }
    }

    #[tokio::test]
    async fn agent_close_emits_seconds_run_and_steps_and_latched_bound_overrides_outcome() {
        use std::time::{Duration, Instant};
        let t0 = Instant::now();
        let constraints = quarry_core::contracts::AgentConstraints {
            max_steps: 50,
            allowed_domains: vec![],
            max_runtime_s: None,
            max_cost_usd: None,
        };
        let mut budget = RunBudget::start(
            &crate::budget::BudgetLimits::default(),
            RunClass::Agent,
            &constraints,
            None,
            t0,
        )
        .with_surface(Surface::Chat);
        budget.charge(true, 0);
        budget.charge(true, 0);
        let usage = budget.segment_usage(t0 + Duration::from_millis(20_400));

        let recording = Recording::default();
        emit_run_segment(
            &recording,
            "run_x",
            "org_a",
            "user_1",
            false,
            &usage,
            Outcome::Ok,
        )
        .await;
        {
            let seen = recording.0.lock().unwrap();
            let ids: Vec<&str> = seen.iter().map(|event| event.event_id.as_str()).collect();
            assert_eq!(seen.len(), 3, "{ids:?}");
            assert_eq!(seen[0].metric, metric_names::BROWSER_SECOND_AGENT);
            assert_eq!(seen[0].quantity, 20.0, "whole seconds, floored");
            assert_eq!(seen[1].metric, metric_names::AGENT_RUN);
            assert_eq!(seen[2].metric, metric_names::AGENT_STEP);
            assert_eq!(seen[2].quantity, 2.0);
            assert_eq!(
                seen[0].event_id,
                "quarry:run_x:s0:quarry.browser.second.agent"
            );
            assert_eq!(seen[0].metadata["surface"], "chat");
            assert_eq!(seen[0].metadata["outcome"], "ok");
        }

        // A manual session emits its own seconds and no run or step.
        let interactive = RunBudget::start(
            &crate::budget::BudgetLimits::default(),
            RunClass::Interactive,
            &constraints,
            None,
            t0,
        );
        let session = interactive.segment_usage(t0 + Duration::from_secs(10));
        let recording = Recording::default();
        emit_run_segment(
            &recording,
            "run_y",
            "org_a",
            "user_1",
            false,
            &session,
            Outcome::Ok,
        )
        .await;
        assert_eq!(recording.0.lock().unwrap().len(), 1);
    }
}
