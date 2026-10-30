//! `/v1/extract` — multi-URL structured extraction (OSS-parity P1 2A).
//!
//! Take a set of URLs plus an optional JSON Schema and
//! return structured data per source. Agent-harness shape: **bounded fan-out**
//! (capped URL count), per-source isolation (one failure never sinks the
//! batch), and a typed per-source result envelope.
//!
//! Scope note: wildcard expansion (`domain/*`) is the two-step `/v1/map` →
//! `/v1/extract` flow for Verevon — call `/v1/map` to discover URLs, then pass
//! them here. This route owns the extract fan-out + schema enforcement.
//!
//! Read-only (no durable writes), org-scoped for metering, ZDR-safe.

use std::sync::Arc;

use axum::{extract::State, http::StatusCode, response::IntoResponse, Extension, Json};
use serde::{Deserialize, Serialize};
use url::Url;

use quarry_core::error::{ErrorCode, QuarryError};
use quarry_core::privacy::PrivacyPolicy;
use quarry_runtime::ai_formats::AiFormatRunner;
use quarry_runtime::driver::FetchHints;
use quarry_runtime::driver_plan::{plan_from_signals, DriverSignals};
use quarry_runtime::mp_client::ModelPlaneClient;
use quarry_transform::structured::StructuredData;

use crate::api_error::ApiError;
use crate::state::AppState;

const DEFAULT_MAX_URLS: usize = 10;
const MAX_MAX_URLS: usize = 25;
const MARKDOWN_CAP: usize = 20_000;
/// Default back-off window advertised on a 429 so callers can retry without
/// guessing. Mirrors the 60s hint the map/search/scrape paths use.
const RATE_LIMITED_RETRY_AFTER_S: u64 = 60;

#[derive(Debug, Deserialize)]
pub struct ExtractRequest {
    pub urls: Vec<String>,
    /// JSON Schema for structured extraction. When omitted, the route returns
    /// cleaned markdown per source instead of structured data.
    #[serde(default)]
    pub schema: Option<serde_json::Value>,
    /// Optional natural-language hint (advisory; structured extraction is
    /// schema-driven).
    #[serde(default)]
    #[allow(dead_code)] // scaffolding: folded into schema-guided prompt in follow-up
    pub prompt: Option<String>,
    #[serde(default)]
    pub max_urls: Option<u32>,
    #[serde(default)]
    pub zdr: Option<bool>,
    #[serde(default)]
    pub privacy: Option<PrivacyPolicy>,
    #[serde(default)]
    pub signals: Option<DriverSignals>,
}

#[derive(Debug, Serialize)]
pub struct ExtractItem {
    pub url: String,
    /// "ok" (structured), "fetched" (markdown only), or "error".
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub markdown: Option<String>,
    /// Machine-readable facts harvested from the page's markup, when the page
    /// carried any. Additive and absent otherwise, so a source with no
    /// structured data serializes exactly as it did before this channel
    /// existed. Independent of `status`: a schema-guided extraction and a
    /// markdown one read the same page, and the harvest belongs to the page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured: Option<StructuredData>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ExtractResponse {
    pub results: Vec<ExtractItem>,
    pub count: usize,
    pub requested: usize,
}

// OSS-parity 3D — shared structured error envelope (extends the old
// `{error, code, hint}` shape with optional rate-limit fields). All extract
// error paths serialize through this type so clients parse one shape.
type ErrorBody = ApiError;

fn bad_request(message: impl Into<String>) -> ErrorBody {
    ApiError::new("BAD_REQUEST", message)
}

fn unsupported(message: impl Into<String>, hint: impl Into<String>) -> ErrorBody {
    ApiError::new("UNSUPPORTED", message).with_hint(hint)
}

/// Validate + dedup + cap the target URL list (bounded fan-out). Drops
/// non-http(s) and unparseable URLs. Order-preserving.
pub(crate) fn expand_targets(urls: &[String], max: usize) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for raw in urls {
        let trimmed = raw.trim();
        match Url::parse(trimmed) {
            Ok(u) if matches!(u.scheme(), "http" | "https") => {
                if seen.insert(u.as_str().to_string()) {
                    out.push(u.as_str().to_string());
                    if out.len() >= max {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Largest index at or below `at` that `s` can be split on.
fn nearest_char_boundary(s: &str, at: usize) -> usize {
    (0..=at.min(s.len()))
        .rev()
        .find(|i| s.is_char_boundary(*i))
        .unwrap_or(0)
}

/// One fetched page, read by both extraction channels.
struct ExtractedPage {
    markdown: String,
    /// `None` when the harvest found nothing — the overwhelming majority of
    /// pages — so the field never reaches the wire for them.
    structured: Option<StructuredData>,
}

async fn fetch_page(
    state: &AppState,
    url: &Url,
    org_id: &str,
    privacy: PrivacyPolicy,
    signals: DriverSignals,
) -> Result<ExtractedPage, QuarryError> {
    let plan = plan_from_signals(signals);
    let driver = state.drivers.build_driver(&plan);
    let hints = FetchHints {
        org_id: org_id.to_string(),
        privacy,
        ..FetchHints::default()
    };
    match driver.fetch_conditional(url, &hints).await {
        Ok(resp) if (200..300).contains(&resp.status) => {
            let html = String::from_utf8_lossy(&resp.body);
            // Second channel over the same document. Readability strips
            // `script`, so a page that publishes its numbers as hydration
            // state returns a nav shell and nothing else — this is where
            // those numbers are recovered. The harvester is infallible by
            // contract (a malformed payload is skipped, the rest of the page
            // proceeds), so it cannot fail a fetch that otherwise succeeded.
            let structured = quarry_transform::extract_structured(&html);
            let mut md = quarry_transform::readability::html_to_readable_markdown(&html);
            if md.len() > MARKDOWN_CAP {
                // `String::truncate` panics when the byte cap lands inside a
                // multi-byte char — one accented word straddling 20 000 bytes
                // is enough, and Norwegian sources supply them — so back off
                // to the nearest boundary. A panic here would take the whole
                // batch down, not just this source.
                md.truncate(nearest_char_boundary(&md, MARKDOWN_CAP));
            }
            Ok(ExtractedPage {
                markdown: md,
                structured: Some(structured).filter(|s| !s.is_empty()),
            })
        }
        Ok(resp) => Err(QuarryError::new(
            ErrorCode::UpstreamBlocked,
            format!("upstream status {}", resp.status),
        )),
        Err(e) => Err(e),
    }
}

/// `POST /v1/extract`. WS4.J: wraps `extract_run` so a ZDR run's metadata-only tombstone is
/// sent once, from the response status (off unless the flag is on; fail open). A per-URL
/// failure inside a 200 batch is not an outcome of the request.
pub async fn extract(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Json(req): Json<ExtractRequest>,
) -> axum::response::Response {
    let pending = crate::zdr_tombstone::Pending::begin(
        state.tombstones.as_ref(),
        claims.effective_zdr(req.zdr.unwrap_or(false)),
        &claims.org_id,
        crate::zdr_tombstone::Kind::Extract,
    );
    let response = extract_run(State(state), Extension(claims), Json(req))
        .await
        .into_response();
    if let Some(pending) = pending {
        pending.finish_status(response.status().as_u16());
    }
    response
}

async fn extract_run(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Json(req): Json<ExtractRequest>,
) -> impl IntoResponse {
    let max = req
        .max_urls
        .map(|m| m as usize)
        .unwrap_or(DEFAULT_MAX_URLS)
        .clamp(1, MAX_MAX_URLS);
    let targets = expand_targets(&req.urls, max);
    if targets.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(bad_request("no valid http(s) URLs in `urls`")),
        )
            .into_response();
    }

    // Structured extraction needs Model Plane. Without it, fall back to
    // returning cleaned markdown (still useful), unless a schema was requested.
    // QB-T17: the runner exists only when QUARRY_EDGE__AI_FORMATS is on and a model path exists.
    let ai_formats_on = state.model_plane.ai_formats;
    let runner: Option<AiFormatRunner> = match state.model_plane_url.as_deref() {
        Some(url) if !url.is_empty() && ai_formats_on => match ModelPlaneClient::new(url) {
            Ok(mut c) => {
                if let Some(provider) = state.service_token_provider.clone() {
                    c = c.with_token_provider(provider);
                } else if let Some(tok) =
                    state.model_plane_token.as_deref().filter(|t| !t.is_empty())
                {
                    c = c.with_bearer_token(tok);
                }
                Some(AiFormatRunner::new(Arc::new(c)))
            }
            Err(_) => None,
        },
        _ => None,
    };

    if req.schema.is_some() && runner.is_none() {
        return (
            StatusCode::NOT_IMPLEMENTED,
            Json(unsupported(
                "structured extraction is not available on this edge",
                "structured extraction needs QUARRY_EDGE__AI_FORMATS=1 and a granted Model Plane path (MODEL_PLANE_URL); omit `schema` to receive markdown",
            )),
        )
            .into_response();
    }

    // Body flag raised (never lowered) by the token's ZDR floor: see
    // `Claims::zdr_floor`.
    let zdr = claims.effective_zdr_mode(req.zdr);
    // WS7.E: every target is gated before the first fetch. One denied URL refuses the whole
    // request with a 403 that names the capability and never a URL.
    let decision = match crate::web_policy_edge::decide(
        &state.policy,
        &claims,
        zdr.is_active(),
        None,
    ) {
        Ok(decision) => decision,
        Err(error) => {
            return (
                StatusCode::FORBIDDEN,
                Json(ApiError::new("FORBIDDEN", error.message)),
            )
                .into_response();
        }
    };
    if let Err(error) =
        crate::web_policy_edge::gate_urls(&decision, targets.iter().map(String::as_str))
    {
        return (
            StatusCode::FORBIDDEN,
            Json(ApiError::new("FORBIDDEN", error.message)),
        )
            .into_response();
    }
    let privacy = req.privacy.unwrap_or_default().with_zdr(zdr);
    let signals = req.signals.unwrap_or_default();
    let mut results = Vec::with_capacity(targets.len());
    let mut rate_limited: Option<QuarryError> = None;
    for raw in &targets {
        let url = match Url::parse(raw) {
            Ok(u) => u,
            Err(_) => continue,
        };
        let item = match fetch_page(
            &state,
            &url,
            &claims.org_id,
            privacy.clone(),
            signals.clone(),
        )
        .await
        {
            Err(e) => {
                // A driver-side 429 short-circuits the whole batch the
                // same way `/v1/search` and `/v1/scrape` do: return a
                // single typed envelope and stop fanning out. Sinking
                // it into a per-item "error" string would tell a
                // caller their entire extract succeeded with one bad
                // source — they would then auto-retry the rest and
                // hammer the throttled host. The 429 surface this up
                // unambiguously.
                if e.code == ErrorCode::RateLimited {
                    rate_limited.get_or_insert(e);
                    break;
                }
                ExtractItem {
                    url: raw.clone(),
                    status: "error".into(),
                    data: None,
                    markdown: None,
                    structured: None,
                    error: Some(e.message),
                }
            }
            // Destructured up front so the two channels move independently:
            // the schema branch hands the prose to the model and keeps the
            // harvest for the response.
            Ok(ExtractedPage {
                markdown,
                structured,
            }) => match (&req.schema, &runner) {
                (Some(schema), Some(r)) => match r
                    .json_for_org(&claims.org_id, &markdown, schema.clone(), zdr)
                    .await
                {
                    Ok(jr) => ExtractItem {
                        url: raw.clone(),
                        status: "ok".into(),
                        data: Some(jr.data),
                        markdown: None,
                        structured,
                        error: None,
                    },
                    Err(e) => {
                        // A Model-Plane 429 hits the same fail-closed
                        // envelope shape so the caller can act on a
                        // single consistent signal across the route.
                        if e.code == ErrorCode::RateLimited {
                            rate_limited
                                .get_or_insert(QuarryError::new(ErrorCode::RateLimited, e.message));
                            break;
                        }
                        ExtractItem {
                            url: raw.clone(),
                            status: "error".into(),
                            data: None,
                            markdown: None,
                            // The page was read; the model call is what
                            // failed. The harvest is still the page's, and
                            // dropping it would throw away the one channel
                            // that needed no model at all.
                            structured,
                            error: Some(e.message),
                        }
                    }
                },
                _ => ExtractItem {
                    url: raw.clone(),
                    status: "fetched".into(),
                    data: None,
                    markdown: Some(markdown),
                    structured,
                    error: None,
                },
            },
        };
        results.push(item);
    }

    if let Some(e) = rate_limited {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(ApiError::rate_limited(
                e.message,
                RATE_LIMITED_RETRY_AFTER_S,
            )),
        )
            .into_response();
    }

    // Usage metering. WS6.C-e: `quarry.extract.url` counts the URLs fetched or extracted (failures
    // are free). The gate decides whether it is published (`on`), counted (`shadow`) or dropped
    // (`off`). Until `on`, the extract is still billed as one search query, as before.
    let request_id = crate::request_id::current();
    let billed_urls = results
        .iter()
        .filter(|item| matches!(item.status.as_str(), "ok" | "fetched"))
        .count();
    let mut attrs = crate::usage_attrs::UsageAttrs::new(
        crate::usage_attrs::Outcome::Ok,
        crate::usage_attrs::surface_for(&claims, None),
    );
    attrs.user_id = Some(claims.user_id.clone());
    attrs.counts = Some(serde_json::json!({ "urls": targets.len(), "items": billed_urls }));
    crate::usage_attrs::emit(
        &*state.usage,
        &[request_id.as_str()],
        &claims.org_id,
        quarry_runtime::usage_metrics::EXTRACT_URL,
        billed_urls as f64,
        &attrs,
        zdr.is_active(),
    )
    .await;
    if !crate::usage_attrs::corrected_rules(&*state.usage) {
        // D2-033: this id is server-minted, never the caller's `X-Request-Id`. The request
        // id is not unique across tenants and a client can repeat or oversize it, so using it
        // as the billing key let a client suppress its own billing (billing-core's dedupe is
        // by event id and refuses a repeat with a later `occurred_at`). A retry-safe key
        // needs the caller's Idempotency-Key and a stable `occurred_at`: see
        // depth/D-D-B3-egress-caps.md.
        let run_id: quarry_core::ids::kinds::RunKind = quarry_core::ids::Id::new();
        state
            .usage
            .meter(quarry_runtime::UsageEvent::new(
                run_id.to_string(),
                claims.org_id.clone(),
                quarry_runtime::usage_metrics::SEARCH_QUERY,
                targets.len() as f64,
                serde_json::json!({
                    "op": "extract",
                    "user_id": claims.user_id,
                    "urls": targets.len(),
                    "structured": req.schema.is_some(),
                }),
            ))
            .await;
    }

    let count = results.len();
    (
        StatusCode::OK,
        Json(ExtractResponse {
            results,
            count,
            requested: targets.len(),
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use quarry_core::output::DriverKind;
    use quarry_core::QuarryResult;
    use quarry_runtime::driver::Driver;
    use quarry_runtime::fetch::FetchResponse;

    /// Driver answering 200 with a caller-chosen body. `test_support`'s stub
    /// serves one fixed page, and these tests are precisely about what
    /// different page shapes produce.
    struct HtmlDriver(String);

    #[async_trait]
    impl Driver for HtmlDriver {
        fn kind(&self) -> DriverKind {
            DriverKind::Static
        }
        async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse> {
            Ok(FetchResponse {
                status: 200,
                final_url: url.clone(),
                headers: vec![],
                body: self.0.clone().into_bytes(),
                duration_ms: 1,
                served_by: DriverKind::Static,
                captures: None,
            })
        }
    }

    async fn extract_one(body: &str) -> serde_json::Value {
        let state = crate::test_support::test_state(Arc::new(HtmlDriver(body.to_string())));
        let response = extract(
            State(state),
            Extension(crate::test_support::claims_for_org("org_alpha")),
            Json(ExtractRequest {
                urls: vec!["https://ssb.example/kommunefakta".into()],
                schema: None,
                prompt: None,
                max_urls: None,
                zdr: None,
                privacy: None,
                signals: None,
            }),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        crate::test_support::response_json(response).await["results"][0].clone()
    }

    /// A meter that keeps every event, in the default (`Off`) browser-metrics mode.
    #[derive(Default)]
    struct KeptEvents(std::sync::Mutex<Vec<quarry_runtime::UsageEvent>>);

    #[async_trait]
    impl quarry_runtime::UsageMeter for KeptEvents {
        async fn meter(&self, event: quarry_runtime::UsageEvent) {
            self.0.lock().unwrap().push(event);
        }
    }

    async fn extract_under_request_id(
        usage: &Arc<KeptEvents>,
        request_id: &str,
    ) -> StatusCode {
        let mut state = crate::test_support::test_state(Arc::new(HtmlDriver(
            "<html><body><article><p>Prose.</p></article></body></html>".to_string(),
        )));
        state.usage = usage.clone();
        quarry_runtime::request_context::scope(request_id.to_string(), async {
            extract(
                State(state),
                Extension(crate::test_support::claims_for_org("org_alpha")),
                Json(ExtractRequest {
                    urls: vec!["https://ssb.example/kommunefakta".into()],
                    schema: None,
                    prompt: None,
                    max_urls: None,
                    zdr: None,
                    privacy: None,
                    signals: None,
                }),
            )
            .await
            .into_response()
            .status()
        })
        .await
    }

    #[tokio::test]
    async fn a_client_chosen_request_id_cannot_collapse_legacy_extract_billing() {
        // D2-033 (verifier finding): the billing event id is server-minted. A client that
        // repeats one `X-Request-Id` (or sends an oversize one) still produces one distinct,
        // billing-core-valid event per extract, so it cannot suppress its own billing.
        let usage = Arc::new(KeptEvents::default());
        let long_id = "r".repeat(128);
        for id in ["same_id", "same_id", long_id.as_str()] {
            assert_eq!(extract_under_request_id(&usage, id).await, StatusCode::OK);
        }
        let ids: Vec<String> = usage
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|event| event.metric == quarry_runtime::usage_metrics::SEARCH_QUERY)
            .map(|event| event.event_id.clone())
            .collect();
        assert_eq!(ids.len(), 3);
        let distinct: std::collections::HashSet<&String> = ids.iter().collect();
        assert_eq!(distinct.len(), 3, "each extract is its own billable event");
        for id in &ids {
            assert!(!id.contains("same_id") && !id.contains("rrrr"), "caller id must not be the key");
            assert!(
                quarry_runtime::usage::validate_event_id(id).is_ok(),
                "event id must satisfy billing-core's rule"
            );
        }
    }

    #[tokio::test]
    async fn page_without_structured_data_serializes_as_before() {
        let item =
            extract_one("<html><body><article><p>Just prose here.</p></article></body></html>")
                .await;
        assert_eq!(
            crate::test_support::json_keys(&item),
            vec!["markdown", "status", "url"],
            "a source with no harvest must carry no new key"
        );
    }

    #[tokio::test]
    async fn hydration_figures_reach_the_caller() {
        // The incident shape: the figure exists only inside an
        // `application/json` payload, which readability strips.
        let item = extract_one(
            "<html><body><article><p>Kommunefakta.</p></article>\
             <script type=\"application/json\">\
             {\"keyFigureTitle\":\"Folketallet\",\"number\":\"729 437\",\
             \"numberDescription\":\"personer\",\"time\":\"2. kvartal 2026\"}\
             </script></body></html>",
        )
        .await;
        let figure = &item["structured"]["figures"][0];
        assert_eq!(figure["label"], "Folketallet");
        assert_eq!(figure["value"], "729 437");
        assert_eq!(figure["unit"], "personer");
        assert_eq!(figure["period"], "2. kvartal 2026");
        // The prose channel is untouched by the addition.
        assert!(item["markdown"]
            .as_str()
            .is_some_and(|m| m.contains("Kommunefakta")));
    }

    #[tokio::test]
    async fn malformed_payload_does_not_fail_the_extraction() {
        let item = extract_one(
            "<html><body><article><p>Readable prose survives.</p></article>\
             <script type=\"application/json\">{\"broken\": </script>\
             </body></html>",
        )
        .await;
        assert_eq!(item["status"], "fetched");
        assert!(item["markdown"]
            .as_str()
            .is_some_and(|m| m.contains("Readable prose survives")));
        assert!(item.get("error").is_none());
    }

    #[test]
    fn markdown_cap_backs_off_to_a_char_boundary() {
        // `String::truncate` panics mid-codepoint, so the byte cap can never
        // be applied raw to text with multi-byte chars.
        let s = "æøå".repeat(10); // two bytes per char
        assert_eq!(nearest_char_boundary(&s, 5), 4, "an index inside a char");
        assert_eq!(nearest_char_boundary(&s, 6), 6, "an index already on one");
        assert_eq!(nearest_char_boundary(&s, s.len() + 99), s.len());
        let mut t = s.clone();
        t.truncate(nearest_char_boundary(&s, 5));
        assert_eq!(t.chars().count(), 2);
    }

    #[test]
    fn expand_dedups_and_caps() {
        let urls = vec![
            "https://a.com/1".into(),
            "https://a.com/1".into(), // dup
            "ftp://a.com/x".into(),   // non-http
            "not a url".into(),       // invalid
            "https://a.com/2".into(),
            "https://a.com/3".into(),
        ];
        let got = expand_targets(&urls, 2);
        assert_eq!(
            got,
            vec!["https://a.com/1".to_string(), "https://a.com/2".to_string()]
        );
    }

    #[test]
    fn expand_empty_when_all_invalid() {
        let urls = vec!["nope".into(), "mailto:x@y.com".into()];
        assert!(expand_targets(&urls, 10).is_empty());
    }

    /// QB-T17: with `QUARRY_EDGE__AI_FORMATS` off, a `schema` request answers 501 before any page
    /// is fetched, and makes no mint and no Model Plane call (the registry refuses that call).
    #[tokio::test]
    async fn structured_extraction_makes_no_model_plane_call_while_its_switch_is_off() {
        use crate::test_support::{claims_for_org, test_state, StubDriver};
        use axum::response::IntoResponse;
        use quarry_runtime::service_tokens::{
            AuthCoreServiceTokenProvider, SharedServiceTokenProvider,
        };
        use wiremock::matchers::any;
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let auth_core = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "token": "header.payload.signature",
                "expiresInSeconds": 300
            })))
            .mount(&auth_core)
            .await;
        let model_plane = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(200))
            .mount(&model_plane)
            .await;
        let provider: SharedServiceTokenProvider = Arc::new(
            AuthCoreServiceTokenProvider::new(auth_core.uri(), "credential-long-enough").unwrap(),
        );
        let state = AppState {
            model_plane_url: Some(model_plane.uri()),
            service_token_provider: Some(provider),
            ..test_state(StubDriver::ok())
        };
        let request: ExtractRequest = serde_json::from_value(serde_json::json!({
            "urls": ["https://example.com/structured"],
            "schema": { "type": "object" }
        }))
        .unwrap();

        let response = extract(
            State(state),
            axum::Extension(claims_for_org("org_extract")),
            axum::Json(request),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
        assert!(auth_core.received_requests().await.unwrap().is_empty());
        assert!(model_plane.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn no_valid_urls_returns_structured_envelope() {
        let state = crate::test_support::test_state(crate::test_support::StubDriver::ok());
        let response = extract(
            State(state),
            Extension(crate::test_support::claims_for_org("org_alpha")),
            Json(ExtractRequest {
                urls: vec!["nope".into()],
                schema: None,
                prompt: None,
                max_urls: None,
                zdr: None,
                privacy: None,
                signals: None,
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = crate::test_support::response_json(response).await;
        // Structured envelope: exactly {error, code} — no rate-limit fields.
        assert_eq!(crate::test_support::json_keys(&body), vec!["code", "error"]);
        assert_eq!(body["code"], "BAD_REQUEST");
    }

    /// A transport-side 429 must short-circuit the whole batch with the
    /// same structured envelope the map/search/scrape paths emit, NOT
    /// degenerate into a per-item "error" string that a caller would
    /// mistake for a one-off failure.
    #[tokio::test]
    async fn rate_limited_short_circuits_to_structured_429() {
        let state = crate::test_support::test_state(crate::test_support::StubDriver::err(
            ErrorCode::RateLimited,
        ));
        let response = extract(
            State(state),
            Extension(crate::test_support::claims_for_org("org_alpha")),
            Json(ExtractRequest {
                urls: vec![
                    "https://a.example/1".into(),
                    "https://a.example/2".into(),
                    "https://a.example/3".into(),
                ],
                schema: None,
                prompt: None,
                max_urls: None,
                zdr: None,
                privacy: None,
                signals: None,
            }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let body = crate::test_support::response_json(response).await;
        // Typed envelope with retry guidance.
        assert_eq!(body["code"], "RATE_LIMITED");
        assert_eq!(body["retry_after_seconds"], 60);
        assert_eq!(body["window"], "1m");
        let keys = crate::test_support::json_keys(&body);
        for required in [
            "code",
            "error",
            "hint",
            "next_actions",
            "retry_after_seconds",
            "window",
        ] {
            assert!(keys.contains(&required), "missing {required} in {keys:?}");
        }
        // The whole batch must NOT be returned as a 200 with per-item errors.
        assert!(body.get("results").is_none());
    }
}
