//! `/v1/map` — fast whole-site URL discovery (cluster #/v1/map, OSS-parity P0 1B).
//!
//! Recombines existing primitives — `transform::sitemap`, `transform::robots`,
//! `transform::links` — plus the runtime `LexicalRanker` to return every URL of
//! a site, optionally ranked by relevance to a `search` term.
//!
//! Read-only: no durable writes, so ZDR=on is always fine. Org-scoped only for
//! usage metering + event emission; the discovery itself touches no tenant data.

use std::collections::BTreeSet;

use axum::{extract::State, http::StatusCode, response::IntoResponse, Extension, Json};
use serde::{Deserialize, Serialize};
use url::Url;

use quarry_runtime::crawl_ranker::{CrawlRanker, LexicalRanker};

use crate::state::AppState;

const DEFAULT_LIMIT: usize = 100;
const MAX_LIMIT: usize = 1000;
const MAP_UA: &str = "QuarryBot";

#[derive(Debug, Deserialize)]
pub struct MapRequest {
    /// Site URL to map (any page on the origin works; the origin is derived).
    pub url: String,
    /// Optional relevance term; when set, results are ranked by it.
    #[serde(default)]
    pub search: Option<String>,
    #[serde(default)]
    pub limit: Option<u32>,
    /// Include subdomains of the seed host in scope (default: same host only).
    #[serde(default)]
    pub include_subdomains: bool,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct MapLink {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f32>,
}

#[derive(Debug, Serialize)]
pub struct MapResponse {
    pub url: String,
    pub links: Vec<MapLink>,
    pub count: usize,
    /// True when results were relevance-ranked against `search`.
    pub ranked: bool,
}

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub error: String,
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

// ── pure helpers (unit-tested; no network) ──────────────────────────────────

/// True when `host` is in scope relative to `seed_host`. Same host always
/// passes; subdomains pass only when `include_subdomains` is set.
pub(crate) fn host_in_scope(seed_host: &str, host: &str, include_subdomains: bool) -> bool {
    if host == seed_host {
        return true;
    }
    include_subdomains && host.ends_with(&format!(".{seed_host}"))
}

/// Merge sitemap + page-link URLs, drop anything out of host scope, dedup
/// (stable by first-seen via sorted set), and return the candidate URL list.
pub(crate) fn merge_and_scope(
    seed_host: &str,
    include_subdomains: bool,
    candidates: impl IntoIterator<Item = String>,
) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for raw in candidates {
        let Ok(u) = Url::parse(&raw) else { continue };
        if !matches!(u.scheme(), "http" | "https") {
            continue;
        }
        let Some(host) = u.host_str() else { continue };
        if !host_in_scope(seed_host, host, include_subdomains) {
            continue;
        }
        let norm = u.as_str().trim_end_matches('#').to_string();
        if seen.insert(norm.clone()) {
            out.push(norm);
        }
    }
    out
}

fn clamp_limit(limit: Option<u32>) -> usize {
    limit
        .map(|l| l as usize)
        .unwrap_or(DEFAULT_LIMIT)
        .clamp(1, MAX_LIMIT)
}

async fn fetch_text(state: &AppState, url: &Url) -> Option<String> {
    match state.driver.fetch(url).await {
        Ok(resp) if (200..300).contains(&resp.status) => {
            Some(String::from_utf8_lossy(&resp.body).into_owned())
        }
        _ => None,
    }
}

// ── handler ─────────────────────────────────────────────────────────────────

pub async fn map(
    State(state): State<AppState>,
    Extension(claims): Extension<crate::auth::Claims>,
    Json(req): Json<MapRequest>,
) -> impl IntoResponse {
    let Ok(base) = Url::parse(req.url.trim()) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: "url must be a valid absolute http(s) URL".into(),
                code: "BAD_REQUEST".into(),
                hint: None,
            }),
        )
            .into_response();
    };
    let Some(seed_host) = base.host_str().map(str::to_owned) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: "url has no host".into(),
                code: "BAD_REQUEST".into(),
                hint: None,
            }),
        )
            .into_response();
    };

    // SSRF guard: scheme/userinfo/host heuristics before any fetch.
    if matches!(
        quarry_security::heur::check(&base).decision,
        quarry_security::Decision::Block
    ) {
        return (
            StatusCode::FORBIDDEN,
            Json(ErrorBody {
                error: "url blocked by security policy".into(),
                code: "FORBIDDEN".into(),
                hint: None,
            }),
        )
            .into_response();
    }

    let origin = format!(
        "{}://{}",
        base.scheme(),
        base.host_str().unwrap_or_default()
    );

    // Best-effort robots for allow-filtering (never fatal).
    let robots = match Url::parse(&format!("{origin}/robots.txt")) {
        Ok(u) => fetch_text(&state, &u)
            .await
            .map(|t| quarry_transform::robots::RobotsTxt::parse(&t)),
        Err(_) => None,
    };

    // Sitemap URLs (best-effort, one level of nested index).
    let mut candidates: Vec<String> = Vec::new();
    if let Ok(sm_url) = Url::parse(&format!("{origin}/sitemap.xml")) {
        if let Some(xml) = fetch_text(&state, &sm_url).await {
            let sm = quarry_transform::sitemap::parse(&xml);
            candidates.extend(sm.urls.into_iter().map(|e| e.loc));
            for nested in sm.nested.into_iter().take(5) {
                if let Ok(nu) = Url::parse(&nested) {
                    if let Some(nx) = fetch_text(&state, &nu).await {
                        candidates.extend(
                            quarry_transform::sitemap::parse(&nx)
                                .urls
                                .into_iter()
                                .map(|e| e.loc),
                        );
                    }
                }
            }
        }
    }

    // Page links + titles.
    let mut titles: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    if let Some(html) = fetch_text(&state, &base).await {
        for link in quarry_transform::links::extract(&html, &base) {
            if let Some(t) = &link.text {
                titles.entry(link.href.clone()).or_insert_with(|| t.clone());
            }
            candidates.push(link.href);
        }
    }

    if candidates.is_empty() {
        return (
            StatusCode::BAD_GATEWAY,
            Json(ErrorBody {
                error: "could not fetch sitemap or page to discover URLs".into(),
                code: "UPSTREAM_EMPTY".into(),
                hint: Some("the origin returned no sitemap.xml and the page had no links".into()),
            }),
        )
            .into_response();
    }

    // Scope + dedup, then robots allow-filter.
    let mut urls = merge_and_scope(&seed_host, req.include_subdomains, candidates);
    if let Some(rt) = &robots {
        urls.retain(|u| {
            Url::parse(u)
                .ok()
                .map(|p| rt.is_allowed(MAP_UA, p.path()))
                .unwrap_or(true)
        });
    }

    let limit = clamp_limit(req.limit);

    // Optional relevance ranking.
    let (links, ranked) = match &req.search {
        Some(term) if !term.trim().is_empty() => {
            let ranker = LexicalRanker;
            match ranker.rank(term, &urls).await {
                Ok(mut ranked_urls) => {
                    ranked_urls.sort_by(|a, b| {
                        b.score
                            .partial_cmp(&a.score)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    });
                    let links = ranked_urls
                        .into_iter()
                        .take(limit)
                        .map(|r| MapLink {
                            title: titles.get(&r.url).cloned(),
                            url: r.url,
                            score: Some(r.score),
                        })
                        .collect::<Vec<_>>();
                    (links, true)
                }
                Err(_) => (to_links(urls, &titles, limit), false),
            }
        }
        _ => (to_links(urls, &titles, limit), false),
    };

    let count = links.len();

    // Usage metering — one unit per map call.
    let run_id: quarry_core::ids::kinds::RunKind = quarry_core::ids::Id::new();
    state
        .usage
        .meter(quarry_runtime::UsageEvent::new(
            run_id.to_string(),
            claims.org_id.clone(),
            quarry_runtime::usage_metrics::SEARCH_QUERY,
            1.0,
            serde_json::json!({
                "op": "map",
                "user_id": claims.user_id,
                "host": seed_host,
                "result_count": count,
                "ranked": ranked,
            }),
        ))
        .await;

    (
        StatusCode::OK,
        Json(MapResponse {
            url: req.url,
            links,
            count,
            ranked,
        }),
    )
        .into_response()
}

fn to_links(
    urls: Vec<String>,
    titles: &std::collections::HashMap<String, String>,
    limit: usize,
) -> Vec<MapLink> {
    urls.into_iter()
        .take(limit)
        .map(|u| MapLink {
            title: titles.get(&u).cloned(),
            url: u,
            score: None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_host_in_scope() {
        assert!(host_in_scope("a.com", "a.com", false));
    }

    #[test]
    fn external_host_rejected() {
        assert!(!host_in_scope("a.com", "b.com", false));
        assert!(!host_in_scope("a.com", "evil-a.com", true));
    }

    #[test]
    fn subdomain_only_with_flag() {
        assert!(!host_in_scope("a.com", "docs.a.com", false));
        assert!(host_in_scope("a.com", "docs.a.com", true));
    }

    #[test]
    fn merge_dedups_and_scopes() {
        let got = merge_and_scope(
            "a.com",
            false,
            vec![
                "https://a.com/x".into(),
                "https://a.com/x".into(),      // dup
                "https://b.com/y".into(),      // external
                "https://docs.a.com/z".into(), // subdomain, flag off
                "ftp://a.com/bad".into(),      // non-http
            ],
        );
        assert_eq!(got, vec!["https://a.com/x".to_string()]);
    }

    #[test]
    fn merge_includes_subdomains_when_flagged() {
        let got = merge_and_scope(
            "a.com",
            true,
            vec!["https://a.com/x".into(), "https://docs.a.com/z".into()],
        );
        assert_eq!(got.len(), 2);
    }

    #[test]
    fn limit_clamps() {
        assert_eq!(clamp_limit(None), DEFAULT_LIMIT);
        assert_eq!(clamp_limit(Some(0)), 1);
        assert_eq!(clamp_limit(Some(99999)), MAX_LIMIT);
        assert_eq!(clamp_limit(Some(25)), 25);
    }
}
