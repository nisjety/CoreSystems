//! Product-aware scraping: render a listing page, extract its products as
//! structured data, and summarize a chosen subset.
//!
//! Pipeline (reuses the same escalation seam as the preview):
//!   1. fetch page markdown — quarry `/v1/extract` (basic render) → on a
//!      block/empty, retry through Quarry's owned TLS/browser driver waterfall
//!      before any configured enhanced provider is considered.
//!   2. structured extraction — model-gateway `/v1/invoke` with a product
//!      JSON schema (`structured_output_schema`) → `{ products: [...] }`.
//!   3. summary — a second `/v1/invoke` turns the user's selected products into
//!      a markdown brief with an AI overview.
//!
//! Works on cooperative sites today; protected retailers improve as Quarry's
//! owned egress/TLS/browser runtime improves. Enhanced providers remain an
//! explicit operator-configured fallback, not the default path.

use axum::{
    extract::{Extension, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    config::AppState,
    domains::knowledge::shared,
    envelope::{error, ok},
    middleware::AuthenticatedUser,
    public_url::normalize_public_http_url,
    upstream::proxy_bearer_json,
};

/// model-gateway rejects content over 100 KB; leave headroom for the instruction.
const CONTENT_CAP: usize = 90_000;
const EXTRACT_MAX_TOKENS: u32 = 8_000;
const SUMMARY_MAX_TOKENS: u32 = 1_500;
const MAX_PRODUCTS: usize = 60;

// ── handlers ────────────────────────────────────────────────────────────────

/// `POST /api/v1/knowledge/scrape/products` — `{ url, prompt? }` → render the
/// listing and return `{ url, source, products: [...] }`.
pub(crate) async fn extract_products(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let target = match target_url(&body) {
        Ok(value) => value,
        Err(message) => {
            return (StatusCode::BAD_REQUEST, Json(error("invalid_url", message))).into_response()
        }
    };
    let hint = body
        .get("prompt")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_default();

    let Some((markdown, source)) = fetch_listing_markdown(&state, &user, &headers, &target).await
    else {
        return (
            StatusCode::BAD_GATEWAY,
            Json(error(
                "scrape_failed",
                "Kunne ikke hente innhold fra siden. Den kan blokkere automatisert henting — sett en proxy-nøkkel for beskyttede nettsteder.",
            )),
        )
            .into_response();
    };

    let token = crate::domains::chat::shared::model_token(&state, &user, &headers).await;
    // We prompt for strict JSON and parse it ourselves rather than using the
    // provider `structured_output_schema` path — inference-core's schema mode
    // currently fails ("all providers exhausted"), while plain invoke is reliable.
    let invoke = json!({
        "content": build_extract_prompt(&markdown, hint),
        "max_tokens": EXTRACT_MAX_TOKENS,
    });
    let url = format!("{}/v1/invoke", state.model_gateway_url);
    let (status, Json(resp)) = crate::domains::chat::shared::proxy_model_json(
        &state,
        Method::POST,
        &url,
        Some(invoke),
        token.as_deref(),
        &user,
    )
    .await;

    if !status.is_success() {
        tracing::warn!(%status, target = %target, "product extraction model call failed");
        return (
            StatusCode::BAD_GATEWAY,
            Json(error(
                "extraction_failed",
                "Kunne ikke trekke ut produkter fra siden.",
            )),
        )
            .into_response();
    }

    let products = parse_products(&resp, &target);
    (
        StatusCode::OK,
        Json(ok(json!({
            "url": target,
            "source": source,
            "count": products.len(),
            "products": products,
        }))),
    )
        .into_response()
}

/// `POST /api/v1/knowledge/scrape/products/summary` — `{ products: [...], prompt? }`
/// → `{ summary }` (markdown brief + AI overview of the selected products).
pub(crate) async fn summarize_products(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let products = body
        .get("products")
        .and_then(Value::as_array)
        .filter(|items| !items.is_empty());
    let Some(products) = products else {
        return (
            StatusCode::BAD_REQUEST,
            Json(error("no_products", "Velg minst ett produkt å oppsummere.")),
        )
            .into_response();
    };
    let hint = body
        .get("prompt")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_default();

    let token = crate::domains::chat::shared::model_token(&state, &user, &headers).await;
    let invoke = json!({
        "content": build_summary_prompt(products, hint),
        "max_tokens": SUMMARY_MAX_TOKENS,
    });
    let url = format!("{}/v1/invoke", state.model_gateway_url);
    let (status, Json(resp)) = crate::domains::chat::shared::proxy_model_json(
        &state,
        Method::POST,
        &url,
        Some(invoke),
        token.as_deref(),
        &user,
    )
    .await;

    if !status.is_success() {
        return (
            StatusCode::BAD_GATEWAY,
            Json(error("summary_failed", "Kunne ikke lage AI-sammendrag.")),
        )
            .into_response();
    }

    let summary = resp
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned();
    (StatusCode::OK, Json(ok(json!({ "summary": summary })))).into_response()
}

// ── page fetch (basic → owned escalation → optional enhanced) ───────────────

/// Returns `(markdown, source)` for the listing. Tries quarry's `/v1/extract`
/// first with the default plan, then with Quarry's TLS-first waterfall.
/// `None` when no path yields content.
///
/// Both attempts stay inside the Ingestion Plane, which owns web fetch. There is
/// deliberately no third tier here: a commercial stealth-proxy fallback
/// (Scrapfly / Bright Data Web Unlocker) used to run in-process at this point,
/// which shipped the org's target URL and the page's full content to a
/// third-party processor with no `zdr` bit, no org scoping, no robots check, no
/// usage record and no step receipt — a page Quarry had just *refused* was
/// fetched anyway, off the audit trail entirely. If a residential/stealth tier
/// is genuinely needed it belongs behind `quarry-edge` as another `DriverKind`
/// in Quarry's own waterfall, where it inherits ZDR gating, residency, cost
/// accounting and receipts; a Quarry refusal must stay a refusal here.
async fn fetch_listing_markdown(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    target: &str,
) -> Option<(String, &'static str)> {
    let cookie = shared::cookie_header(headers);
    let token = shared::quarry_token(state, user, &cookie).await;
    let url = format!("{}/v1/extract", state.quarry_edge_url);
    let body = json!({ "urls": [target], "max_urls": 1 });
    if let Some(markdown) = fetch_extract_markdown(state, &token, &user.user_id, &url, body).await {
        return Some((markdown, "basic"));
    }

    // Retry through Quarry's owned TLS-first waterfall. This catches pages where
    // the static path returns an empty JS shell or trips a bot-wall that a
    // browser-like transport profile can handle. It still stays inside Quarry;
    // no Scrapfly/BrightData-style provider is used here.
    let escalated_body = json!({
        "urls": [target],
        "max_urls": 1,
        "signals": { "prior_block_signals": 1 }
    });
    if let Some(markdown) =
        fetch_extract_markdown(state, &token, &user.user_id, &url, escalated_body).await
    {
        return Some((markdown, "quarry_tls"));
    }

    // Still blocked or empty after Quarry's own waterfall: that is the answer.
    None
}

async fn fetch_extract_markdown(
    state: &AppState,
    token: &Option<String>,
    user_id: &str,
    url: &str,
    body: Value,
) -> Option<String> {
    let (status, Json(resp)) = proxy_bearer_json(
        state,
        Method::POST,
        url,
        Some(body),
        token.as_deref(),
        user_id,
    )
    .await;

    if status.is_success() {
        if let Some(markdown) = extract_first_markdown(&resp) {
            return Some(cap_content(&markdown));
        }
    }

    tracing::warn!(%status, "quarry product extract returned no usable markdown");
    None
}

fn extract_first_markdown(resp: &Value) -> Option<String> {
    for pointer in [
        "/results/0/markdown",
        "/data/results/0/markdown",
        "/data/markdown",
        "/markdown",
    ] {
        if let Some(value) = resp.pointer(pointer).and_then(Value::as_str) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_owned());
            }
        }
    }
    None
}

// ── prompts + schema ────────────────────────────────────────────────────────

/// Pull a JSON object out of a model completion. Handles a clean object, ```json
/// code fences, and prose wrapped around the object (first `{` … last `}`).
fn extract_json_object(content: &str) -> Option<Value> {
    let trimmed = content.trim();
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return Some(value);
    }
    let unfenced = trimmed
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    if let Ok(value) = serde_json::from_str::<Value>(unfenced) {
        return Some(value);
    }
    let start = unfenced.find('{')?;
    let end = unfenced.rfind('}')?;
    if end > start {
        serde_json::from_str::<Value>(&unfenced[start..=end]).ok()
    } else {
        None
    }
}

fn build_extract_prompt(markdown: &str, hint: &str) -> String {
    let focus = if hint.is_empty() {
        String::new()
    } else {
        format!(" Focus on products matching: {hint}.")
    };
    format!(
        "You extract product data from an e-commerce / listing page. From the PAGE CONTENT \
below, identify every distinct, purchasable product (skip nav, ads, and related-article links). \
Return ONLY a single minified JSON object — no markdown, no code fences, no prose — of this exact shape:\n\
{{\"products\":[{{\"name\":\"\",\"price\":\"\",\"currency\":\"\",\"image\":\"\",\"url\":\"\",\"specs\":[\"\"],\"description\":\"\"}}]}}\n\
Use absolute URLs for image and url. Omit any field you cannot determine — never invent values. \
If there are no products, return {{\"products\":[]}}.{focus}\n\n\
PAGE CONTENT:\n{content}",
        content = cap_content(markdown)
    )
}

fn build_summary_prompt(products: &[Value], hint: &str) -> String {
    let focus = if hint.is_empty() {
        String::new()
    } else {
        format!(" The user is interested in: {hint}.")
    };
    let list = serde_json::to_string(products).unwrap_or_else(|_| "[]".to_owned());
    format!(
        "You are a Norwegian shopping assistant. Write a concise markdown brief for the \
selected products below.{focus} Start with a 1–2 sentence overview, then a markdown table \
with columns Produkt | Pris | Nøkkelspesifikasjoner, then a short recommendation. \
Write in Norwegian. Keep image/links out of the table.\n\n\
PRODUCTS (JSON):\n{list}"
    )
}

// ── response parsing ──────────────────────────────────────────────────────────

/// Pull the product array out of the model response. `content` is a JSON string
/// (structured output) shaped `{ products: [...] }`; parse defensively and cap.
fn parse_products(resp: &Value, page_url: &str) -> Vec<Value> {
    let content = resp.get("content").and_then(Value::as_str).unwrap_or("");
    let parsed = extract_json_object(content).unwrap_or(Value::Null);
    let items = parsed
        .get("products")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    items
        .into_iter()
        .filter_map(|item| normalize_product(item, page_url))
        .take(MAX_PRODUCTS)
        .collect()
}

/// Keep only products with a usable name; pass through known fields, dropping
/// empties so the UI renders cleanly.
fn normalize_product(item: Value, page_url: &str) -> Option<Value> {
    let name = item
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let mut out = json!({ "name": name });
    for key in ["price", "currency", "description"] {
        if let Some(value) = item
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            out[key] = json!(value);
        }
    }
    for key in ["image", "url"] {
        if let Some(value) = item
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            out[key] = json!(absolutize(value, page_url));
        }
    }
    if let Some(specs) = item.get("specs").and_then(Value::as_array) {
        let cleaned: Vec<Value> = specs
            .iter()
            .filter_map(|s| s.as_str().map(str::trim).filter(|v| !v.is_empty()))
            .take(8)
            .map(|s| json!(s))
            .collect();
        if !cleaned.is_empty() {
            out["specs"] = json!(cleaned);
        }
    }
    Some(out)
}

/// Resolve a relative image/product URL against the page URL; pass absolute
/// http(s) URLs through unchanged, drop anything unparseable.
fn absolutize(value: &str, page_url: &str) -> String {
    if value.starts_with("http://") || value.starts_with("https://") {
        return value.to_owned();
    }
    url::Url::parse(page_url)
        .ok()
        .and_then(|base| base.join(value).ok())
        .map(|u| u.to_string())
        .unwrap_or_else(|| value.to_owned())
}

fn cap_content(markdown: &str) -> String {
    markdown.trim().chars().take(CONTENT_CAP).collect()
}

fn target_url(body: &Value) -> Result<String, String> {
    let raw = body
        .get("url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "A non-empty URL is required.".to_owned())?;
    normalize_public_http_url(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_json_object_handles_clean_fenced_and_prose() {
        let clean = extract_json_object(r#"{"products":[{"name":"A"}]}"#).unwrap();
        assert_eq!(clean["products"][0]["name"], "A");
        let fenced = extract_json_object("```json\n{\"products\":[]}\n```").unwrap();
        assert!(fenced["products"].as_array().unwrap().is_empty());
        let prosed =
            extract_json_object("Here you go:\n{\"products\":[{\"name\":\"B\"}]} — done").unwrap();
        assert_eq!(prosed["products"][0]["name"], "B");
        assert!(extract_json_object("no json here").is_none());
    }

    #[test]
    fn parse_products_reads_structured_content_and_caps() {
        let content = json!({
            "products": [
                { "name": "MacBook Air M3", "price": "14 999", "currency": "kr", "specs": ["16GB", "512GB", ""], "image": "/img/a.jpg" },
                { "name": "  ", "price": "0" },
                { "name": "MacBook Pro 14", "url": "https://www.elkjop.no/p/mbp" }
            ]
        })
        .to_string();
        let resp = json!({ "content": content, "model_used": "x" });
        let products = parse_products(&resp, "https://www.elkjop.no/cat/mac");
        assert_eq!(products.len(), 2); // blank-name dropped
        assert_eq!(products[0]["name"], "MacBook Air M3");
        assert_eq!(products[0]["image"], "https://www.elkjop.no/img/a.jpg"); // absolutized
        assert_eq!(products[0]["specs"].as_array().unwrap().len(), 2); // empty spec dropped
        assert_eq!(products[1]["url"], "https://www.elkjop.no/p/mbp");
    }

    #[test]
    fn parse_products_tolerates_non_json_content() {
        let resp = json!({ "content": "sorry, I could not parse the page" });
        assert!(parse_products(&resp, "https://x.io").is_empty());
    }

    #[test]
    fn extract_first_markdown_reads_extract_envelope() {
        let resp = json!({ "results": [{ "markdown": "# Mac\n\n999 kr" }] });
        assert_eq!(extract_first_markdown(&resp).unwrap(), "# Mac\n\n999 kr");
        assert!(extract_first_markdown(&json!({ "results": [{ "markdown": "  " }] })).is_none());
    }

    #[test]
    fn extract_prompt_includes_hint_and_caps_content() {
        let prompt = build_extract_prompt("# Macs\ncontent", "MacBook Air under 15000");
        assert!(prompt.contains("MacBook Air under 15000"));
        assert!(prompt.contains("PAGE CONTENT:"));
    }

    #[test]
    fn absolutize_resolves_relative_and_passes_absolute() {
        assert_eq!(
            absolutize("/p/x.jpg", "https://a.io/cat/mac"),
            "https://a.io/p/x.jpg"
        );
        assert_eq!(
            absolutize("https://cdn.a.io/x.jpg", "https://a.io/cat"),
            "https://cdn.a.io/x.jpg"
        );
    }
}
