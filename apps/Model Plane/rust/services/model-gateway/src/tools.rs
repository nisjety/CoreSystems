//! Wave 10a — handlers for the 5 trivial tool RPCs, plus the plain-chat-only
//! `get_weather` builtin.
//!
//! - `WebSearch`:       `quarry::Client::search` proxy
//! - `GetWeather`:      information-core (Yr/met.no) proxy, plain-chat builtin only
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

use crate::quarry::QuarryError;
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

/// Runs a web search via the Quarry edge.
///
/// # Errors
///
/// Returns `Status::unimplemented` if Quarry is not configured, or maps a
/// Quarry search failure to a `Status`.
pub async fn handle_web_search(
    state: &AppState,
    req: WebSearchRequest,
) -> Result<WebSearchResponse, Status> {
    if !state.quarry.available() {
        return Err(Status::unimplemented("quarry edge not configured"));
    }
    let results = state
        .quarry
        .search(&req.query, req.limit, &req.intent, &req.org_id, req.zdr)
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
    inference_bearer: &str,
    session_bearer: &str,
    run_id: &str,
    org_id: &str,
    user_id: &str,
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
        hook_context: String::new(),
        org_id: org_id.to_owned(),
        user_id: user_id.to_owned(),
        zdr,
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
    let normalized = trimmed
        .to_lowercase()
        .replace('æ', "ae")
        .replace('ø', "o")
        .replace('å', "a");
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

    #[tokio::test]
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
        assert_eq!(resolve_location("Ålesund"), None, "uncovered Norwegian city");
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
