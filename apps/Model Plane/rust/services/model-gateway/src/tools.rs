//! Wave 10a — handlers for the 5 trivial tool RPCs, plus the plain-chat-only
//! `get_weather` and `get_statistics` builtins.
//!
//! - `WebSearch`:       `quarry::Client::search` proxy
//! - `GetWeather`:      information-core (Yr/met.no) proxy, plain-chat builtin only
//! - `GetStatistics`:   information-core (SSB `PxWebApi` v2) proxy, plain-chat builtin only
//! - Sleep:           `tokio::time::sleep` with server-side cap
//! - `RemoteTrigger`:   outbound HTTP webhook + SSRF guard
//! - `SendMessage`:     quarantined legacy RPC (fails closed)
//! - `SyntheticOutput`: deterministic echo (test/dev only)
//!
//! Each handler is intentionally < 60 LOC. The corresponding v2 Python
//! files totalled ~650 LOC; the bulk of that was Pydantic validation
//! and framework glue that Tonic + prost give us for free.

use std::fmt::Write as _;
use std::net::IpAddr;
use std::time::Duration;

use tonic::Status;
use tracing::warn;

use mp_contracts::model_plane::v1::{
    ExecuteStepRequest, RemoteTriggerRequest, RemoteTriggerResponse, SendMessageRequest,
    SendMessageResponse, SleepRequest, SleepResponse, SyntheticOutputRequest,
    SyntheticOutputResponse, WebSearchRequest, WebSearchResponse, WebSearchResult,
};

use crate::quarry::{QuarryError, SearchOptions};
use crate::state::AppState;

/// Hard cap on Sleep duration — agents can't tie up a gateway worker
/// for more than this even if they ask.
const MAX_SLEEP_MS: i32 = 60_000;

/// Hard cap on `RemoteTrigger` timeout.
const MAX_REMOTE_TIMEOUT_MS: i32 = 30_000;

/// Maximum response body we'll return through `RemoteTrigger`. Beyond
/// this we truncate. Keeps a misbehaving webhook from blowing the gRPC
/// 4 MB message limit.
const MAX_REMOTE_BODY_BYTES: usize = 2 * 1024 * 1024;

// ---------------- WebSearch ----------------

/// Runs a web search via the Quarry edge with no per-call narrowing options —
/// the `WebSearchRequest` proto carries none, so this is every caller that
/// only has the proto message to work from. The request's paid-provider grant
/// still reaches the edge; see [`handle_web_search_with_options`].
///
/// # Errors
///
/// Returns `Status::unimplemented` if Quarry is not configured, or maps a
/// Quarry search failure to a `Status`.
pub async fn handle_web_search(
    state: &AppState,
    req: WebSearchRequest,
) -> Result<WebSearchResponse, Status> {
    handle_web_search_with_options(state, req, &SearchOptions::default()).await
}

/// Runs a web search via the Quarry edge, narrowing it with [`SearchOptions`]
/// (language, region, topic vertical, recency window, domain filters).
///
/// Separate from [`handle_web_search`] rather than an added parameter on it
/// because the narrowing options are not part of the `WebSearchRequest` proto:
/// they are derived by the caller from turn context (the user's language,
/// whether the question is recency-sensitive), which the gRPC surface never
/// sees.
///
/// `allow_paid_providers` is the one search option that works the other way
/// round, and the request always wins on it — see the note at the assignment.
///
/// # Errors
///
/// Returns `Status::unimplemented` if Quarry is not configured, or maps a
/// Quarry search failure to a `Status`.
pub async fn handle_web_search_with_options(
    state: &AppState,
    req: WebSearchRequest,
    options: &SearchOptions,
) -> Result<WebSearchResponse, Status> {
    if !state.quarry.available() {
        return Err(Status::unimplemented("quarry edge not configured"));
    }
    // The paid-provider grant comes from the REQUEST, overriding whatever the
    // options carried, in both directions. It is a tenant-and-tier decision
    // that only the gRPC caller can make, whereas `options` is assembled from
    // turn context and — in the tool loop — partly from model-authored tool
    // arguments. Honouring the options value as well would give the model a
    // second, quieter way to reach a paid provider with external egress that
    // its tenant may not be entitled to; taking the request's `false` over an
    // options `true` is the fail-closed direction of the same rule.
    let options = SearchOptions {
        allow_paid_providers: req.allow_paid_providers,
        ..options.clone()
    };
    let results = state
        .quarry
        .search_with_options(
            &req.query,
            req.limit,
            &req.intent,
            &req.org_id,
            req.zdr,
            &options,
        )
        .await
        .map_err(quarry_err_to_status)?;

    let total = i32::try_from(results.len()).unwrap_or(i32::MAX);
    Ok(WebSearchResponse {
        request_id: req.request_id,
        results: results
            .into_iter()
            .map(|r| WebSearchResult {
                url: r.url,
                title: r.title,
                snippet: r.snippet,
                source: r.source,
                score: r.score,
            })
            .collect(),
        total,
    })
}

// ---------------- RunCode ----------------

/// Normalizes a model-supplied language to the value execution-core's
/// `code_interpreter` accepts. Validating here gives the model a corrective
/// error in one round-trip instead of a confusing sandbox failure.
fn normalize_code_language(language: &str) -> Result<&'static str, String> {
    match language.trim().to_lowercase().as_str() {
        // Python is the default: it's what "run some code" means to a chat
        // user doing calculations, data work, or generating a document.
        "" | "python" | "python3" | "py" => Ok("python"),
        "sh" | "bash" | "shell" | "posix" => Ok("sh"),
        other => Err(format!(
            "code_interpreter supports 'python' and 'sh', not '{other}'"
        )),
    }
}

/// Test-only accessor for the language whitelist, so `tool_loop`'s tests can
/// pin the mapping without this function needing to be `pub`.
#[cfg(test)]
pub(crate) fn normalize_code_language_for_test(language: &str) -> Result<&'static str, String> {
    normalize_code_language(language)
}

/// Executes a short code snippet via execution-core's sandboxed `shell` tool
/// (bubblewrap: read-only rootfs, no network, wall-clock timeout, output
/// scrubbed and capped by the executor). This is the same primitive agentic
/// runs use — chat gets no second, weaker sandbox.
///
/// # Errors
///
/// Returns `Err` when the language is unsupported, the code is empty, the
/// credential cannot be forwarded, execution-core is unreachable, or the step
/// finishes in any status other than `completed`.
#[allow(clippy::too_many_arguments)] // request context, mirrors dispatch_tool
pub async fn handle_code_interpreter(
    state: &AppState,
    execution_bearer: &crate::auth::VerifiedExecutionBearer,
    data_plane_bearer: &crate::auth::VerifiedDataPlaneBearer,
    // Present only for a Space-scoped turn. Forwarded unchanged for
    // execution-core to verify and present to sandbox-manager's AcquireLease
    // (S3_3_DURABLE_WORKSPACE_DESIGN_2026-09-11.md §3.5 phase B.2).
    sandbox_bearer: Option<&crate::auth::VerifiedSandboxBearer>,
    inference_bearer: &str,
    session_bearer: &str,
    run_id: &str,
    org_id: &str,
    user_id: &str,
    space_id: &str,
    zdr: bool,
    language: &str,
    code: &str,
    files_in: Option<&serde_json::Value>,
) -> Result<String, String> {
    if code.trim().is_empty() {
        return Err("code_interpreter requires non-empty 'code'".to_owned());
    }
    let language = normalize_code_language(language)?;
    let mut tool_input = serde_json::json!({ "language": language, "code": code });
    // Forwarded as-is when present; execution-core validates each entry's name
    // (rejecting absolute paths and traversal) since it owns the workspace.
    if let Some(files_in) = files_in.filter(|value| !value.is_null()) {
        tool_input["files_in"] = files_in.clone();
    }
    let mut request = tonic::Request::new(ExecuteStepRequest {
        run_id: run_id.to_owned(),
        step_id: format!("code-{}", mp_ids::new_ulid()),
        // `code_interpreter`, NOT `shell`: the two are deliberately distinct
        // capabilities. `shell` runs an arbitrary command and is bound to
        // high-risk `cap.command.shell`, which capability-core resolves to
        // `ask` — a human approval on every call. That is right for arbitrary
        // shell and wrong for a calculator, so the hermetic path (no network,
        // read-only rootfs, throwaway workspace, hard timeout, scrubbed
        // output) is its own low-risk `cap.command.sandbox` capability.
        tool_name: "code_interpreter".to_owned(),
        tool_input: tool_input.to_string(),
        // The sandbox itself is the safety boundary for inline snippets, the
        // same posture a ChatGPT-style interpreter takes. Execution-core's hook
        // rules can still deny the tool outright per deployment.
        permission_mode: "auto".to_owned(),
        // Forward the org's registered hook rules so execution-core can apply
        // them. Without this the comment above was false: no rule ever reached
        // the engine, so a deployment could not deny this tool.
        hook_context: crate::runtime_registries::hook_context_json(&state.hooks, org_id),
        org_id: org_id.to_owned(),
        user_id: user_id.to_owned(),
        zdr,
        // code_interpreter never reaches knowledge_search, so there is no
        // sovereignty derivation for this field to feed.
        min_privacy_tier: mp_contracts::model_plane::v1::PrivacyTier::Unspecified as i32,
        space_id: space_id.to_owned(),
    });
    request.metadata_mut().insert(
        "authorization",
        format!("Bearer {}", execution_bearer.as_str())
            .parse()
            .map_err(|_| "execution credential is not forwardable".to_owned())?,
    );
    request.metadata_mut().insert(
        "x-session-authorization",
        format!("Bearer {session_bearer}")
            .parse()
            .map_err(|_| "session credential is not forwardable".to_owned())?,
    );
    // execution-core's `execute_step` authenticates the data-plane AND
    // inference bearers unconditionally, for EVERY tool — not just the ones
    // that use them (see execution-core `src/grpc.rs:313-320`). Sending only
    // the execution + session pair returns `unauthenticated` before the tool
    // name is even looked at, which is exactly how the first live code_interpreter
    // attempt failed.
    request.metadata_mut().insert(
        "x-data-plane-authorization",
        format!("Bearer {}", data_plane_bearer.as_str())
            .parse()
            .map_err(|_| "data-plane credential is not forwardable".to_owned())?,
    );
    request.metadata_mut().insert(
        "x-inference-authorization",
        format!("Bearer {inference_bearer}")
            .parse()
            .map_err(|_| "inference credential is not forwardable".to_owned())?,
    );
    // Unlike the four above, conditional: execution-core demands it only for a
    // Space-scoped code_interpreter step, and an empty `Bearer ` would be
    // refused there, never treated as absent.
    if let Some(sandbox_bearer) = sandbox_bearer {
        request.metadata_mut().insert(
            "x-sandbox-authorization",
            format!("Bearer {}", sandbox_bearer.as_str())
                .parse()
                .map_err(|_| "sandbox credential is not forwardable".to_owned())?,
        );
    }
    let response = state
        .execution_client
        .clone()
        .execute_step(request)
        .await
        .map_err(|status| format!("code_interpreter failed: {}", status.message()))?
        .into_inner();
    match response.status.as_str() {
        "completed" => Ok(response.output),
        status => Err(if response.error.is_empty() {
            format!("code_interpreter finished with status '{status}'")
        } else {
            response.error
        }),
    }
}

// ---------------- GetWeather ----------------

/// Oslo — the same fallback information-core's own `Weather` handler applies
/// when `lat`/`lon` are omitted (`internal/http/handlers.go:442` in
/// information-core). Kept in sync deliberately: an unresolved location
/// should degrade to exactly what the server itself would have shown.
const DEFAULT_WEATHER_LAT: f64 = 59.9139;
const DEFAULT_WEATHER_LON: f64 = 10.7522;

/// v1 static location table: city name → (lat, lon). information-core has no
/// geocoding of its own — its `Weather` handler takes `lat`/`lon` only — so
/// resolving a free-text place name has to happen here. This is a deliberate
/// v1 scope limit (top-10-ish Norwegian cities by population), not a hidden
/// gap: anything outside this list (smaller towns, non-Norwegian cities,
/// typos) falls back to Oslo, same as an empty location would.
const NORWEGIAN_CITIES: &[(&str, f64, f64)] = &[
    ("oslo", DEFAULT_WEATHER_LAT, DEFAULT_WEATHER_LON),
    ("bergen", 60.3913, 5.3221),
    ("trondheim", 63.4305, 10.3951),
    ("stavanger", 58.9700, 5.7331),
    ("tromso", 69.6492, 18.9553),
    ("kristiansand", 58.1467, 7.9956),
    ("drammen", 59.7440, 10.2045),
    ("fredrikstad", 59.2181, 10.9298),
    ("sandnes", 58.8516, 5.7357),
    ("sarpsborg", 59.2839, 11.1096),
];

/// Lowercases and folds Norwegian æøå to plain ASCII, so "Tromsø", "tromsø"
/// and "TROMSO" are one key. Shared by the weather city table and the
/// statistics region table: a user who types a place name for one tool spells
/// it the same way for the other, and two copies of this would drift.
fn fold_norwegian(value: &str) -> String {
    value
        .trim()
        .to_lowercase()
        .replace('æ', "ae")
        .replace('ø', "o")
        .replace('å', "a")
}

/// Resolves a free-text location to (lat, lon) via [`NORWEGIAN_CITIES`].
/// Case/whitespace-insensitive; Norwegian æøå are folded to plain ASCII so
/// "Tromsø", "tromsø", and "TROMSO" all resolve the same entry.
///
/// An EMPTY location resolves to Oslo — the caller asked for "the weather"
/// with no place, and Oslo is what information-core itself defaults to. A
/// NON-EMPTY location that isn't in the table resolves to `None`, because
/// silently substituting Oslo is the one outcome worse than no answer:
/// "what's the weather in Paris" would come back as real, confident,
/// correctly-labelled *Oslo* data, and nothing downstream could tell it was
/// answering a different question than the one asked.
fn resolve_location(location: &str) -> Option<(f64, f64)> {
    let trimmed = location.trim();
    if trimmed.is_empty() {
        return Some((DEFAULT_WEATHER_LAT, DEFAULT_WEATHER_LON));
    }
    let normalized = fold_norwegian(trimmed);
    NORWEGIAN_CITIES
        .iter()
        .find(|(name, _, _)| *name == normalized)
        .map(|&(_, lat, lon)| (lat, lon))
}

/// The message returned for a location outside [`NORWEGIAN_CITIES`]. Names
/// the covered cities and points at `web_search`, so the model can recover on
/// its own instead of dead-ending the user.
fn unsupported_location_message(location: &str) -> String {
    let supported: Vec<&str> = NORWEGIAN_CITIES.iter().map(|&(name, ..)| name).collect();
    format!(
        "get_weather has no coordinates for \"{}\" — it covers only these Norwegian cities: {}. \
         Do not answer with another city's weather. Use web_search for this location instead, \
         or tell the user this city is not covered.",
        location.trim(),
        supported.join(", ")
    )
}

#[derive(serde::Deserialize)]
struct WeatherPayload {
    current: WeatherCurrent,
    #[serde(default)]
    forecast: Vec<WeatherForecastDay>,
}

#[derive(serde::Deserialize)]
struct WeatherCurrent {
    condition: String,
    location: String,
    temperature: i64,
    #[serde(rename = "windSpeed")]
    wind_speed: i64,
    precipitation: f64,
    humidity: i64,
}

#[derive(serde::Deserialize)]
struct WeatherForecastDay {
    date: String,
    condition: String,
    temperature: WeatherForecastMinMax,
}

#[derive(serde::Deserialize)]
struct WeatherForecastMinMax {
    max: i64,
    min: i64,
}

/// Formats a compact, model-friendly text summary (never a raw JSON dump):
/// current conditions plus up to 3 days of forecast highs/lows.
fn format_weather_summary(payload: &WeatherPayload) -> String {
    let c = &payload.current;
    let mut out = format!(
        "Weather for {}: {}, {}°C, wind {} m/s, {} mm precipitation, {}% humidity.",
        c.location, c.condition, c.temperature, c.wind_speed, c.precipitation, c.humidity
    );
    if !payload.forecast.is_empty() {
        let days: Vec<String> = payload
            .forecast
            .iter()
            .take(3)
            .map(|d| {
                format!(
                    "{} {} (high {}°C / low {}°C)",
                    d.date, d.condition, d.temperature.max, d.temperature.min
                )
            })
            .collect();
        let _ = write!(out, " Forecast: {}.", days.join("; "));
    }
    out
}

/// Fetches current conditions + a short forecast for a Norwegian city from
/// information-core (Application Plane; wraps Yr/met.no).
///
/// Unlike `web_search`, no `org_id`/`zdr` is threaded through this call:
/// weather data is public, non-personal information, not a caller-specific
/// query whose text could itself be sensitive. This is the exact same
/// judgment the frontend gateway already makes for this same upstream (see
/// `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/information.rs:14`:
/// "Information feeds: weather, traffic, news ... public data ... No session
/// guard") — carried over here deliberately, not an oversight.
///
/// # Errors
///
/// Returns `Err` (never panics/unwraps) when information-core is
/// unreachable, returns a non-2xx status, or replies with an unparseable body.
pub async fn handle_get_weather(state: &AppState, location: &str) -> Result<String, String> {
    let Some((lat, lon)) = resolve_location(location) else {
        return Err(unsupported_location_message(location));
    };
    let base = state.information_core_base_url.trim_end_matches('/');
    let url = format!("{base}/api/v1/weather?lat={lat}&lon={lon}");
    let mut req = state.http_client.get(&url);
    if !state.information_core_internal_key.is_empty() {
        req = req.header("x-internal-api-key", &state.information_core_internal_key);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| format!("information-core unreachable: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "information-core returned {}",
            resp.status().as_u16()
        ));
    }
    let payload: WeatherPayload = resp
        .json()
        .await
        .map_err(|e| format!("information-core returned an unparseable weather payload: {e}"))?;
    Ok(format_weather_summary(&payload))
}

// ---------------- GetStatistics ----------------

/// One curated Statistics Norway series: the table it lives in, the
/// `ContentsCode` that picks it out of that table, and how we name it.
///
/// Table id and contents code are exactly the two values a model would
/// otherwise have to invent, and inventing them is the failure this tool
/// exists to prevent: SSB publishes thousands of tables, so a made-up
/// five-digit id has a perfectly plausible shape and no way to be recognized
/// as wrong before it is queried.
struct CuratedStatistic {
    /// The value the model passes as `statistic`.
    key: &'static str,
    /// SSB `PxWebApi` v2 table id (five digits, as the API validates).
    table: &'static str,
    /// `ContentsCode` value selecting this series inside `table`.
    contents_code: &'static str,
    /// English description, used in the tool's own coverage message.
    label: &'static str,
}

/// v1 statistics coverage — deliberately ONE verified series rather than a
/// plausible-looking dozen.
///
/// The series and its table are the ones the Oslo-population incident
/// established: the SSB page that was fetched and then failed to be read cites
/// its own origin, "Kilde: Statistisk sentralbyrå, tabell 01222".
///
/// A table id or contents code that has NOT been checked against the live API
/// is the same failure class as one the model hallucinated — it advertises a
/// capability whose only possible outcome is a dead end — so this map grows by
/// verified addition, with `describe` (below) as the way to read what a table
/// really offers before an entry is written here.
///
/// `Folketallet11` is that verification, and it is why the rule above is
/// written so sharply. The page's hydration payload labels the figure
/// `Folketallet`, and that spelling is NOT a contents code — sending it made
/// SSB refuse the whole query, which reached the model as "statistics are
/// unavailable" for a number the API serves instantly. Table 01222 offers
/// eleven contents codes, and the one matching this entry's label is
/// `Folketallet11` ("Population at the end of the quarter"); `Folketallet1` is
/// the quarter's OPENING figure and would be quietly off by one quarter.
/// Verified live against `/api/v1/statistics/metadata?table=01222`, which
/// returns `value: [729437]` for Oslo at `2026K2`.
const CURATED_STATISTICS: &[CuratedStatistic] = &[CuratedStatistic {
    key: "population",
    table: "01222",
    contents_code: "Folketallet11",
    label: "population at the end of the quarter, by municipality",
}];

/// One curated SSB region.
struct CuratedRegion {
    /// Spellings a user might type. The empty string is the national figure:
    /// "what is Norway's population" names no municipality.
    aliases: &'static [&'static str],
    /// The `Region` value code SSB indexes this region under.
    code: &'static str,
    /// Lowercase fragment that the region label SSB sends BACK must contain.
    /// Checked on every answer — see [`region_label_disagrees`].
    expected_label: &'static str,
}

/// v1 region coverage. Municipality codes are renumbered by county reforms
/// (Tromsø was 5401 under Troms og Finnmark and 5501 since Troms was restored
/// in 2024), which is precisely why every answer re-checks the label SSB
/// returned against `expected_label` instead of trusting this table: a stale
/// code must fail loudly, never come back as another municipality's real,
/// confident, correctly-formatted population.
const CURATED_REGIONS: &[CuratedRegion] = &[
    CuratedRegion {
        aliases: &["", "norge", "norway", "hele landet", "the whole country"],
        code: "0",
        expected_label: "whole country",
    },
    CuratedRegion {
        aliases: &["oslo"],
        code: "0301",
        expected_label: "oslo",
    },
    CuratedRegion {
        aliases: &["bergen"],
        code: "4601",
        expected_label: "bergen",
    },
    CuratedRegion {
        aliases: &["trondheim"],
        code: "5001",
        expected_label: "trondheim",
    },
    CuratedRegion {
        aliases: &["stavanger"],
        code: "1103",
        expected_label: "stavanger",
    },
    CuratedRegion {
        aliases: &["kristiansand"],
        code: "4204",
        expected_label: "kristiansand",
    },
    CuratedRegion {
        aliases: &["sandnes"],
        code: "1108",
        expected_label: "sandnes",
    },
    CuratedRegion {
        aliases: &["drammen"],
        code: "3301",
        expected_label: "drammen",
    },
    CuratedRegion {
        aliases: &["tromso", "tromsoe"],
        code: "5501",
        expected_label: "tromso",
    },
];

fn resolve_statistic(statistic: &str) -> Option<&'static CuratedStatistic> {
    let key = statistic.trim().to_lowercase();
    CURATED_STATISTICS.iter().find(|entry| entry.key == key)
}

fn resolve_region(region: &str) -> Option<&'static CuratedRegion> {
    let key = fold_norwegian(region);
    CURATED_REGIONS
        .iter()
        .find(|entry| entry.aliases.contains(&key.as_str()))
}

/// The message returned for a `statistic` outside [`CURATED_STATISTICS`].
///
/// Names what IS available, exactly as [`unsupported_location_message`] does
/// for weather. The failure it forbids is sharper here than there: told only
/// "unknown statistic", a model's next move is to supply an SSB table id it
/// believes in, and a five-digit invention is indistinguishable from a real id
/// until it has already been queried.
fn unsupported_statistic_message(statistic: &str) -> String {
    let available: Vec<String> = CURATED_STATISTICS
        .iter()
        .map(|entry| format!("'{}' ({})", entry.key, entry.label))
        .collect();
    format!(
        "get_statistics has no series called \"{}\" — it covers only: {}. Do not answer with a \
         different statistic, and do not pass an SSB table id: the table is chosen by the series \
         name, not by you. Use web_search for anything outside this list, or tell the user this \
         figure is not covered.",
        statistic.trim(),
        available.join("; ")
    )
}

/// The message returned for a `region` outside [`CURATED_REGIONS`]. Same
/// contract as [`unsupported_location_message`]: name the coverage, forbid the
/// silent substitution, point at the tool that can still answer.
fn unsupported_region_message(region: &str) -> String {
    let covered: Vec<&str> = CURATED_REGIONS
        .iter()
        .filter_map(|entry| entry.aliases.first().copied())
        .map(|alias| if alias.is_empty() { "Norway" } else { alias })
        .collect();
    format!(
        "get_statistics has no SSB region code for \"{}\" — it covers only: {}. Do not answer \
         with another region's figure. Use web_search for this place instead, or tell the user it \
         is not covered.",
        region.trim(),
        covered.join(", ")
    )
}

/// Selection sent to information-core: one value per variable, so the reply is
/// a single cell.
///
/// `top(1)` is a `PxWebApi` v2 selection expression for "the most recent period".
/// Asking for the latest by name would mean guessing which quarter SSB has
/// published, and guessing that wrong is how a query for "now" silently
/// becomes a query for a quarter that does not exist yet.
fn statistics_selection(statistic: &CuratedStatistic, region_code: &str) -> serde_json::Value {
    serde_json::json!({
        "table": statistic.table,
        "selection": [
            { "variableCode": "Region", "valueCodes": [region_code] },
            { "variableCode": "ContentsCode", "valueCodes": [statistic.contents_code] },
            { "variableCode": "Tid", "valueCodes": ["top(1)"] },
        ]
    })
}

/// The single dimension value code JSON-stat2 carries for `dimension`.
///
/// `None` when the dimension holds anything other than exactly one value:
/// having selected one value per variable, more than one back means the query
/// did not mean what we thought, and picking the first cell of an unexpected
/// result set is how a figure ends up labelled with the wrong period.
fn single_dimension_code(dimension: &serde_json::Value) -> Option<String> {
    match dimension.get("category")?.get("index")? {
        serde_json::Value::Object(map) if map.len() == 1 => map.keys().next().cloned(),
        serde_json::Value::Array(codes) if codes.len() == 1 => {
            codes[0].as_str().map(str::to_owned)
        }
        _ => None,
    }
}

fn dimension_label(dimension: &serde_json::Value, code: &str) -> String {
    dimension
        .get("category")
        .and_then(|category| category.get("label"))
        .and_then(|labels| labels.get(code))
        .and_then(serde_json::Value::as_str)
        .unwrap_or(code)
        .to_owned()
}

/// The dimension name SSB assigns to a JSON-stat2 role (`time`, `geo`,
/// `metric`), falling back to the conventional `PxWeb` variable name. Read from
/// `role` rather than hard-coded because the fallback names are a convention,
/// not a guarantee, and a table that names its time variable something else
/// must still yield a period.
fn role_dimension<'a>(data: &'a serde_json::Value, role: &str, fallback: &'a str) -> &'a str {
    data.get("role")
        .and_then(|roles| roles.get(role))
        .and_then(serde_json::Value::as_array)
        .and_then(|names| names.first())
        .and_then(serde_json::Value::as_str)
        .unwrap_or(fallback)
}

/// True when the region SSB answered about is not the one the curated code was
/// supposed to denote — a stale municipality number after a county reform.
fn region_label_disagrees(expected: &str, returned: &str) -> bool {
    !fold_norwegian(returned).contains(expected)
}

/// Reduces a JSON-stat2 single-cell reply to the four things an answer needs:
/// the figure, its unit, its period, and its source.
///
/// The period is not optional. "729 437" is a correct answer this quarter and
/// a wrong one next quarter, and nothing downstream can tell the two apart
/// once the period has been dropped — so a payload that yields no period is an
/// error here, not a figure without a date.
fn statistics_figure(
    statistic: &CuratedStatistic,
    region: &CuratedRegion,
    data: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let dimensions = data
        .get("dimension")
        .ok_or_else(|| "SSB payload carries no 'dimension' block".to_owned())?;

    let time_name = role_dimension(data, "time", "Tid");
    let geo_name = role_dimension(data, "geo", "Region");
    let metric_name = role_dimension(data, "metric", "ContentsCode");

    let time = dimensions
        .get(time_name)
        .ok_or_else(|| format!("SSB payload carries no '{time_name}' dimension"))?;
    let period_code = single_dimension_code(time).ok_or_else(|| {
        format!("SSB returned more than one period for {time_name}; refusing to pick one")
    })?;

    let geo = dimensions
        .get(geo_name)
        .ok_or_else(|| format!("SSB payload carries no '{geo_name}' dimension"))?;
    let region_code = single_dimension_code(geo)
        .ok_or_else(|| format!("SSB returned more than one region for {geo_name}"))?;
    let region_label = dimension_label(geo, &region_code);
    if region_label_disagrees(region.expected_label, &region_label) {
        return Err(format!(
            "refusing to answer: SSB region code {} came back as \"{}\", not the region this tool \
             maps that code to. The code is most likely stale after a municipality renumbering — \
             answering would report the wrong place's figure.",
            region.code, region_label
        ));
    }

    let values = data
        .get("value")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "SSB payload carries no 'value' array".to_owned())?;
    let [figure] = values.as_slice() else {
        return Err(format!(
            "SSB returned {} figures for a single-cell query; refusing to pick one",
            values.len()
        ));
    };
    if !figure.is_number() {
        return Err(format!(
            "SSB has no {} figure for {region_label} in {period_code} (the cell is empty, \
             confidential, or not yet published)",
            statistic.key
        ));
    }

    let unit = dimensions
        .get(metric_name)
        .and_then(|metric| metric.get("category"))
        .and_then(|category| category.get("unit"))
        .and_then(|units| units.get(statistic.contents_code))
        .and_then(|unit| unit.get("base"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_owned();

    let provider = data
        .get("source")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("Statistics Norway (SSB)");

    let mut out = serde_json::json!({
        "statistic": statistic.label,
        "region": region_label,
        "period": dimension_label(time, &period_code),
        "value": figure.clone(),
        "source": format!("{provider}, table {}", statistic.table),
    });
    if !unit.is_empty() {
        out["unit"] = serde_json::json!(unit);
    }
    // When SSB published the figure, distinct from the period it describes.
    if let Some(updated) = data.get("updated").and_then(serde_json::Value::as_str) {
        out["updated"] = serde_json::json!(updated);
    }
    Ok(out)
}

/// Most values listed per variable in a `describe` reply. The metadata for a
/// regional table names every municipality; the model needs the SHAPE of the
/// table, and the whole list would cost more context than the figure it is
/// trying to confirm.
const MAX_DESCRIBED_VALUES: usize = 8;

/// Reduces `PxWebApi` v2 table metadata to what a model can act on: the table's
/// title and its variables, each with a few example value codes.
fn statistics_metadata_summary(
    statistic: &CuratedStatistic,
    data: &serde_json::Value,
) -> serde_json::Value {
    let variables: Vec<serde_json::Value> = data
        .get("dimension")
        .and_then(serde_json::Value::as_object)
        .map(|dimensions| {
            dimensions
                .iter()
                .map(|(name, dimension)| {
                    let codes: Vec<&String> = dimension
                        .get("category")
                        .and_then(|category| category.get("index"))
                        .and_then(serde_json::Value::as_object)
                        .map(|index| index.keys().collect())
                        .unwrap_or_default();
                    serde_json::json!({
                        "variable": name,
                        "label": dimension.get("label").and_then(serde_json::Value::as_str).unwrap_or(name),
                        "values": codes.len(),
                        "examples": codes.iter().take(MAX_DESCRIBED_VALUES).collect::<Vec<_>>(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    serde_json::json!({
        "statistic": statistic.key,
        "table": statistic.table,
        "title": data.get("label").and_then(serde_json::Value::as_str).unwrap_or(statistic.label),
        "contents_code": statistic.contents_code,
        "variables": variables,
    })
}

/// Maps an information-core status to a message the model can act on.
///
/// 503 is called out by name because it is the one outage with a different
/// correct response: SSB is not wired up in this deployment at all, so
/// retrying is pointless and the model should say so and move on rather than
/// burning the turn.
fn statistics_status_message(status: u16) -> String {
    if status == 503 {
        return "information-core has no SSB statistics source configured in this deployment. Do \
                not retry; use web_search, and say the figure came from the web rather than from \
                Statistics Norway."
            .to_owned();
    }
    format!(
        "information-core returned {status} for the SSB query. Do not state a figure you did not \
         receive — say the lookup failed, or try web_search."
    )
}

/// The information-core envelope both statistics endpoints return. `data` is
/// left as a `Value`: JSON-stat2 keys its category maps by SSB's own value
/// codes, so there is no fixed struct to deserialize into.
#[derive(serde::Deserialize)]
struct StatisticsEnvelope {
    #[serde(default)]
    data: serde_json::Value,
}

/// Answers a curated statistic for a curated region from information-core
/// (Application Plane; wraps SSB `PxWebApi` v2), or — with `describe` — reports
/// what the underlying table actually contains.
///
/// This is the tier that makes the Oslo-population case need no scraping at
/// all: the SSB page names its own origin ("tabell 01222"), and that origin is
/// an API this deployment already calls.
///
/// Like `get_weather`, no `org_id`/`zdr` is threaded through: official
/// statistics are public, non-personal data, and the query text is a curated
/// key rather than anything the user typed.
///
/// # Errors
///
/// Returns `Err` when the statistic or region is outside the curated coverage,
/// when information-core is unreachable or replies non-2xx, when the payload
/// cannot be reduced to a figure WITH its period, or when SSB answers about a
/// different region than the curated code was supposed to denote.
pub async fn handle_get_statistics(
    state: &AppState,
    statistic: &str,
    region: &str,
    describe: bool,
) -> Result<String, String> {
    let Some(statistic) = resolve_statistic(statistic) else {
        return Err(unsupported_statistic_message(statistic));
    };
    // `describe` asks what the TABLE contains, so it needs no region — and
    // refusing it over an uncovered place would deny the model the one lookup
    // that could have told it which places the table actually offers.
    let resolved_region = if describe {
        None
    } else {
        match resolve_region(region) {
            Some(resolved) => Some(resolved),
            None => return Err(unsupported_region_message(region)),
        }
    };

    let base = state.information_core_base_url.trim_end_matches('/');
    let request = match resolved_region {
        None => state.http_client.get(format!(
            "{base}/api/v1/statistics/metadata?table={}",
            statistic.table
        )),
        Some(region) => state
            .http_client
            .post(format!("{base}/api/v1/statistics/query"))
            .json(&statistics_selection(statistic, region.code)),
    };
    let request = if state.information_core_internal_key.is_empty() {
        request
    } else {
        request.header("x-internal-api-key", &state.information_core_internal_key)
    };

    let resp = request
        .send()
        .await
        .map_err(|e| format!("information-core unreachable: {e}"))?;
    if !resp.status().is_success() {
        return Err(statistics_status_message(resp.status().as_u16()));
    }
    let envelope: StatisticsEnvelope = resp
        .json()
        .await
        .map_err(|e| format!("information-core returned an unparseable SSB payload: {e}"))?;

    let payload = match resolved_region {
        None => statistics_metadata_summary(statistic, &envelope.data),
        Some(region) => statistics_figure(statistic, region, &envelope.data)?,
    };
    // TOON, not the JSON-stat2 SSB sent: that payload spends several hundred
    // tokens of dimension/category scaffolding to deliver one cell, and
    // prompt-facing compaction of an already-extracted value is exactly what
    // mp-toon is for.
    let toon = mp_toon::encode(&payload);
    tracing::debug!(
        statistic = statistic.key,
        region = resolved_region.map_or("", |region| region.code),
        tokens = mp_toon::estimate_tokens(&toon),
        "get_statistics result encoded as TOON"
    );
    Ok(toon)
}

// ---------------- Sleep ----------------

/// Sleeps for the requested duration (capped at `MAX_SLEEP_MS`).
///
/// # Errors
///
/// Infallible in practice; returns `Result` to match the gRPC handler contract.
pub async fn handle_sleep(req: SleepRequest) -> Result<SleepResponse, Status> {
    let requested = req.duration_ms.max(0);
    let actual = requested.min(MAX_SLEEP_MS);
    if requested > MAX_SLEEP_MS {
        warn!(
            requested_ms = requested,
            cap_ms = MAX_SLEEP_MS,
            "Sleep duration capped"
        );
    }
    tokio::time::sleep(Duration::from_millis(u64::try_from(actual).unwrap_or(0))).await;
    Ok(SleepResponse {
        request_id: req.request_id,
        actual_ms: actual,
    })
}

// ---------------- RemoteTrigger ----------------

/// Issues an outbound HTTP request to a remote endpoint (dev-mode helper, SSRF-guarded).
///
/// # Errors
///
/// Returns `Status::invalid_argument` for a missing/invalid URL or disallowed scheme,
/// `Status::permission_denied` for an SSRF-unsafe host, or `Status::internal` if the
/// HTTP client cannot be built.
pub async fn handle_remote_trigger(
    req: RemoteTriggerRequest,
) -> Result<RemoteTriggerResponse, Status> {
    let url = req.url.trim();
    if url.is_empty() {
        return Err(Status::invalid_argument("url is required"));
    }
    let parsed = reqwest::Url::parse(url)
        .map_err(|e| Status::invalid_argument(format!("invalid url: {e}")))?;

    // Scheme must be http(s). FTP, file://, gopher://, etc. are
    // out-of-scope and a classic SSRF vector.
    match parsed.scheme() {
        "http" | "https" => {}
        other => {
            return Err(Status::invalid_argument(format!(
                "scheme not allowed: {other}"
            )))
        }
    }

    // SSRF guard: reject literal-IP hosts that fall inside loopback,
    // RFC1918, link-local, multicast, or unique-local IPv6 ranges.
    // Hostname-based targets pass — we trust DNS to resolve to
    // routable addresses for this dev-mode helper. Production should
    // delegate to Quarry's security engine (which does TOCTOU-safe
    // resolution + per-hop checks) by adding a Quarry endpoint for
    // generic HTTP egress; out of scope for this wave.
    if let Some(host) = parsed.host_str() {
        if let Ok(addr) = host.parse::<std::net::IpAddr>() {
            if !is_egress_safe(addr) {
                return Err(Status::permission_denied(format!(
                    "ssrf: refusing to call private/loopback ip: {addr}"
                )));
            }
        }
    }

    let timeout_ms = u64::try_from(req.timeout_ms.clamp(100, MAX_REMOTE_TIMEOUT_MS)).unwrap_or(100);
    let method = if req.method.is_empty() {
        "POST".to_string()
    } else {
        req.method.to_uppercase()
    };

    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(timeout_ms))
        // Block redirects so a clever target can't bounce us into a
        // private network.
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| Status::internal(format!("http client: {e}")))?;

    let method = reqwest::Method::from_bytes(method.as_bytes())
        .map_err(|e| Status::invalid_argument(format!("invalid http method: {e}")))?;
    let mut http_req = client.request(method, parsed.clone());
    if !req.content_type.is_empty() {
        http_req = http_req.header(reqwest::header::CONTENT_TYPE, &req.content_type);
    }
    if !req.body.is_empty() {
        http_req = http_req.body(req.body.clone());
    }

    let resp = http_req
        .send()
        .await
        .map_err(|e| Status::unavailable(format!("remote: {e}")))?;
    let status_code = i32::from(resp.status().as_u16());
    let final_url = resp.url().to_string();
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let mut body = resp
        .bytes()
        .await
        .map_err(|e| Status::internal(format!("read body: {e}")))?
        .to_vec();
    if body.len() > MAX_REMOTE_BODY_BYTES {
        body.truncate(MAX_REMOTE_BODY_BYTES);
    }

    Ok(RemoteTriggerResponse {
        request_id: req.request_id,
        status_code,
        body,
        content_type,
        final_url,
    })
}

/// True when an IPv4/IPv6 literal is safe to dial. False for loopback,
/// link-local, broadcast, multicast, private (RFC1918 / RFC4193),
/// documentation, and unspecified addresses.
fn is_egress_safe(addr: IpAddr) -> bool {
    match addr {
        IpAddr::V4(v4) => {
            if v4.is_loopback() || v4.is_private() || v4.is_link_local() {
                return false;
            }
            if v4.is_broadcast() || v4.is_multicast() || v4.is_unspecified() {
                return false;
            }
            if v4.is_documentation() {
                return false;
            }
            // RFC6598 shared address space (CGN). Not flagged by the
            // stdlib helpers; the 100.64.0.0/10 range routes to
            // carrier-grade NAT space inside ISPs and should not be a
            // valid egress target from us.
            let oct = v4.octets();
            if oct[0] == 100 && (oct[1] & 0xC0) == 64 {
                return false;
            }
            true
        }
        IpAddr::V6(v6) => {
            if v6.is_loopback() || v6.is_multicast() || v6.is_unspecified() {
                return false;
            }
            let segs = v6.segments();
            // fc00::/7 — unique local
            if (segs[0] & 0xfe00) == 0xfc00 {
                return false;
            }
            // fe80::/10 — link local
            if (segs[0] & 0xffc0) == 0xfe80 {
                return false;
            }
            // ::ffff:0:0/96 — IPv4-mapped; re-check the embedded v4.
            if segs[0] == 0
                && segs[1] == 0
                && segs[2] == 0
                && segs[3] == 0
                && segs[4] == 0
                && segs[5] == 0xffff
            {
                let v4 = std::net::Ipv4Addr::new(
                    (segs[6] >> 8) as u8,
                    (segs[6] & 0xff) as u8,
                    (segs[7] >> 8) as u8,
                    (segs[7] & 0xff) as u8,
                );
                return is_egress_safe(IpAddr::V4(v4));
            }
            true
        }
    }
}

// ---------------- SendMessage ----------------

/// Rejects the legacy free-form NATS publishing RPC.
///
/// # Errors
///
/// Always returns `Status::failed_precondition`. The previous prefix allowlist
/// accepted caller-selected ambient subjects, bypassing capability policy and
/// approval enforcement. The RPC remains in the protocol only for backward
/// compatibility; it must not publish through any backend.
pub fn handle_send_message(
    _state: &AppState,
    _req: SendMessageRequest,
) -> Result<SendMessageResponse, Status> {
    Err(Status::failed_precondition(
        "send_message is quarantined: free-form NATS publishing is disabled for the secure MVP",
    ))
}

// ---------------- SyntheticOutput ----------------

/// Echoes a payload back after an optional delay (test/synthetic helper).
///
/// # Errors
///
/// Infallible in practice; returns `Result` to match the gRPC handler contract.
pub async fn handle_synthetic_output(
    req: SyntheticOutputRequest,
) -> Result<SyntheticOutputResponse, Status> {
    let delay_ms = u64::try_from(req.delay_ms.clamp(0, MAX_SLEEP_MS)).unwrap_or(0);
    if delay_ms > 0 {
        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
    }
    Ok(SyntheticOutputResponse {
        request_id: req.request_id,
        echoed_payload: req.payload,
    })
}

// ---------------- Error mapping helper ----------------

fn quarry_err_to_status(err: QuarryError) -> Status {
    match err {
        QuarryError::Unavailable => Status::unimplemented("quarry edge not configured"),
        QuarryError::Transport(e) => Status::unavailable(format!("quarry transport: {e}")),
        QuarryError::EmptyEnvelope => Status::internal("quarry: empty envelope"),
        QuarryError::Decode(e) => Status::internal(format!("quarry decode: {e}")),
        // The detailed cause (e.g. missing MODEL_GATEWAY_SERVICE_API_KEY /
        // AUTH_CORE_URL misconfiguration, or Auth Core refusing the
        // service-principal credential) is deliberately not echoed to the
        // caller/model, but must not be silently swallowed either — log it
        // so this is diagnosable from server logs instead of only ever
        // surfacing as an opaque "quarry authentication is unavailable".
        QuarryError::Authentication(detail) => {
            warn!(
                error = %detail,
                "quarry authentication failed; check MODEL_GATEWAY_SERVICE_API_KEY and the Auth Core service-principal credential"
            );
            Status::unavailable("quarry authentication is unavailable")
        }
        QuarryError::Typed { code, message, .. } => match code.as_str() {
            "BAD_REQUEST" | "INVALID_ARGUMENT" => Status::invalid_argument(message),
            "SECURITY_BLOCKED" | "FORBIDDEN" => {
                Status::permission_denied(format!("{code}: {message}"))
            }
            "RATE_LIMITED" => Status::resource_exhausted(message),
            "TIMEOUT" => Status::deadline_exceeded(message),
            _ => Status::internal(format!("{code}: {message}")),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    use wiremock::matchers::{body_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn state_with_quarry(base_url: String) -> AppState {
        let mut state = AppState::new();
        state.quarry = crate::quarry::Client::new(crate::quarry::Config {
            base_url,
            token: "test-token".to_owned(),
            timeout: Duration::from_secs(5),
        });
        state
    }

    fn web_search_request(query: &str) -> WebSearchRequest {
        WebSearchRequest {
            request_id: "request-test".to_owned(),
            org_id: "org-test".to_owned(),
            query: query.to_owned(),
            limit: 5,
            intent: "research".to_owned(),
            zdr: false,
            allow_paid_providers: false,
        }
    }

    /// An empty `/v1/search` response in the edge's own shape (top-level
    /// `results`, no `data` wrapper).
    fn search_ok() -> serde_json::Value {
        serde_json::json!({
            "query": "",
            "provider": "smart_router",
            "results": [],
            "count": 0,
        })
    }

    /// The options-taking handler exists so a caller can narrow the search to
    /// the turn's language and recency; this pins that they actually reach
    /// the edge under the names it reads, rather than stopping at the gateway.
    #[tokio::test]
    async fn web_search_options_reach_the_edge() {
        let quarry = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/search"))
            .and(body_json(serde_json::json!({
                "query": "strømpris",
                "limit": 5,
                "intent": "research",
                "zdr": false,
                "allow_paid_providers": false,
                "language": "nb",
                "time_range": "day",
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(search_ok()))
            .expect(1)
            .mount(&quarry)
            .await;

        handle_web_search_with_options(
            &state_with_quarry(quarry.uri()),
            web_search_request("strømpris"),
            &SearchOptions {
                language: Some("nb".to_owned()),
                time_range: Some("day".to_owned()),
                ..SearchOptions::default()
            },
        )
        .await
        .expect("search succeeds");
    }

    /// The pre-existing entry point must add no narrowing keys of its own —
    /// `body_json` is an exact match, so anything leaking into the default
    /// path beyond the always-present `allow_paid_providers` fails here.
    #[tokio::test]
    async fn web_search_without_options_adds_no_narrowing_keys() {
        let quarry = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/search"))
            .and(body_json(serde_json::json!({
                "query": "strømpris",
                "limit": 5,
                "intent": "research",
                "zdr": false,
                "allow_paid_providers": false,
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(search_ok()))
            .expect(1)
            .mount(&quarry)
            .await;

        handle_web_search(
            &state_with_quarry(quarry.uri()),
            web_search_request("strømpris"),
        )
        .await
        .expect("search succeeds");
    }

    /// The tiering grant has to travel from the proto request all the way onto
    /// the edge request body; a handler that only read it from `SearchOptions`
    /// would leave every gRPC caller silently on free providers.
    #[tokio::test]
    async fn the_requests_paid_provider_grant_reaches_the_edge() {
        let quarry = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/search"))
            .and(body_json(serde_json::json!({
                "query": "strømpris",
                "limit": 5,
                "intent": "research",
                "zdr": false,
                "allow_paid_providers": true,
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(search_ok()))
            .expect(1)
            .mount(&quarry)
            .await;

        let mut req = web_search_request("strømpris");
        req.allow_paid_providers = true;
        handle_web_search(&state_with_quarry(quarry.uri()), req)
            .await
            .expect("search succeeds");
    }

    /// The request is authoritative in the closing direction too: options
    /// assembled from turn context (or from model-authored tool arguments)
    /// must not be able to buy external egress the caller did not grant.
    #[tokio::test]
    async fn options_cannot_grant_paid_providers_the_request_withheld() {
        let quarry = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/search"))
            .and(body_json(serde_json::json!({
                "query": "strømpris",
                "limit": 5,
                "intent": "research",
                "zdr": false,
                "allow_paid_providers": false,
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(search_ok()))
            .expect(1)
            .mount(&quarry)
            .await;

        handle_web_search_with_options(
            &state_with_quarry(quarry.uri()),
            web_search_request("strømpris"),
            &SearchOptions {
                allow_paid_providers: true,
                ..SearchOptions::default()
            },
        )
        .await
        .expect("search succeeds");
    }

    #[test]
    fn ssrf_rejects_loopback_v4() {
        assert!(!is_egress_safe("127.0.0.1".parse().unwrap()));
    }

    #[test]
    fn ssrf_rejects_rfc1918() {
        for ip in ["10.0.0.1", "172.16.0.1", "192.168.1.1"] {
            assert!(
                !is_egress_safe(ip.parse().unwrap()),
                "{ip} should be blocked"
            );
        }
    }

    #[test]
    fn ssrf_rejects_cgn() {
        assert!(!is_egress_safe(IpAddr::V4(Ipv4Addr::new(100, 64, 0, 1))));
        assert!(!is_egress_safe(IpAddr::V4(Ipv4Addr::new(
            100, 127, 255, 254
        ))));
    }

    #[test]
    fn ssrf_allows_public_v4() {
        assert!(is_egress_safe("1.1.1.1".parse().unwrap()));
        assert!(is_egress_safe("8.8.8.8".parse().unwrap()));
    }

    #[test]
    fn ssrf_rejects_ipv6_loopback_and_ula() {
        assert!(!is_egress_safe(IpAddr::V6(Ipv6Addr::LOCALHOST)));
        assert!(!is_egress_safe("fc00::1".parse().unwrap()));
        assert!(!is_egress_safe("fe80::1".parse().unwrap()));
    }

    #[test]
    fn ssrf_rejects_v4_mapped_private() {
        // ::ffff:10.0.0.1
        assert!(!is_egress_safe("::ffff:10.0.0.1".parse().unwrap()));
    }

    #[test]
    fn ssrf_allows_public_v6() {
        // 2001:db8::1 is documentation but stdlib doesn't reject it
        // for v6; we don't either since dev fixtures use it. Real
        // public v6 is allowed.
        assert!(is_egress_safe("2606:4700:4700::1111".parse().unwrap()));
    }

    // `handle_sleep` really sleeps, so at the 60s cap this burned a full
    // minute of wall clock per run. `start_paused` puts the test on tokio's
    // virtual clock: the timer is still awaited and the cap still asserted,
    // but the runtime advances time instantly instead of waiting.
    #[tokio::test(start_paused = true)]
    async fn sleep_caps_at_max() {
        let r = handle_sleep(SleepRequest {
            request_id: "t".into(),
            org_id: String::new(),
            duration_ms: 10_000_000,
            reason: String::new(),
        })
        .await
        .unwrap();
        assert_eq!(r.actual_ms, MAX_SLEEP_MS);
    }

    #[tokio::test]
    async fn synthetic_output_echoes() {
        let r = handle_synthetic_output(SyntheticOutputRequest {
            request_id: "t".into(),
            org_id: String::new(),
            payload: "hello".into(),
            delay_ms: 0,
        })
        .await
        .unwrap();
        assert_eq!(r.echoed_payload, "hello");
    }

    #[tokio::test]
    async fn send_message_is_quarantined_without_publishing_ambient_subjects() {
        let state = AppState::new();

        for subject in ["agents.worker-1", "org.org-test.events", "notify.user-test"] {
            let error = handle_send_message(
                &state,
                SendMessageRequest {
                    request_id: "request-test".to_owned(),
                    org_id: "org-test".to_owned(),
                    subject: subject.to_owned(),
                    payload_json: r#"{\"message\":\"must not publish\"}"#.to_owned(),
                    idempotency_key: "idempotency-test".to_owned(),
                },
            )
            .expect_err("ambient SendMessage must be quarantined");

            assert_eq!(error.code(), tonic::Code::FailedPrecondition);
        }

        assert!(
            state.publisher.drain().is_empty(),
            "quarantined SendMessage must not reach any publisher backend"
        );
    }

    #[test]
    fn resolve_location_finds_known_city() {
        assert_eq!(resolve_location("Bergen"), Some((60.3913, 5.3221)));
        assert_eq!(resolve_location("Trondheim"), Some((63.4305, 10.3951)));
    }

    #[test]
    fn resolve_location_is_case_insensitive() {
        assert_eq!(resolve_location("bergen"), resolve_location("BERGEN"));
        assert_eq!(resolve_location("  Bergen  "), resolve_location("bergen"));
    }

    #[test]
    fn resolve_location_folds_norwegian_characters() {
        // "Tromsø" must resolve the same entry regardless of how æøå are cased
        // or whether the caller typed the ASCII-folded form.
        assert_eq!(resolve_location("Tromsø"), Some((69.6492, 18.9553)));
        assert_eq!(resolve_location("tromsø"), resolve_location("Tromsø"));
        assert_eq!(resolve_location("TROMSØ"), resolve_location("Tromsø"));
    }

    /// An unrecognized city must NOT silently become Oslo. It used to: asking
    /// for Paris returned real, confident, Oslo-labelled data, and nothing
    /// downstream could tell it had answered a different question than the one
    /// asked. Refusing is the only honest outcome.
    #[test]
    fn resolve_location_refuses_a_city_it_cannot_place() {
        assert_eq!(resolve_location("Atlantis"), None);
        assert_eq!(resolve_location("Paris"), None, "non-Norwegian city");
        assert_eq!(
            resolve_location("Ålesund"),
            None,
            "uncovered Norwegian city"
        );
        assert_eq!(resolve_location("Oslu"), None, "typo must not resolve");
    }

    /// The refusal has to be actionable, not just a failure: it names what IS
    /// covered and points at the tool that can answer instead.
    #[test]
    fn unsupported_location_message_names_coverage_and_the_fallback() {
        let message = unsupported_location_message("Paris");
        assert!(message.contains("Paris"), "must echo what was asked for");
        assert!(message.contains("oslo") && message.contains("bergen"));
        assert!(
            message.contains("web_search"),
            "must point at the tool that can answer"
        );
        assert!(
            message.contains("Do not answer with another city"),
            "must forbid the silent-substitution failure this replaced"
        );
    }

    /// An empty location is a different case entirely: the user asked for "the
    /// weather" with no place, so Oslo — information-core's own default — is
    /// the right answer, not a refusal.
    #[test]
    fn resolve_location_defaults_to_oslo_when_empty() {
        let oslo = Some((DEFAULT_WEATHER_LAT, DEFAULT_WEATHER_LON));
        assert_eq!(resolve_location(""), oslo);
        assert_eq!(resolve_location("   "), oslo);
    }

    #[test]
    fn format_weather_summary_includes_current_and_forecast() {
        let payload = WeatherPayload {
            current: WeatherCurrent {
                condition: "Cloudy".to_owned(),
                location: "Oslo".to_owned(),
                temperature: 12,
                wind_speed: 3,
                precipitation: 0.5,
                humidity: 70,
            },
            forecast: vec![WeatherForecastDay {
                date: "2026-07-31".to_owned(),
                condition: "Rain".to_owned(),
                temperature: WeatherForecastMinMax { max: 14, min: 8 },
            }],
        };
        let summary = format_weather_summary(&payload);
        assert!(summary.contains("Oslo"));
        assert!(summary.contains("Cloudy"));
        assert!(summary.contains("12"));
        assert!(summary.contains("Forecast"));
        assert!(summary.contains("Rain"));
        assert!(summary.contains("high 14"));
        assert!(summary.contains("low 8"));
    }

    fn state_with_information_core(base_url: String) -> AppState {
        let mut state = AppState::new();
        state.information_core_base_url = base_url;
        state
    }

    /// A single-cell JSON-stat2 reply in information-core's envelope — the
    /// shape `POST /api/v1/statistics/query` returns for table 01222.
    fn ssb_population_envelope() -> serde_json::Value {
        serde_json::json!({
            "source": { "provider": "ssb", "dataset": "pxwebapi-v2", "license": "CC-BY-4.0" },
            "tableId": "01222",
            "queryHash": "hash-test",
            "data": {
                "class": "dataset",
                "label": "01222: Population and changes during the quarter",
                "source": "Statistics Norway",
                "updated": "2026-08-20T06:00:00Z",
                "id": ["Region", "ContentsCode", "Tid"],
                "size": [1, 1, 1],
                "dimension": {
                    "Region": {
                        "label": "region",
                        "category": { "index": { "0301": 0 }, "label": { "0301": "Oslo" } }
                    },
                    "ContentsCode": {
                        "label": "contents",
                        "category": {
                            "index": { "Folketallet11": 0 },
                            "label": { "Folketallet11": "Population at the end of the quarter" },
                            "unit": { "Folketallet11": { "base": "persons", "decimals": 0 } }
                        }
                    },
                    "Tid": {
                        "label": "quarter",
                        "category": { "index": { "2026K2": 0 }, "label": { "2026K2": "2026K2" } }
                    }
                },
                "value": [729_437],
                "role": { "time": ["Tid"], "metric": ["ContentsCode"], "geo": ["Region"] }
            }
        })
    }

    /// The measured incident, end to end: the figure the SSB page buried in a
    /// hydration payload is reachable as a first-class API answer, and it
    /// arrives WITH the quarter it describes. A bare "729437" is a correct
    /// answer this quarter and a wrong one next quarter.
    #[tokio::test]
    async fn get_statistics_answers_a_known_statistic_with_its_period_and_source() {
        let information_core = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v1/statistics/query"))
            // Pins the whole bounded selection: a wrong table, contents code,
            // region code, or period expression fails here rather than in
            // production against the real SSB. The contents code is the part
            // that actually went wrong once — `Folketallet` is the page's
            // display label, not a code, and SSB refused the query outright —
            // so this assertion is the regression guard for that, and every
            // value in it has been confirmed against the live API.
            .and(body_json(serde_json::json!({
                "table": "01222",
                "selection": [
                    { "variableCode": "Region", "valueCodes": ["0301"] },
                    { "variableCode": "ContentsCode", "valueCodes": ["Folketallet11"] },
                    { "variableCode": "Tid", "valueCodes": ["top(1)"] },
                ]
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(ssb_population_envelope()))
            .expect(1)
            .mount(&information_core)
            .await;

        let summary = handle_get_statistics(
            &state_with_information_core(information_core.uri()),
            "population",
            "Oslo",
            false,
        )
        .await
        .expect("a curated statistic for a curated region resolves");

        assert!(summary.contains("729437"), "the figure itself: {summary}");
        assert!(
            summary.contains("2026K2"),
            "the period must travel with the figure: {summary}"
        );
        assert!(summary.contains("Oslo"), "the region: {summary}");
        assert!(summary.contains("persons"), "the unit: {summary}");
        assert!(
            summary.contains("table 01222"),
            "the source the SSB page itself cites: {summary}"
        );
        assert!(
            !summary.contains("\"class\"") && !summary.contains("dimension"),
            "raw JSON-stat2 must not reach the model: {summary}"
        );
    }

    /// Norwegian spellings and casings reach the same region code, exactly as
    /// they do for `get_weather`.
    #[tokio::test]
    async fn get_statistics_folds_norwegian_region_spellings() {
        assert_eq!(
            resolve_region("Tromsø").map(|region| region.code),
            Some("5501")
        );
        assert_eq!(resolve_region("TROMSO").map(|r| r.code), Some("5501"));
        // No region at all is the national figure, not a refusal: "Norway's
        // population" names no municipality.
        assert_eq!(resolve_region("").map(|r| r.code), Some("0"));
        assert_eq!(resolve_region("Norge").map(|r| r.code), Some("0"));
    }

    /// An unknown statistic must fail by NAMING what is available. The failure
    /// this forbids is specific: told only "unknown", a model's next move is to
    /// supply an SSB table id it invented, and five digits look equally real
    /// whether or not the table exists.
    #[tokio::test]
    async fn get_statistics_refuses_an_unknown_statistic_naming_what_is_available() {
        let error = handle_get_statistics(&AppState::new(), "unemployment rate", "Oslo", false)
            .await
            .expect_err("an uncurated statistic must not be answered");

        assert!(error.contains("unemployment rate"), "{error}");
        assert!(
            error.contains("'population'"),
            "must name what IS available: {error}"
        );
        assert!(
            error.contains("do not pass an SSB table id"),
            "must forbid the invented-table-id recovery: {error}"
        );
        assert!(error.contains("web_search"), "{error}");
    }

    #[tokio::test]
    async fn get_statistics_refuses_an_uncovered_region_naming_its_coverage() {
        let error = handle_get_statistics(&AppState::new(), "population", "Ålesund", false)
            .await
            .expect_err("an uncurated region must not be answered");

        assert!(error.contains("Ålesund"), "{error}");
        assert!(error.contains("oslo") && error.contains("bergen"), "{error}");
        assert!(
            error.contains("Do not answer with another region"),
            "must forbid the silent substitution: {error}"
        );
    }

    /// An information-core that has no SSB source wired up degrades honestly:
    /// it says so, tells the model not to retry, and hands it a real
    /// alternative — instead of leaving it to invent a population.
    #[tokio::test]
    async fn get_statistics_degrades_honestly_when_the_ssb_source_is_unconfigured() {
        let information_core = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v1/statistics/query"))
            .respond_with(ResponseTemplate::new(503).set_body_json(serde_json::json!({
                "error": { "code": "source_not_configured" }
            })))
            .mount(&information_core)
            .await;

        let error = handle_get_statistics(
            &state_with_information_core(information_core.uri()),
            "population",
            "Oslo",
            false,
        )
        .await
        .expect_err("a 503 must not become an answer");

        assert!(error.contains("no SSB statistics source configured"), "{error}");
        assert!(error.contains("Do not retry"), "{error}");
        assert!(
            !error.chars().any(|c| c.is_ascii_digit()) || !error.contains("729"),
            "an outage must never carry a figure: {error}"
        );
    }

    /// An unreachable information-core is an error, never an empty success —
    /// the same rule `handle_get_weather` follows.
    #[tokio::test]
    async fn get_statistics_reports_an_unreachable_information_core() {
        // Port 0 is never listening, so this fails at connect without waiting.
        let error = handle_get_statistics(
            &state_with_information_core("http://127.0.0.1:1".to_owned()),
            "population",
            "Oslo",
            false,
        )
        .await
        .expect_err("an unreachable upstream must not be answered");
        assert!(error.contains("information-core unreachable"), "{error}");
    }

    /// A cell SSB has not published (null) must fail, not become a figure.
    #[test]
    fn statistics_figure_refuses_an_unpublished_cell() {
        let mut envelope = ssb_population_envelope();
        envelope["data"]["value"] = serde_json::json!([null]);
        let error = statistics_figure(
            &CURATED_STATISTICS[0],
            resolve_region("Oslo").unwrap(),
            &envelope["data"],
        )
        .expect_err("a null cell is not a figure");
        assert!(error.contains("empty, confidential, or not yet published"), "{error}");
    }

    /// The guard that makes a stale municipality code safe. Municipality
    /// numbers are renumbered by county reforms; if a curated code now denotes
    /// a different place, SSB answers happily and correctly about THAT place,
    /// and only this check can tell the difference.
    #[test]
    fn statistics_figure_refuses_a_region_ssb_disagrees_about() {
        let mut envelope = ssb_population_envelope();
        envelope["data"]["dimension"]["Region"]["category"]["label"]["0301"] =
            serde_json::json!("Bergen");
        let error = statistics_figure(
            &CURATED_STATISTICS[0],
            resolve_region("Oslo").unwrap(),
            &envelope["data"],
        )
        .expect_err("a region mismatch must not be answered");
        assert!(error.contains("refusing to answer"), "{error}");
        assert!(error.contains("Bergen"), "must name what came back: {error}");
    }

    /// SSB labels a municipality "0301 Oslo" in some tables and "Oslo" in
    /// others; both denote the region we asked for, so neither may trip the
    /// mismatch guard.
    #[test]
    fn region_label_guard_accepts_ssbs_code_prefixed_labels() {
        assert!(!region_label_disagrees("oslo", "0301 Oslo"));
        assert!(!region_label_disagrees("tromso", "Tromsø"));
        assert!(!region_label_disagrees("whole country", "The whole country"));
        assert!(region_label_disagrees("oslo", "Bergen"));
    }

    /// A period the payload cannot supply is a hard failure: the figure alone
    /// is how a correct number becomes a wrong answer six months later.
    #[test]
    fn statistics_figure_refuses_a_payload_without_a_period() {
        let mut envelope = ssb_population_envelope();
        envelope["data"]["dimension"]["Tid"]["category"]["index"] =
            serde_json::json!({ "2026K1": 0, "2026K2": 1 });
        let error = statistics_figure(
            &CURATED_STATISTICS[0],
            resolve_region("Oslo").unwrap(),
            &envelope["data"],
        )
        .expect_err("an ambiguous period must not be answered");
        assert!(error.contains("more than one period"), "{error}");
    }

    /// `describe` is how the curated map grows without guesswork: it reports
    /// the table's real variables instead of a figure.
    #[tokio::test]
    async fn get_statistics_describe_reports_the_tables_shape_not_a_figure() {
        let information_core = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/statistics/metadata"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "tableId": "01222",
                "data": {
                    "label": "01222: Population and changes during the quarter",
                    "dimension": {
                        "Region": {
                            "label": "region",
                            "category": { "index": { "0": 0, "0301": 1, "4601": 2 } }
                        },
                        "ContentsCode": {
                            "label": "contents",
                            "category": { "index": { "Folketallet": 0, "Fodte": 1 } }
                        }
                    }
                }
            })))
            .expect(2)
            .mount(&information_core)
            .await;

        let state = state_with_information_core(information_core.uri());
        let summary = handle_get_statistics(&state, "population", "", true)
            .await
            .expect("describe resolves for a curated statistic");
        // `describe` asks about the TABLE, so an uncovered region must not
        // block it — that lookup is how the model finds out which regions the
        // table offers in the first place.
        handle_get_statistics(&state, "population", "Ålesund", true)
            .await
            .expect("describe needs no region");

        assert!(summary.contains("01222"), "{summary}");
        assert!(summary.contains("Region"), "{summary}");
        assert!(summary.contains("Folketallet"), "{summary}");
        assert!(
            !summary.contains("729437"),
            "describe reports shape, never a figure: {summary}"
        );
    }

    #[test]
    fn format_weather_summary_without_forecast_omits_forecast_section() {
        let payload = WeatherPayload {
            current: WeatherCurrent {
                condition: "Clear".to_owned(),
                location: "Bergen".to_owned(),
                temperature: 18,
                wind_speed: 1,
                precipitation: 0.0,
                humidity: 40,
            },
            forecast: vec![],
        };
        let summary = format_weather_summary(&payload);
        assert!(!summary.contains("Forecast"));
    }
}
