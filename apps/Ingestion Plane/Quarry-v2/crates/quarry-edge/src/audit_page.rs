//! Bounded public-page evidence for Marketing Core's audit rules.
//! No analysis, durable artifact, browser fallback, provider or ingest calls.

use axum::{
    extract::State,
    response::{IntoResponse, Response},
    Extension, Json,
};
use quarry_core::error::{ErrorCode, QuarryError};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use url::Url;

use crate::{auth::Claims, state::AppState};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditPageRequest {
    pub url: String,
}

const MAX_LINKS: usize = 100;
const MAX_HEADINGS: usize = 200;
const TIMEOUT: Duration = Duration::from_secs(10);
static CAPTURE_CAPACITY: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(8);

#[derive(Serialize)]
struct Metadata {
    title: Option<String>,
    description: Option<String>,
    content_type: Option<String>,
}
#[derive(Serialize)]
struct AuditLink {
    url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
}
#[derive(Serialize)]
struct Heading {
    level: u8,
    text: String,
}
#[derive(Serialize)]
struct Limits {
    body_bytes: usize,
    timeout_ms: u64,
    max_links: usize,
}
#[derive(Serialize)]
struct AuditPage {
    version: &'static str,
    url: String,
    final_url: String,
    status: u16,
    fetched_at: String,
    run_id: String,
    fingerprint: String,
    metadata: Metadata,
    canonical: Option<String>,
    links: Vec<AuditLink>,
    headings: Vec<Heading>,
    seo: crate::audit_seo::SeoEvidence,
    redirects: Vec<crate::map_routes::RedirectHop>,
    partial: bool,
    limits: Limits,
}

fn denied() -> QuarryError {
    QuarryError::new(
        ErrorCode::SecurityBlocked,
        "public audit page blocked by policy",
    )
}
fn too_large() -> QuarryError {
    QuarryError::new(ErrorCode::BadRequest, "audit evidence exceeds field limits")
}

pub async fn capture(
    State(state): State<AppState>,
    Extension(_claims): Extension<Claims>,
    Json(request): Json<AuditPageRequest>,
) -> Response {
    capture_with_capacity(state, request, &CAPTURE_CAPACITY).await
}

async fn capture_with_capacity(
    state: AppState,
    request: AuditPageRequest,
    capacity: &tokio::sync::Semaphore,
) -> Response {
    // Process-local transport protection, not a tenant billing/quota policy.
    // Never accumulate an unbounded queue of authenticated capture requests.
    let Ok(_permit) = capacity.try_acquire() else {
        return (
            axum::http::StatusCode::TOO_MANY_REQUESTS,
            Json(crate::api_error::ApiError::rate_limited(
                "audit capture capacity is busy",
                1,
            )),
        )
            .into_response();
    };
    capture_with_budget(state, request, TIMEOUT).await
}

async fn capture_with_budget(
    state: AppState,
    request: AuditPageRequest,
    budget: Duration,
) -> Response {
    match tokio::time::timeout(budget, capture_inner(&state, request)).await {
        Ok(Ok(document)) => Json(document).into_response(),
        Ok(Err(error)) => crate::map_routes::map_failure(error),
        Err(_) => crate::map_routes::map_failure(QuarryError::new(
            ErrorCode::Timeout,
            "audit capture deadline",
        )),
    }
}

async fn capture_inner(
    state: &AppState,
    request: AuditPageRequest,
) -> Result<AuditPage, QuarryError> {
    if request.url.len() > 4096 {
        return Err(too_large());
    }
    let url = Url::parse(request.url.trim()).map_err(|_| denied())?;
    if !crate::map_routes::public_discovery_url(&url)
        || quarry_security::heur::check(&url).decision == quarry_security::Decision::Block
    {
        return Err(denied());
    }
    let robots_url = url.join("/robots.txt").map_err(|_| denied())?;
    let robots = crate::map_routes::fetch_document(state, &robots_url)
        .await?
        .ok_or_else(denied)?;
    crate::map_routes::check_robots(&robots, url.path())?;
    let mut redirects = Vec::new();
    let response = crate::map_routes::fetch_document_traced(state, &url, &mut redirects)
        .await?
        .ok_or_else(|| QuarryError::new(ErrorCode::DriverFailed, "audit source unavailable"))?;
    evidence_with_context(request.url, response, robots.status, redirects)
}

fn evidence_with_context(
    requested: String,
    response: quarry_runtime::fetch::FetchResponse,
    robots_status: u16,
    redirects: Vec<crate::map_routes::RedirectHop>,
) -> Result<AuditPage, QuarryError> {
    let content_type = response
        .headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
        .map(|(_, value)| value.clone());
    if content_type.as_ref().is_some_and(|value| value.len() > 256) {
        return Err(too_large());
    }
    let html_content = content_type
        .as_deref()
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "text/html" | "application/xhtml+xml"
            )
        });
    if !html_content && (200..300).contains(&response.status) {
        return Err(QuarryError::new(
            ErrorCode::Unsupported,
            "audit capture requires an HTML document",
        ));
    }
    let html = if html_content {
        String::from_utf8_lossy(&response.body).into_owned()
    } else {
        String::new()
    };
    let doc = Html::parse_document(&html);
    let title = selected_text(&doc, "title");
    let description = doc
        .select(&Selector::parse("meta[name]").unwrap())
        .find(|element| {
            element
                .value()
                .attr("name")
                .is_some_and(|name| name.eq_ignore_ascii_case("description"))
        })
        .and_then(|element| element.value().attr("content"))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    if title
        .as_ref()
        .is_some_and(|value| value.chars().count() > 512)
        || description
            .as_ref()
            .is_some_and(|value| value.chars().count() > 2048)
    {
        return Err(too_large());
    }
    let canonical =
        quarry_transform::attributes::normalize_canonical_url(&html, response.final_url.as_str())
            .filter(|raw| {
                raw.len() <= 4096
                    && Url::parse(raw)
                        .is_ok_and(|url| crate::map_routes::public_discovery_url(&url))
            });
    let all_links = quarry_transform::links::extract(&html, &response.final_url);
    let links = all_links
        .iter()
        .filter(|link| {
            link.href.len() <= 4096
                && Url::parse(&link.href).is_ok_and(|url| {
                    crate::map_routes::public_discovery_url(&url)
                        && quarry_security::heur::check(&url).decision
                            != quarry_security::Decision::Block
                })
        })
        .take(MAX_LINKS)
        .map(|link| AuditLink {
            url: link.href.clone(),
            text: link
                .text
                .as_ref()
                .map(|text| text.chars().take(512).collect()),
        })
        .collect();
    let headings_selector = Selector::parse("h1,h2,h3,h4,h5,h6").unwrap();
    let all_headings = doc.select(&headings_selector).collect::<Vec<_>>();
    let headings = all_headings
        .iter()
        .take(MAX_HEADINGS)
        .map(|element| Heading {
            level: element.value().name()[1..].parse().unwrap_or(0),
            text: element
                .text()
                .collect::<String>()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(512)
                .collect(),
        })
        .collect();
    let run_id: quarry_core::ids::kinds::RunKind = quarry_core::ids::Id::new();
    let seo = crate::audit_seo::extract(&doc, &html, &response.final_url, &response.headers, robots_status);
    Ok(AuditPage {
        version: "bounded_page_v1",
        url: requested,
        final_url: response.final_url.to_string(),
        status: response.status,
        fetched_at: chrono::Utc::now().to_rfc3339(),
        run_id: run_id.to_string(),
        fingerprint: format!("blake3:{}", blake3::hash(&response.body).to_hex()),
        metadata: Metadata {
            title,
            description,
            content_type,
        },
        canonical,
        links,
        headings,
        seo,
        redirects,
        partial: all_links.len() > MAX_LINKS || all_headings.len() > MAX_HEADINGS,
        limits: Limits {
            body_bytes: 1048576,
            timeout_ms: 10000,
            max_links: MAX_LINKS,
        },
    })
}

fn selected_text(doc: &Html, selector: &str) -> Option<String> {
    doc.select(&Selector::parse(selector).unwrap())
        .next()
        .map(|element| element.text().collect::<String>().trim().to_owned())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
#[path = "audit_page_tests.rs"]
mod tests;
