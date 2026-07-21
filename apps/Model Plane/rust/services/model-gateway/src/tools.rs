//! Wave 10a — handlers for the 5 trivial tool RPCs.
//!
//! - `WebSearch`:       `quarry::Client::search` proxy
//! - Sleep:           `tokio::time::sleep` with server-side cap
//! - `RemoteTrigger`:   outbound HTTP webhook + SSRF guard
//! - `SendMessage`:     quarantined legacy RPC (fails closed)
//! - `SyntheticOutput`: deterministic echo (test/dev only)
//!
//! Each handler is intentionally < 60 LOC. The corresponding v2 Python
//! files totalled ~650 LOC; the bulk of that was Pydantic validation
//! and framework glue that Tonic + prost give us for free.

use std::net::IpAddr;
use std::time::Duration;

use tonic::Status;
use tracing::warn;

use mp_contracts::model_plane::v1::{
    RemoteTriggerRequest, RemoteTriggerResponse, SendMessageRequest, SendMessageResponse,
    SleepRequest, SleepResponse, SyntheticOutputRequest, SyntheticOutputResponse, WebSearchRequest,
    WebSearchResponse, WebSearchResult,
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
}
