//! The edge half of the AI formats of `POST /v1/scrape` (INNH-1 U3): the switch,
//! who may ask, whether the lane is there, and the call into the engine.
//!
//! The contract is `quarry_core::ai_spec` (U1), the engine is
//! `quarry_runtime::ai_scrape` (U2). This module decides *whether* to call the
//! engine and builds it from the edge's own state; it holds no prompt, no schema
//! and no page text beyond the call.
//!
//! - **Dark by default.** `QUARRY_EDGE__AI_FORMATS` must be `on`, `1` or `true`.
//!   Anything else (a typo included) leaves every AI format a 400
//!   `feature_disabled` and changes nothing else. Read once per process.
//! - **ZDR refuses** (ADR-4). Under effective ZDR no model is called and every
//!   requested AI format is answered `skipped` / `ZDR_UNAVAILABLE` inside a normal
//!   200. `QUARRY_EDGE__AI_FORMATS_ZDR=allow` is reserved: it is logged once as
//!   ignored and behaves as `refuse`.
//! - **Who may ask.** A user token. A service principal needs the `extract:ai`
//!   scope, and a request labelled `initiated_by: "agent"` is refused whatever the
//!   token (a Model-relayed scrape must not spend model money outside its run
//!   budget). Both are a 403 before the page is fetched.
//! - **The lane.** The minted path first (WS16.S3): when the edge holds the
//!   Auth Core service-token provider (`state.service_token_provider`) the client
//!   mints a Model Plane token per verified request org. Otherwise the static
//!   development credential of the edge (`state.model_plane_token`, which
//!   `main.rs` sets only under the auth dev bypass) plus an optional second dev
//!   token for the inference audience. With no `MODEL_PLANE_URL` the answer is
//!   501 `model_plane_unavailable`; with a URL but neither credential, 501
//!   `model_lane_not_granted`. Both are decided before the page is fetched.
//! - **Nothing here logs content.** The only log line is the boot-style warning
//!   for `AI_FORMATS_ZDR=allow`.

use std::sync::{Arc, Mutex, OnceLock};

use quarry_core::ai_spec::{AiCode, AiFormatReport, AiReason, AiReport, AiSpec};
use quarry_core::error::{ErrorCode, QuarryError};
use quarry_runtime::ai_scrape::{
    AiFields, AiScrapeConfig, AiScrapeEngine, AiScrapeInput, DEFAULT_INPUT_CHARS,
    DEFAULT_MAX_CALLS, DEFAULT_MAX_TOTAL_TOKENS, DEFAULT_MODEL_JSON, DEFAULT_MODEL_TEXT,
    MAX_INPUT_CHARS, MIN_INPUT_CHARS,
};
use quarry_runtime::mp_client::ModelPlaneClient;
use serde_json::Value;
use tokio::sync::Semaphore;

use crate::auth::{Claims, SCOPE_EXTRACT_AI};
use crate::state::AppState;

pub const ENV_AI_FORMATS: &str = "QUARRY_EDGE__AI_FORMATS";
pub const ENV_AI_FORMATS_ZDR: &str = "QUARRY_EDGE__AI_FORMATS_ZDR";
pub const ENV_MODEL_JSON: &str = "QUARRY_EDGE__AI_FORMATS_MODEL_JSON";
pub const ENV_MODEL_TEXT: &str = "QUARRY_EDGE__AI_FORMATS_MODEL_TEXT";
pub const ENV_MAX_INPUT_CHARS: &str = "QUARRY_EDGE__AI_FORMATS_MAX_INPUT_CHARS";
pub const ENV_MAX_TOTAL_TOKENS: &str = "QUARRY_EDGE__AI_FORMATS_MAX_TOTAL_TOKENS";
pub const ENV_MAX_CALLS: &str = "QUARRY_EDGE__AI_FORMATS_MAX_CALLS";
pub const ENV_CONCURRENCY: &str = "QUARRY_EDGE__AI_FORMATS_CONCURRENCY";
pub const ENV_INFERENCE_TOKEN: &str = "QUARRY_EDGE__MODEL_PLANE_INFERENCE_TOKEN";

/// Every name this module reads; a test pins that the code reads exactly these.
#[cfg(test)]
const ENV_NAMES: [&str; 9] = [
    ENV_AI_FORMATS,
    ENV_AI_FORMATS_ZDR,
    ENV_MODEL_JSON,
    ENV_MODEL_TEXT,
    ENV_MAX_INPUT_CHARS,
    ENV_MAX_TOTAL_TOKENS,
    ENV_MAX_CALLS,
    ENV_CONCURRENCY,
    ENV_INFERENCE_TOKEN,
];

/// The process-wide cap on model calls in flight, across requests.
pub const PROCESS_CONCURRENCY: usize = 16;

/// What the operator configured, read once.
#[derive(Clone, PartialEq, Eq)]
pub struct AiFormatsSettings {
    /// The switch.
    pub enabled: bool,
    /// `AI_FORMATS_ZDR=allow` was set. Reserved: it is ignored (ADR-4 needs an
    /// amendment first), so ZDR always refuses.
    pub zdr_allow_requested: bool,
    pub model_json: String,
    pub model_text: String,
    pub max_input_chars: usize,
    pub max_total_tokens: u64,
    pub max_calls: u32,
    pub concurrency: usize,
    /// Development bypass only: the token for the inference audience.
    pub inference_token: Option<String>,
}

// The token is a credential: it never reaches a log or a debug dump.
impl std::fmt::Debug for AiFormatsSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AiFormatsSettings")
            .field("enabled", &self.enabled)
            .field("zdr_allow_requested", &self.zdr_allow_requested)
            .field("model_json", &self.model_json)
            .field("model_text", &self.model_text)
            .field("max_input_chars", &self.max_input_chars)
            .field("max_total_tokens", &self.max_total_tokens)
            .field("max_calls", &self.max_calls)
            .field("concurrency", &self.concurrency)
            .field(
                "inference_token",
                &self.inference_token.as_ref().map(|_| "<set>"),
            )
            .finish()
    }
}

fn number<T: std::str::FromStr>(value: Option<String>) -> Option<T> {
    value.and_then(|raw| raw.trim().parse::<T>().ok())
}

fn non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|raw| raw.trim().to_owned())
        .filter(|raw| !raw.is_empty())
}

impl AiFormatsSettings {
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        // Only these values turn it on. A typo never widens the endpoint.
        let enabled = matches!(
            lookup(ENV_AI_FORMATS)
                .map(|raw| raw.trim().to_ascii_lowercase())
                .as_deref(),
            Some("on" | "1" | "true")
        );
        let zdr_allow_requested = lookup(ENV_AI_FORMATS_ZDR)
            .map(|raw| raw.trim().eq_ignore_ascii_case("allow"))
            .unwrap_or(false);
        Self {
            enabled,
            zdr_allow_requested,
            model_json: non_empty(lookup(ENV_MODEL_JSON))
                .unwrap_or_else(|| DEFAULT_MODEL_JSON.to_owned()),
            model_text: non_empty(lookup(ENV_MODEL_TEXT))
                .unwrap_or_else(|| DEFAULT_MODEL_TEXT.to_owned()),
            // The ceiling can only be lowered: a value above it is the ceiling.
            max_input_chars: number::<usize>(lookup(ENV_MAX_INPUT_CHARS))
                .map(|value| value.clamp(MIN_INPUT_CHARS, MAX_INPUT_CHARS))
                .unwrap_or(DEFAULT_INPUT_CHARS),
            max_total_tokens: number::<u64>(lookup(ENV_MAX_TOTAL_TOKENS))
                .map(|value| value.max(1_000))
                .unwrap_or(DEFAULT_MAX_TOTAL_TOKENS),
            max_calls: number::<u32>(lookup(ENV_MAX_CALLS))
                .map(|value| value.clamp(1, DEFAULT_MAX_CALLS))
                .unwrap_or(DEFAULT_MAX_CALLS),
            concurrency: number::<usize>(lookup(ENV_CONCURRENCY))
                .map(|value| value.clamp(1, quarry_runtime::ai_scrape::DEFAULT_CONCURRENCY))
                .unwrap_or(quarry_runtime::ai_scrape::DEFAULT_CONCURRENCY),
            inference_token: non_empty(lookup(ENV_INFERENCE_TOKEN)),
        }
    }

    /// The engine's config from these settings.
    pub fn engine_config(&self) -> AiScrapeConfig {
        AiScrapeConfig {
            model_json: self.model_json.clone(),
            model_text: self.model_text.clone(),
            max_input_chars: self.max_input_chars,
            max_total_tokens: self.max_total_tokens,
            max_calls: self.max_calls,
            concurrency: self.concurrency,
            ..AiScrapeConfig::default()
        }
        .clamped()
    }
}

/// The process settings, read once from the environment.
pub fn settings() -> &'static AiFormatsSettings {
    static SETTINGS: OnceLock<AiFormatsSettings> = OnceLock::new();
    SETTINGS.get_or_init(|| {
        let settings = AiFormatsSettings::from_lookup(|name| std::env::var(name).ok());
        if settings.zdr_allow_requested {
            tracing::warn!(
                "QUARRY_EDGE__AI_FORMATS_ZDR=allow is reserved and ignored: AI formats are refused under ZDR"
            );
        }
        settings
    })
}

/// Whether the switch is on.
pub fn enabled() -> bool {
    settings().enabled
}

/// Whether `formats` (the raw request value) names an AI format: an object whose
/// `type` is one of the four, or the string `summary`. Works on the raw value, so
/// the stream route can refuse without parsing and without the switch.
pub fn requests_ai_format(formats: Option<&Value>) -> bool {
    first_ai_index(formats).is_some()
}

/// The index of the first AI format in the raw `formats` array.
pub fn first_ai_index(formats: Option<&Value>) -> Option<usize> {
    formats?.as_array()?.iter().position(|entry| match entry {
        Value::String(name) => name == "summary",
        Value::Object(object) => object
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|name| matches!(name, "json" | "summary" | "question" | "highlights")),
        _ => false,
    })
}

/// The 403 for a request that may not use AI formats, decided before the page is
/// fetched. A user token passes; a service principal needs `extract:ai`; a request
/// labelled `initiated_by: "agent"` never passes. It only tightens.
pub fn check_principal(
    claims: &Claims,
    initiated_by: Option<&str>,
    format_index: Option<usize>,
) -> Result<(), QuarryError> {
    let refused = initiated_by == Some("agent")
        || (claims.is_service() && !claims.has_scope(SCOPE_EXTRACT_AI));
    if !refused {
        return Ok(());
    }
    let field = format!("formats[{}]", format_index.unwrap_or(0));
    Err(QuarryError::new(
        ErrorCode::Forbidden,
        "AI formats need a user request, or a service principal with the extract:ai scope",
    )
    .with_details(serde_json::json!({
        "reason": "extract_ai_scope_required",
        "field": field,
    })))
}

/// The 501 for an edge that cannot reach a model, decided before the page is
/// fetched. `model_plane_unavailable`: no `MODEL_PLANE_URL`.
/// `model_lane_not_granted`: a URL, but no credential Quarry may use on the chat
/// lane: neither the minted service-token provider nor the development token.
pub fn availability(state: &AppState) -> Result<(), QuarryError> {
    let has_url = state
        .model_plane_url
        .as_deref()
        .is_some_and(|url| !url.trim().is_empty());
    if !has_url {
        return Err(QuarryError::new(
            ErrorCode::Unsupported,
            "AI formats need the Model Plane, and QUARRY_EDGE__MODEL_PLANE_URL is not set on this endpoint",
        )
        .with_details(serde_json::json!({ "reason": "model_plane_unavailable" })));
    }
    // The minted lane (service-token provider) or the dev token: either one is a
    // usable credential, as in `extract_routes`.
    let has_credential = lane_credential(state).is_some();
    if !has_credential {
        return Err(QuarryError::new(
            ErrorCode::Unsupported,
            "AI formats are not available: this endpoint holds no credential for the Model Plane chat lane",
        )
        .with_details(serde_json::json!({ "reason": "model_lane_not_granted" })));
    }
    Ok(())
}

fn process_limit() -> Arc<Semaphore> {
    static LIMIT: OnceLock<Arc<Semaphore>> = OnceLock::new();
    LIMIT
        .get_or_init(|| Arc::new(Semaphore::new(PROCESS_CONCURRENCY)))
        .clone()
}

/// The credential the engine authenticates with: the minted service-token
/// provider when the edge has one (production, WS16.S3), else the static dev
/// token of the auth dev bypass.
enum LaneCredential<'a> {
    Minted(quarry_runtime::service_tokens::SharedServiceTokenProvider),
    DevToken(&'a str),
}

fn lane_credential(state: &AppState) -> Option<LaneCredential<'_>> {
    if let Some(provider) = state.service_token_provider.clone() {
        return Some(LaneCredential::Minted(provider));
    }
    state
        .model_plane_token
        .as_deref()
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(LaneCredential::DevToken)
}

/// The engine for this edge's Model Plane settings, built once per distinct
/// (url, credential kind) and shared. For the dev token the key covers the token
/// and the inference token; for the minted lane it covers the provider instance.
/// The key is held as a digest, so the credentials are not kept twice.
fn engine_for(state: &AppState, settings: &AiFormatsSettings) -> Option<Arc<AiScrapeEngine>> {
    static CACHE: Mutex<Option<(String, Arc<AiScrapeEngine>)>> = Mutex::new(None);
    let url = state.model_plane_url.as_deref()?.trim();
    if url.is_empty() {
        return None;
    }
    let credential = lane_credential(state)?;
    let credential_key = match &credential {
        // The provider instance, not a token: tokens are minted per org and per call.
        LaneCredential::Minted(provider) => {
            format!("minted\n{:p}", Arc::as_ptr(provider) as *const ())
        }
        LaneCredential::DevToken(token) => format!(
            "dev\n{token}\n{}",
            settings.inference_token.as_deref().unwrap_or("")
        ),
    };
    let key = blake3::hash(format!("{url}\n{credential_key}\n{}", settings.model_json).as_bytes())
        .to_hex()
        .to_string();
    let mut cache = CACHE.lock().ok()?;
    if let Some((held, engine)) = cache.as_ref() {
        if *held == key {
            return Some(engine.clone());
        }
    }
    let base = ModelPlaneClient::new(url).ok()?;
    let client = match credential {
        // Minted per verified request org (`AiScrapeInput::org_id`); the dev
        // inference token never applies to this lane.
        LaneCredential::Minted(provider) => base.with_token_provider(provider),
        LaneCredential::DevToken(token) => {
            let mut client = base.with_bearer_token(token);
            if let Some(second) = settings.inference_token.as_deref() {
                client = client.with_inference_bearer_token(second);
            }
            client
        }
    };
    let engine = Arc::new(
        AiScrapeEngine::new(Arc::new(client), settings.engine_config())
            .with_process_limit(process_limit()),
    );
    *cache = Some((key, engine.clone()));
    Some(engine)
}

/// WS16.S6: the shared engine for another route of this edge (`/v1/extract`),
/// built exactly as the scrape route builds it: the process settings, the same
/// cache, the same process-wide limit on calls in flight. `None` when the edge
/// has no Model Plane URL or no credential for the chat lane. It does not read
/// the switch: the caller decides whether a model may be asked.
pub(crate) fn engine_for_request(state: &AppState) -> Option<Arc<AiScrapeEngine>> {
    engine_for(state, settings())
}

/// What every requested format answers under ZDR: skipped, no call made.
pub fn zdr_report(specs: &[AiSpec]) -> AiReport {
    let mut report = AiReport::default();
    for spec in specs {
        report.set(spec.kind(), AiFormatReport::skipped(AiCode::ZdrUnavailable));
    }
    report
}

/// What every requested format answers when the request's privacy block forbids
/// a model hop (`credential_or_secret`, `zdr_ephemeral`): skipped, no call made.
pub fn privacy_report(specs: &[AiSpec]) -> AiReport {
    let mut report = AiReport::default();
    for spec in specs {
        report.set(spec.kind(), AiFormatReport::skipped(AiCode::PrivacyPolicy));
    }
    report
}

/// Run the requested AI formats against the page text. Never fails: what went
/// wrong is in the report. Under ZDR, or when the privacy block forbids a model
/// hop (`model_hop_allowed` false), it answers without building a client, so no
/// call can be made.
pub async fn run_ai_formats(
    state: &AppState,
    org_id: &str,
    specs: &[AiSpec],
    source: &str,
    zdr: bool,
    model_hop_allowed: bool,
    request_id: &str,
) -> (AiFields, AiReport) {
    if zdr {
        return (AiFields::default(), zdr_report(specs));
    }
    if !model_hop_allowed {
        return (AiFields::default(), privacy_report(specs));
    }
    let Some(engine) = engine_for(state, settings()) else {
        // `availability` refuses this before the fetch; this is the guard behind it.
        let mut report = AiReport::default();
        for spec in specs {
            report.set(
                spec.kind(),
                AiFormatReport::failed(AiCode::ModelUnavailable, Some(AiReason::Transport)),
            );
        }
        return (AiFields::default(), report);
    };
    engine
        .run(
            specs,
            AiScrapeInput {
                source,
                org_id,
                zdr,
                request_id,
                deadline: None,
            },
        )
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn lookup<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_owned())
        }
    }

    fn claims(principal: &str, scopes: &[&str]) -> Claims {
        serde_json::from_value(json!({
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
    fn the_switch_is_off_unless_the_value_names_on() {
        assert!(!AiFormatsSettings::from_lookup(lookup(&[])).enabled);
        for off in ["", "0", "off", "false", "yes", "enable", "onn", "2"] {
            let settings = AiFormatsSettings::from_lookup(lookup(&[(ENV_AI_FORMATS, off)]));
            assert!(!settings.enabled, "{off:?} must not enable it");
        }
        for on in ["on", "1", "true", " ON ", "True"] {
            let settings = AiFormatsSettings::from_lookup(lookup(&[(ENV_AI_FORMATS, on)]));
            assert!(settings.enabled, "{on:?} must enable it");
        }
    }

    #[test]
    fn the_code_reads_exactly_the_names_the_docs_list() {
        let asked = std::cell::RefCell::new(std::collections::BTreeSet::new());
        let _ = AiFormatsSettings::from_lookup(|name| {
            asked.borrow_mut().insert(name.to_owned());
            None
        });
        let expected: std::collections::BTreeSet<String> =
            ENV_NAMES.iter().map(|name| (*name).to_owned()).collect();
        assert_eq!(*asked.borrow(), expected);
        assert!(ENV_NAMES
            .iter()
            .all(|name| name.starts_with("QUARRY_EDGE__")));
    }

    #[test]
    fn defaults_match_the_design() {
        let settings = AiFormatsSettings::from_lookup(lookup(&[]));
        assert_eq!(settings.model_json, "verevon-balance");
        assert_eq!(settings.model_text, "verevon-budget");
        assert_eq!(settings.max_input_chars, 60_000);
        assert_eq!(settings.max_total_tokens, 200_000);
        assert_eq!(settings.max_calls, 8);
        assert_eq!(settings.concurrency, 4);
        assert!(settings.inference_token.is_none());
        assert!(!settings.zdr_allow_requested);
    }

    #[test]
    fn the_caps_can_be_lowered_and_never_raised_past_the_ceiling() {
        let settings = AiFormatsSettings::from_lookup(lookup(&[
            (ENV_MAX_INPUT_CHARS, "5000"),
            (ENV_MAX_CALLS, "3"),
            (ENV_CONCURRENCY, "2"),
            (ENV_MAX_TOTAL_TOKENS, "50000"),
        ]));
        assert_eq!(settings.max_input_chars, 5_000);
        assert_eq!(settings.max_calls, 3);
        assert_eq!(settings.concurrency, 2);
        assert_eq!(settings.max_total_tokens, 50_000);

        let high = AiFormatsSettings::from_lookup(lookup(&[
            (ENV_MAX_INPUT_CHARS, "9999999"),
            (ENV_MAX_CALLS, "99"),
            (ENV_CONCURRENCY, "99"),
        ]));
        assert_eq!(high.max_input_chars, 80_000);
        assert_eq!(high.max_calls, 8);
        assert_eq!(high.concurrency, 4);

        let low = AiFormatsSettings::from_lookup(lookup(&[
            (ENV_MAX_INPUT_CHARS, "10"),
            (ENV_MAX_CALLS, "0"),
            (ENV_CONCURRENCY, "0"),
            (ENV_MAX_TOTAL_TOKENS, "5"),
        ]));
        assert_eq!(low.max_input_chars, 1_000);
        assert_eq!(low.max_calls, 1);
        assert_eq!(low.concurrency, 1);
        assert_eq!(low.max_total_tokens, 1_000);

        let junk = AiFormatsSettings::from_lookup(lookup(&[(ENV_MAX_INPUT_CHARS, "lots")]));
        assert_eq!(junk.max_input_chars, 60_000);
    }

    #[test]
    fn models_are_aliases_and_a_blank_one_falls_back() {
        let settings = AiFormatsSettings::from_lookup(lookup(&[
            (ENV_MODEL_JSON, " verevon-premium "),
            (ENV_MODEL_TEXT, "  "),
        ]));
        assert_eq!(settings.model_json, "verevon-premium");
        assert_eq!(settings.model_text, "verevon-budget");
        let config = settings.engine_config();
        assert_eq!(config.model_json, "verevon-premium");
    }

    #[test]
    fn zdr_allow_is_noted_and_changes_nothing_else() {
        let settings = AiFormatsSettings::from_lookup(lookup(&[(ENV_AI_FORMATS_ZDR, "ALLOW")]));
        assert!(settings.zdr_allow_requested);
        let refuse = AiFormatsSettings::from_lookup(lookup(&[(ENV_AI_FORMATS_ZDR, "refuse")]));
        assert!(!refuse.zdr_allow_requested);
    }

    #[test]
    fn the_debug_dump_hides_the_inference_token() {
        let settings =
            AiFormatsSettings::from_lookup(lookup(&[(ENV_INFERENCE_TOKEN, "s3cret-token")]));
        let dump = format!("{settings:?}");
        assert!(!dump.contains("s3cret-token"), "{dump}");
        assert!(dump.contains("<set>"));
    }

    #[test]
    fn raw_formats_are_detected_by_object_type_and_the_summary_string() {
        let detect = |value: Value| requests_ai_format(Some(&value));
        assert!(detect(json!(["markdown", "summary"])));
        assert!(detect(json!([{ "type": "json", "schema": {} }])));
        assert!(detect(json!([{ "type": "question", "question": "q" }])));
        assert!(detect(
            json!(["markdown", { "type": "highlights", "query": "q" }])
        ));
        assert!(!detect(json!(["markdown", "html"])));
        assert!(!detect(json!([{ "type": "screenshot" }])));
        assert!(!detect(json!("summary")));
        assert!(!requests_ai_format(None));
        assert_eq!(
            first_ai_index(Some(&json!(["markdown", "html", "summary"]))),
            Some(2)
        );
    }

    #[test]
    fn a_user_may_ask_a_service_needs_the_scope_and_an_agent_label_never_passes() {
        let user = claims("user", &["scrape:read"]);
        assert!(check_principal(&user, None, Some(0)).is_ok());
        assert!(check_principal(&user, Some("somebody"), Some(0)).is_ok());

        let refused = check_principal(&user, Some("agent"), Some(2)).expect_err("agent label");
        assert_eq!(refused.code, ErrorCode::Forbidden);
        let details = refused.details.expect("details");
        assert_eq!(details["reason"], "extract_ai_scope_required");
        assert_eq!(details["field"], "formats[2]");

        let bare = claims("service", &["scrape:read"]);
        assert!(check_principal(&bare, None, Some(1)).is_err());
        let granted = claims("service", &["scrape:read", SCOPE_EXTRACT_AI]);
        assert!(check_principal(&granted, None, Some(1)).is_ok());
        assert!(
            check_principal(&granted, Some("agent"), Some(1)).is_err(),
            "the scope does not lift the agent label"
        );
        // The scrape:interact scope of actions is not this scope.
        let interact = claims("service", &["scrape:interact"]);
        assert!(check_principal(&interact, None, None).is_err());
    }

    #[test]
    fn zdr_answers_every_requested_format_skipped() {
        use quarry_core::ai_spec::{AiKind, AiStatus};
        let specs = vec![AiSpec::default_summary()];
        let report = zdr_report(&specs);
        let summary = report.get(AiKind::Summary).expect("summary entry");
        assert_eq!(summary.status, AiStatus::Skipped);
        assert_eq!(summary.code, Some(AiCode::ZdrUnavailable));
        assert!(report.get(AiKind::Json).is_none());
        assert_eq!(report.total.calls, 0);
    }
}
