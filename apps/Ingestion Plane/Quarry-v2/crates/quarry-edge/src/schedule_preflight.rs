//! `POST /v1/internal/schedules/preflight`: the fire-time check of an executable
//! scheduled job (WS15.S2, real checks WS15.S4a).
//!
//! The orchestrator's `ScheduledJobWF` asks this route before every fire. S2
//! answered always allow (posture `unchecked`, entitlement `floor`). S4a checks,
//! at fire time:
//!
//! - **ZDR posture**: org-core `GET /internal/orgs/:org/retention-posture`
//!   (`{zdrIntent, entitled}`; effective ZDR is `zdrIntent && entitled`). One read
//!   per fire, never cached, 3 s timeout. Effective ZDR answers `skipped_zdr` (and,
//!   under `enforce`, asks control to pause the schedule). A read that fails for
//!   any reason (timeout, 5xx, 404, no credential) answers
//!   `skipped_posture_unknown`: fail closed, and the next cron fire tries again.
//! - **Entitlement**: the WS6.D [`LimitsResolver`](crate::limits::LimitsResolver)
//!   on `AppState` (the `static` source answers the floor). A spent allowance for
//!   the schedule's kind answers `skipped_no_entitlement`; so does a resolver that
//!   refuses (fail mode `closed`).
//!
//! The closed reason set also holds `skipped_overlap` (written by the
//! orchestrator's reconcile, never by this route), `skipped_kind_disabled` and
//! `skipped_disabled` (answered when the caller sends the schedule's kind or
//! enabled state; today's orchestrator sends neither, so they stay latent).
//!
//! `QUARRY_EDGE__SCHEDULE_POSTURE` picks the mode: `off` (the S2 answer, no
//! reads), `observe` (the default: every check runs and is logged, the fire is
//! allowed, nothing is paused), `enforce` (a skip reason denies the fire).
//!
//! Authentication follows `run_page`: the shared runtime bearer token names the
//! caller, and the `X-Quarry-Run-Sig` run binding ties the tenant to the request.
//! The binding is signed over `org_id + "\n" + "schedule:" + schedule_id`, so a
//! leaked token cannot ask about another tenant's schedule. When the edge has no
//! shared secret the binding is not enforced, as for `run_page`.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use async_trait::async_trait;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};

use quarry_core::envelope::Envelope;
use quarry_core::error::{ErrorCode, QuarryError};

use crate::limits::{Allowance, OrgLimits};
use crate::state::AppState;

// =============================================================================
// Configuration
// =============================================================================

pub const ENV_SCHEDULE_POSTURE: &str = "QUARRY_EDGE__SCHEDULE_POSTURE";
pub const ENV_ORG_CORE_URL: &str = "QUARRY_EDGE__ORG_CORE_URL";
pub const ENV_ORG_CORE_SERVICE_TOKEN: &str = "QUARRY_EDGE__ORG_CORE_SERVICE_TOKEN";

/// The principal org-core knows this edge by (W2-CP-R13-2 registry row).
pub const ORG_CORE_SERVICE_ID: &str = "quarry-edge";
/// The posture read's bound. No retry: the next cron fire is the retry.
pub const POSTURE_TIMEOUT: Duration = Duration::from_secs(3);

/// How the preflight answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostureMode {
    /// The S2 answer: allow, posture `unchecked`, entitlement `floor`. No reads.
    Off,
    /// Every check runs and is logged; the fire is allowed and nothing is paused.
    Observe,
    /// A skip reason denies the fire; `skipped_zdr` also pauses the schedule.
    Enforce,
}

impl PostureMode {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "" | "observe" => Ok(Self::Observe),
            "off" => Ok(Self::Off),
            "enforce" => Ok(Self::Enforce),
            other => Err(format!(
                "{ENV_SCHEDULE_POSTURE}={other:?} is not one of off, observe, enforce"
            )),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Observe => "observe",
            Self::Enforce => "enforce",
        }
    }
}

/// The process-wide preflight configuration, read from the environment once.
struct PreflightConfig {
    mode: PostureMode,
    /// `None` when the org-core URL or token is unset: every posture read is then
    /// `unknown` (fail closed under `enforce`).
    posture: Option<Arc<dyn PostureSource>>,
}

static CONFIG: OnceLock<PreflightConfig> = OnceLock::new();

fn config() -> &'static PreflightConfig {
    CONFIG.get_or_init(|| {
        let raw = std::env::var(ENV_SCHEDULE_POSTURE).unwrap_or_default();
        let mode = PostureMode::parse(&raw).unwrap_or_else(|_| {
            // The parse error echoes the raw value; log a closed label instead.
            tracing::warn!(
                error_class = "invalid_mode",
                switch = ENV_SCHEDULE_POSTURE,
                "schedule preflight: invalid mode; using observe"
            );
            PostureMode::Observe
        });
        let url = std::env::var(ENV_ORG_CORE_URL).unwrap_or_default();
        let token = std::env::var(ENV_ORG_CORE_SERVICE_TOKEN).unwrap_or_default();
        let posture: Option<Arc<dyn PostureSource>> = if url.trim().is_empty() || token.trim().is_empty() {
            if mode != PostureMode::Off {
                tracing::warn!(
                    mode = mode.label(),
                    "schedule preflight: org-core URL or token unset; posture reads answer unknown"
                );
            }
            None
        } else {
            match OrgCorePosture::new(url.trim(), token.trim()) {
                Ok(client) => Some(Arc::new(client)),
                Err(class) => {
                    tracing::warn!(class, "schedule preflight: org-core client not built; posture reads answer unknown");
                    None
                }
            }
        };
        tracing::info!(mode = mode.label(), posture_wired = posture.is_some(), "schedule preflight configured");
        PreflightConfig { mode, posture }
    })
}

// =============================================================================
// Closed vocabularies
// =============================================================================

/// Posture values: `unchecked` (mode off), `standard`, `zdr`, `unknown`.
pub const POSTURE_UNCHECKED: &str = "unchecked";
pub const POSTURE_STANDARD: &str = "standard";
pub const POSTURE_ZDR: &str = "zdr";
pub const POSTURE_UNKNOWN: &str = "unknown";

/// Entitlement values: the [`LimitsOrigin`](crate::limits::LimitsOrigin) labels
/// (`floor`, `open`, `entitlement`) plus `unknown` when the resolver refused.
pub const ENTITLEMENT_FLOOR: &str = "floor";
pub const ENTITLEMENT_UNKNOWN: &str = "unknown";

/// Skip reasons (closed set, shared with the orchestrator and control).
pub const SKIPPED_ZDR: &str = "skipped_zdr";
pub const SKIPPED_POSTURE_UNKNOWN: &str = "skipped_posture_unknown";
pub const SKIPPED_NO_ENTITLEMENT: &str = "skipped_no_entitlement";
pub const SKIPPED_OVERLAP: &str = "skipped_overlap";
pub const SKIPPED_KIND_DISABLED: &str = "skipped_kind_disabled";
pub const SKIPPED_DISABLED: &str = "skipped_disabled";

/// Every skip reason, in one place for tests and readers.
pub const SKIP_REASONS: [&str; 6] = [
    SKIPPED_ZDR,
    SKIPPED_POSTURE_UNKNOWN,
    SKIPPED_NO_ENTITLEMENT,
    SKIPPED_OVERLAP,
    SKIPPED_KIND_DISABLED,
    SKIPPED_DISABLED,
];

/// The schedule kinds a fire may run.
const SCHEDULE_KINDS: [&str; 4] = ["scrape", "crawl", "batch", "change_monitor"];

// =============================================================================
// Wire shapes
// =============================================================================

/// The fire-time question: which schedule, and which tenant it belongs to.
/// `target_kind` and `enabled` are optional; today's orchestrator sends neither.
#[derive(Debug, Deserialize)]
pub struct PreflightRequest {
    pub schedule_id: String,
    pub org_id: String,
    #[serde(default)]
    pub target_kind: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
}

/// The answer. `posture`, `entitlement` and `reason` are closed vocabularies
/// (see the constants above). `reason` is set only on a denied fire;
/// `observed_reason` carries the reason `observe` mode would have denied with.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct PreflightResponse {
    pub allow: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
    pub posture: &'static str,
    pub entitlement: &'static str,
    /// Effective ZDR when the posture was read; absent when unchecked or unknown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zdr: Option<bool>,
    /// What the organization has left of the kind's metered allowance, when one
    /// is configured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clamped_units: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_reason: Option<&'static str>,
}

/// The S2 answer, still served under mode `off`.
pub fn s2_answer() -> PreflightResponse {
    PreflightResponse {
        allow: true,
        reason: None,
        posture: POSTURE_UNCHECKED,
        entitlement: ENTITLEMENT_FLOOR,
        zdr: None,
        clamped_units: None,
        observed_reason: None,
    }
}

/// The canonical run-binding target for a schedule: `schedule:<id>`. The
/// orchestrator signs exactly this string (`runBindingSig(secret, org, "schedule:"+id)`).
pub fn binding_target(schedule_id: &str) -> String {
    format!("schedule:{schedule_id}")
}

// =============================================================================
// Posture source (org-core)
// =============================================================================

/// One posture read. `Unknown` carries a closed class for the log, never a body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostureRead {
    Known { zdr_intent: bool, entitled: bool },
    Unknown(&'static str),
}

impl PostureRead {
    /// Effective ZDR: intent AND entitlement. `None` when unknown.
    pub fn effective_zdr(self) -> Option<bool> {
        match self {
            Self::Known { zdr_intent, entitled } => Some(zdr_intent && entitled),
            Self::Unknown(_) => None,
        }
    }
}

/// Where the fire-time posture comes from. Implementations must not cache.
#[async_trait]
pub trait PostureSource: Send + Sync {
    async fn read(&self, org_id: &str) -> PostureRead;
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RetentionPostureDoc {
    zdr_intent: bool,
    entitled: bool,
}

/// org-core's `GET /internal/orgs/:orgId/retention-posture`, authenticated with
/// the `quarry-edge` static credential (`X-Service-Id` / `X-Service-Token`,
/// scope `org:retention:read:any`). No cache: every call is a read.
pub struct OrgCorePosture {
    client: reqwest::Client,
    base_url: String,
    token: String,
}

impl OrgCorePosture {
    pub fn new(base_url: &str, token: &str) -> Result<Self, &'static str> {
        reqwest::Url::parse(base_url).map_err(|_| "config")?;
        let client = reqwest::Client::builder()
            .timeout(POSTURE_TIMEOUT)
            .connect_timeout(POSTURE_TIMEOUT)
            .build()
            .map_err(|_| "client")?;
        Ok(Self {
            client,
            base_url: base_url.to_string(),
            token: token.to_string(),
        })
    }

    fn url_for(&self, org_id: &str) -> Result<reqwest::Url, &'static str> {
        let mut url = reqwest::Url::parse(&self.base_url).map_err(|_| "config")?;
        url.path_segments_mut()
            .map_err(|_| "config")?
            .pop_if_empty()
            .extend(["internal", "orgs", org_id, "retention-posture"]);
        Ok(url)
    }
}

#[async_trait]
impl PostureSource for OrgCorePosture {
    async fn read(&self, org_id: &str) -> PostureRead {
        let url = match self.url_for(org_id) {
            Ok(url) => url,
            Err(class) => return PostureRead::Unknown(class),
        };
        let response = match self
            .client
            .get(url)
            .header("x-service-id", ORG_CORE_SERVICE_ID)
            .header("x-service-token", &self.token)
            .header("accept", "application/json")
            .header("cache-control", "no-store")
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) if e.is_timeout() => return PostureRead::Unknown("timeout"),
            Err(_) => return PostureRead::Unknown("unreachable"),
        };
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            return PostureRead::Unknown(match status {
                401 => "unauthorized",
                403 => "forbidden",
                404 => "not_found",
                500..=599 => "unavailable",
                _ => "rejected",
            });
        }
        match response.json::<RetentionPostureDoc>().await {
            Ok(doc) => PostureRead::Known {
                zdr_intent: doc.zdr_intent,
                entitled: doc.entitled,
            },
            Err(_) => PostureRead::Unknown("decode"),
        }
    }
}

// =============================================================================
// The decision
// =============================================================================

/// The kind's metered allowance, when the kind has one.
fn kind_allowance(kind: Option<&str>, limits: &OrgLimits) -> Option<Allowance> {
    match kind {
        Some("crawl") => Some(limits.crawl_pages),
        Some("batch") => Some(limits.batch_urls),
        _ => None,
    }
}

/// The checked answer, before the mode is applied. Order: disabled, kind,
/// posture (ZDR, then unknown), entitlement. The first skip reason wins.
pub fn evaluate(
    req: &PreflightRequest,
    posture: PostureRead,
    limits: &Result<OrgLimits, QuarryError>,
) -> (Option<&'static str>, PreflightResponse) {
    let kind = req.target_kind.as_deref().map(str::trim).filter(|k| !k.is_empty());

    let (posture_label, zdr) = match posture.effective_zdr() {
        Some(true) => (POSTURE_ZDR, Some(true)),
        Some(false) => (POSTURE_STANDARD, Some(false)),
        None => (POSTURE_UNKNOWN, None),
    };

    let (entitlement, clamped_units, spent) = match limits {
        Ok(l) => {
            let allowance = kind_allowance(kind, l);
            let clamped = allowance.and_then(Allowance::remaining);
            let spent = matches!(allowance, Some(Allowance::Remaining(0)));
            (l.origin.label(), clamped, spent)
        }
        Err(_) => (ENTITLEMENT_UNKNOWN, None, true),
    };

    let reason = if req.enabled == Some(false) {
        Some(SKIPPED_DISABLED)
    } else if kind.is_some_and(|k| !SCHEDULE_KINDS.contains(&k)) {
        Some(SKIPPED_KIND_DISABLED)
    } else if zdr == Some(true) {
        Some(SKIPPED_ZDR)
    } else if zdr.is_none() {
        Some(SKIPPED_POSTURE_UNKNOWN)
    } else if spent {
        Some(SKIPPED_NO_ENTITLEMENT)
    } else {
        None
    };

    (
        reason,
        PreflightResponse {
            allow: reason.is_none(),
            reason,
            posture: posture_label,
            entitlement,
            zdr,
            clamped_units,
            observed_reason: None,
        },
    )
}

/// Applies the mode to a checked answer. `observe` allows and reports the
/// reason it would have denied with; `enforce` denies.
pub fn apply_mode(mode: PostureMode, reason: Option<&'static str>, checked: PreflightResponse) -> PreflightResponse {
    match mode {
        PostureMode::Off => s2_answer(),
        PostureMode::Enforce => checked,
        PostureMode::Observe => PreflightResponse {
            allow: true,
            reason: None,
            observed_reason: reason,
            ..checked
        },
    }
}

/// The whole preflight for one request: reads (never cached) and the decision.
/// Kept apart from the handler so the table tests drive it with fakes.
pub async fn decide(
    mode: PostureMode,
    req: &PreflightRequest,
    posture: Option<&dyn PostureSource>,
    limits: &dyn crate::limits::LimitsResolver,
) -> PreflightResponse {
    if mode == PostureMode::Off {
        return s2_answer();
    }
    let read = match posture {
        Some(source) => source.read(&req.org_id).await,
        None => PostureRead::Unknown("not_configured"),
    };
    if let PostureRead::Unknown(class) = read {
        tracing::warn!(class, mode = mode.label(), "schedule preflight: posture unknown");
    }
    let resolved = limits.org_limits(&req.org_id).await;
    let (reason, checked) = evaluate(req, read, &resolved);
    if let Some(reason) = reason {
        tracing::info!(
            reason,
            mode = mode.label(),
            posture = checked.posture,
            entitlement = checked.entitlement,
            "schedule preflight: skip reason"
        );
    }
    apply_mode(mode, reason, checked)
}

// =============================================================================
// Route
// =============================================================================

/// Refuses a request whose identifiers cannot be signed unambiguously.
fn validate(req: &PreflightRequest) -> Result<(), QuarryError> {
    if req.org_id.trim().is_empty() {
        return Err(QuarryError::new(
            ErrorCode::BadRequest,
            "preflight requires org_id in the body",
        ));
    }
    if req.schedule_id.trim().is_empty() || req.schedule_id.len() > 128 {
        return Err(QuarryError::new(
            ErrorCode::BadRequest,
            "preflight requires a schedule_id of at most 128 characters",
        ));
    }
    if req.org_id.contains('\n') || req.schedule_id.contains('\n') {
        return Err(QuarryError::new(
            ErrorCode::BadRequest,
            "identifiers may not contain a newline",
        ));
    }
    Ok(())
}

fn refuse(
    request_id: &str,
    err: QuarryError,
) -> (StatusCode, Json<Envelope<()>>) {
    (
        StatusCode::from_u16(err.code.http_status()).unwrap_or(StatusCode::BAD_REQUEST),
        Json(Envelope::<()>::err(request_id, err)),
    )
}

/// The route handler. Runs the runtime-token check first, then validates the
/// body, then checks the run binding when the edge has a shared secret, then
/// decides by the configured mode.
pub async fn preflight(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<PreflightRequest>,
) -> Result<Json<PreflightResponse>, (StatusCode, Json<Envelope<()>>)> {
    let request_id = crate::request_id::current();
    crate::routes::verify_runtime_token(&headers).map_err(|e| refuse(&request_id, e))?;
    validate(&req).map_err(|e| refuse(&request_id, e))?;

    if let Some(signer) = state.internal_signer.as_ref() {
        let sig = headers
            .get("x-quarry-run-sig")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let target = binding_target(&req.schedule_id);
        if !signer.verify_run_binding(&req.org_id, &target, sig) {
            tracing::warn!(had_sig = !sig.is_empty(), "schedule preflight binding failed");
            let err = QuarryError::new(ErrorCode::Unauthorized, "preflight org binding invalid");
            return Err(refuse(&request_id, err));
        }
    }

    let cfg = config();
    let answer = decide(cfg.mode, &req, cfg.posture.as_deref(), state.limits.as_ref()).await;

    // skipped_zdr also pauses the schedule through control. Enforce only;
    // observe logs what it would have paused. Best effort and off the fire's
    // path: a failed pause is retried by the next fire's preflight.
    match (cfg.mode, answer.reason, answer.observed_reason) {
        (PostureMode::Enforce, Some(SKIPPED_ZDR), _) => {
            let state = state.clone();
            let schedule_id = req.schedule_id.clone();
            let org_id = req.org_id.clone();
            tokio::spawn(async move {
                if let Err(e) =
                    crate::schedule_routes::pause_for_posture(&state, &schedule_id, &org_id).await
                {
                    tracing::warn!(code = ?e.code, "schedule preflight: pause after skipped_zdr failed");
                }
            });
        }
        (PostureMode::Observe, _, Some(SKIPPED_ZDR)) => {
            tracing::info!("schedule preflight: would_pause (observe)");
        }
        _ => {}
    }

    Ok(Json(answer))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::{LimitsOrigin, LimitsResolver, StaticFloor};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn req(kind: Option<&str>, enabled: Option<bool>) -> PreflightRequest {
        PreflightRequest {
            schedule_id: "sch_1".into(),
            org_id: "org_1".into(),
            target_kind: kind.map(str::to_owned),
            enabled,
        }
    }

    const STANDARD: PostureRead = PostureRead::Known { zdr_intent: false, entitled: true };
    const ZDR: PostureRead = PostureRead::Known { zdr_intent: true, entitled: true };
    const INTENT_NOT_ENTITLED: PostureRead = PostureRead::Known { zdr_intent: true, entitled: false };
    const UNKNOWN: PostureRead = PostureRead::Unknown("timeout");

    fn entitled(crawl: Allowance) -> OrgLimits {
        OrgLimits {
            origin: LimitsOrigin::Entitlement,
            crawl_pages: crawl,
            ..OrgLimits::floor()
        }
    }

    fn refused() -> Result<OrgLimits, QuarryError> {
        Err(QuarryError::new(ErrorCode::RuntimeNotReady, "organization limits are unreadable"))
    }

    /// A posture source that counts its reads and answers a fixed value.
    struct CountingPosture {
        answer: PostureRead,
        reads: AtomicUsize,
    }

    #[async_trait]
    impl PostureSource for CountingPosture {
        async fn read(&self, _org_id: &str) -> PostureRead {
            self.reads.fetch_add(1, Ordering::SeqCst);
            self.answer
        }
    }

    struct RefusingLimits;

    #[async_trait]
    impl LimitsResolver for RefusingLimits {
        async fn org_limits(&self, _org_id: &str) -> Result<OrgLimits, QuarryError> {
            refused()
        }
    }

    #[test]
    fn s2_answer_allows_with_the_floor_entitlement() {
        let a = s2_answer();
        assert!(a.allow);
        assert_eq!(a.posture, "unchecked");
        assert_eq!(a.entitlement, "floor");
        assert_eq!(a.reason, None);
    }

    #[test]
    fn s2_answer_wire_shape_is_unchanged() {
        let v = serde_json::to_value(s2_answer()).unwrap();
        assert_eq!(
            v,
            serde_json::json!({"allow": true, "posture": "unchecked", "entitlement": "floor"})
        );
    }

    #[test]
    fn binding_target_is_schedule_prefixed() {
        assert_eq!(binding_target("sch_abc"), "schedule:sch_abc");
    }

    #[test]
    fn validate_refuses_newlines_and_empty_identifiers() {
        assert!(validate(&req(None, None)).is_ok());
        let mut empty_org = req(None, None);
        empty_org.org_id = " ".into();
        assert!(validate(&empty_org).is_err());
        let mut newline = req(None, None);
        newline.schedule_id = "sch_1\nx".into();
        assert!(validate(&newline).is_err());
    }

    #[test]
    fn request_without_optional_fields_still_decodes() {
        let r: PreflightRequest =
            serde_json::from_str(r#"{"schedule_id":"sch_1","org_id":"org_1"}"#).unwrap();
        assert_eq!(r.target_kind, None);
        assert_eq!(r.enabled, None);
    }

    #[test]
    fn mode_parses_and_defaults_to_observe() {
        assert_eq!(PostureMode::parse("").unwrap(), PostureMode::Observe);
        assert_eq!(PostureMode::parse("observe").unwrap(), PostureMode::Observe);
        assert_eq!(PostureMode::parse(" OFF ").unwrap(), PostureMode::Off);
        assert_eq!(PostureMode::parse("enforce").unwrap(), PostureMode::Enforce);
        assert!(PostureMode::parse("shadow").is_err());
    }

    /// The preflight table: (request, posture, limits) -> enforced reason, posture, entitlement.
    #[test]
    fn preflight_table() {
        type Row = (
            &'static str,
            PreflightRequest,
            PostureRead,
            Result<OrgLimits, QuarryError>,
            Option<&'static str>,
            &'static str,
            &'static str,
        );
        let rows: Vec<Row> = vec![
            ("standard floor allows", req(Some("crawl"), None), STANDARD, Ok(OrgLimits::floor()), None, POSTURE_STANDARD, "floor"),
            ("zdr denies", req(Some("scrape"), None), ZDR, Ok(OrgLimits::floor()), Some(SKIPPED_ZDR), POSTURE_ZDR, "floor"),
            ("intent without entitlement is standard", req(None, None), INTENT_NOT_ENTITLED, Ok(OrgLimits::floor()), None, POSTURE_STANDARD, "floor"),
            ("unknown posture fails closed", req(Some("crawl"), None), UNKNOWN, Ok(OrgLimits::floor()), Some(SKIPPED_POSTURE_UNKNOWN), POSTURE_UNKNOWN, "floor"),
            ("spent crawl allowance", req(Some("crawl"), None), STANDARD, Ok(entitled(Allowance::Remaining(0))), Some(SKIPPED_NO_ENTITLEMENT), POSTURE_STANDARD, "entitlement"),
            ("spent crawl allowance does not stop a scrape", req(Some("scrape"), None), STANDARD, Ok(entitled(Allowance::Remaining(0))), None, POSTURE_STANDARD, "entitlement"),
            ("resolver refusal fails closed", req(Some("batch"), None), STANDARD, refused(), Some(SKIPPED_NO_ENTITLEMENT), POSTURE_STANDARD, ENTITLEMENT_UNKNOWN),
            ("disabled wins over zdr", req(Some("crawl"), Some(false)), ZDR, Ok(OrgLimits::floor()), Some(SKIPPED_DISABLED), POSTURE_ZDR, "floor"),
            ("unknown kind", req(Some("teleport"), None), STANDARD, Ok(OrgLimits::floor()), Some(SKIPPED_KIND_DISABLED), POSTURE_STANDARD, "floor"),
            ("zdr wins over unknown limits", req(Some("crawl"), None), ZDR, refused(), Some(SKIPPED_ZDR), POSTURE_ZDR, ENTITLEMENT_UNKNOWN),
        ];
        for (name, r, posture, limits, want_reason, want_posture, want_ent) in rows {
            let (reason, checked) = evaluate(&r, posture, &limits);
            assert_eq!(reason, want_reason, "{name}: reason");
            let enforced = apply_mode(PostureMode::Enforce, reason, checked);
            assert_eq!(enforced.allow, want_reason.is_none(), "{name}: allow");
            assert_eq!(enforced.reason, want_reason, "{name}: enforced reason");
            assert_eq!(enforced.posture, want_posture, "{name}: posture");
            assert_eq!(enforced.entitlement, want_ent, "{name}: entitlement");
            if let Some(reason) = want_reason {
                assert!(SKIP_REASONS.contains(&reason), "{name}: closed set");
            }
        }
    }

    #[test]
    fn preflight_entitlement_floor_is_reported() {
        let (_, checked) = evaluate(&req(Some("crawl"), None), STANDARD, &Ok(OrgLimits::floor()));
        assert_eq!(checked.entitlement, "floor");
        assert_eq!(checked.clamped_units, None);
        let (_, checked) = evaluate(&req(Some("crawl"), None), STANDARD, &Ok(entitled(Allowance::Remaining(40))));
        assert_eq!(checked.entitlement, "entitlement");
        assert_eq!(checked.clamped_units, Some(40));
    }

    #[tokio::test]
    async fn preflight_unknown_posture_fails_closed() {
        let src = CountingPosture { answer: UNKNOWN, reads: AtomicUsize::new(0) };
        let a = decide(PostureMode::Enforce, &req(Some("crawl"), None), Some(&src), &StaticFloor).await;
        assert!(!a.allow);
        assert_eq!(a.reason, Some(SKIPPED_POSTURE_UNKNOWN));
        assert_eq!(a.posture, POSTURE_UNKNOWN);
        assert_eq!(a.zdr, None);
        // No org-core configured at all is unknown too.
        let a = decide(PostureMode::Enforce, &req(None, None), None, &StaticFloor).await;
        assert_eq!(a.reason, Some(SKIPPED_POSTURE_UNKNOWN));
    }

    #[tokio::test]
    async fn preflight_denies_zdr_org_under_enforce() {
        let src = CountingPosture { answer: ZDR, reads: AtomicUsize::new(0) };
        let a = decide(PostureMode::Enforce, &req(Some("crawl"), None), Some(&src), &StaticFloor).await;
        assert!(!a.allow);
        assert_eq!(a.reason, Some(SKIPPED_ZDR));
        assert_eq!(a.zdr, Some(true));
    }

    #[tokio::test]
    async fn preflight_observe_mode_allows_and_logs() {
        let src = CountingPosture { answer: ZDR, reads: AtomicUsize::new(0) };
        let a = decide(PostureMode::Observe, &req(Some("crawl"), None), Some(&src), &StaticFloor).await;
        assert!(a.allow);
        assert_eq!(a.reason, None);
        assert_eq!(a.observed_reason, Some(SKIPPED_ZDR));
        assert_eq!(a.posture, POSTURE_ZDR);
        assert_eq!(src.reads.load(Ordering::SeqCst), 1);

        let a = decide(PostureMode::Observe, &req(None, None), None, &RefusingLimits).await;
        assert!(a.allow);
        assert_eq!(a.observed_reason, Some(SKIPPED_POSTURE_UNKNOWN));
    }

    #[tokio::test]
    async fn preflight_off_mode_reads_nothing() {
        let src = CountingPosture { answer: ZDR, reads: AtomicUsize::new(0) };
        let a = decide(PostureMode::Off, &req(None, None), Some(&src), &RefusingLimits).await;
        assert_eq!(a, s2_answer());
        assert_eq!(src.reads.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn preflight_never_caches_posture() {
        let src = CountingPosture { answer: STANDARD, reads: AtomicUsize::new(0) };
        for _ in 0..3 {
            let a = decide(PostureMode::Enforce, &req(None, None), Some(&src), &StaticFloor).await;
            assert!(a.allow);
        }
        assert_eq!(src.reads.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn org_core_posture_reads_every_time_with_the_service_credential() {
        use wiremock::matchers::{header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/internal/orgs/org_1/retention-posture"))
            .and(header("x-service-id", "quarry-edge"))
            .and(header("x-service-token", "test-token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"zdrIntent": true, "entitled": true})),
            )
            .expect(2)
            .mount(&server)
            .await;

        let client = OrgCorePosture::new(&server.uri(), "test-token").unwrap();
        for _ in 0..2 {
            let read = client.read("org_1").await;
            assert_eq!(read, PostureRead::Known { zdr_intent: true, entitled: true });
        }
        server.verify().await;
    }

    #[tokio::test]
    async fn org_core_posture_errors_are_unknown() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/internal/orgs/org_404/retention-posture"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/internal/orgs/org_500/retention-posture"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/internal/orgs/org_bad/retention-posture"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
            .mount(&server)
            .await;

        let client = OrgCorePosture::new(&server.uri(), "test-token").unwrap();
        assert_eq!(client.read("org_404").await, PostureRead::Unknown("not_found"));
        assert_eq!(client.read("org_500").await, PostureRead::Unknown("unavailable"));
        assert_eq!(client.read("org_bad").await, PostureRead::Unknown("decode"));

        // Nothing listening: unknown, not a panic and not "standard".
        let dead = OrgCorePosture::new("http://127.0.0.1:9", "test-token").unwrap();
        assert!(matches!(dead.read("org_1").await, PostureRead::Unknown(_)));
    }
}
