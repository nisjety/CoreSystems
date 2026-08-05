//! Optional enhanced-provider fallback for sites that still block Quarry's
//! owned fetch/TLS/browser paths.
//!
//! GDPR/default deployments should leave this unconfigured. When an operator
//! explicitly configures a provider, callers still try Quarry-owned paths first
//! and only use this module as a last resort.

use std::time::Duration;

use serde_json::{json, Value};
use url::Url;

use crate::config::AppState;

/// Stealth renders go through residential proxies + challenge solving, which is
/// far slower than a datacenter fetch — allow generous headroom (the gateway's
/// shared client caps at 25s, so this runs on the no-overall-timeout streaming
/// client wrapped in an explicit budget).
const ENHANCED_TIMEOUT: Duration = Duration::from_secs(75);
const MARKDOWN_CAP: usize = 80_000;

pub(crate) struct EnhancedPage {
    pub(crate) markdown: String,
    pub(crate) status: u16,
}

/// True when an enhanced provider is fully configured (name + API key present).
pub(crate) fn enhanced_enabled(state: &AppState) -> bool {
    !state.enhanced_scrape_provider.is_empty() && !state.enhanced_scrape_api_key.is_empty()
}

/// Fetch `target` through the configured stealth/proxy provider, returning clean
/// markdown. Returns `None` when no provider is configured, the call fails, or it
/// exceeds the budget — callers then surface a clear error instead of hanging.
pub(crate) async fn enhanced_fetch(state: &AppState, target: &str) -> Option<EnhancedPage> {
    if !enhanced_enabled(state) {
        return None;
    }
    let provider = state.enhanced_scrape_provider.as_str();
    let fetched = tokio::time::timeout(ENHANCED_TIMEOUT, async {
        match provider {
            "scrapfly" => fetch_scrapfly(state, target).await,
            "brightdata" | "bright_data" => fetch_brightdata(state, target).await,
            other => {
                tracing::warn!(provider = other, "unknown SCRAPE_ENHANCED_PROVIDER");
                None
            }
        }
    })
    .await;

    match fetched {
        Ok(page) => page,
        Err(_) => {
            tracing::warn!(target, provider, "enhanced scrape timed out");
            None
        }
    }
}

// ── Scrapfly (free/default tier) ────────────────────────────────────────────

/// `GET /scrape?asp=true&render_js=true&format=markdown` — ASP is Scrapfly's
/// anti-scraping-protection (residential rotation + challenge solving); markdown
/// format returns clean text directly, so no HTML conversion is needed.
fn scrapfly_url(key: &str, target: &str, country: &str) -> Option<Url> {
    let mut url = Url::parse("https://api.scrapfly.io/scrape").ok()?;
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("key", key);
        q.append_pair("url", target);
        q.append_pair("asp", "true");
        q.append_pair("render_js", "true");
        q.append_pair("format", "markdown");
        if !country.is_empty() {
            q.append_pair("country", country);
        }
    }
    Some(url)
}

/// Scrapfly wraps its payload under `result`: `{ result: { content, status_code } }`.
/// Falls back to top-level keys defensively.
fn extract_scrapfly_content(body: &Value) -> Option<(String, u16)> {
    let result = body.get("result").unwrap_or(body);
    let content = result
        .get("content")
        .or_else(|| body.get("content"))
        .or_else(|| body.get("body"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .filter(|c| !c.trim().is_empty())?;
    let status = result
        .get("status_code")
        .or_else(|| body.get("status_code"))
        .and_then(Value::as_u64)
        .map(|s| s as u16)
        .unwrap_or(200);
    Some((content, status))
}

async fn fetch_scrapfly(state: &AppState, target: &str) -> Option<EnhancedPage> {
    let url = scrapfly_url(
        &state.enhanced_scrape_api_key,
        target,
        &state.enhanced_scrape_country,
    )?;
    let resp = state.streaming_client.get(url).send().await.ok()?;
    let http_status = resp.status();
    let body: Value = resp.json().await.ok()?;
    let Some((content, upstream_status)) = extract_scrapfly_content(&body) else {
        tracing::warn!(target, %http_status, "scrapfly returned no usable content");
        return None;
    };
    Some(EnhancedPage {
        markdown: truncate_markdown(&content),
        status: upstream_status,
    })
}

// ── Bright Data Web Unlocker (best/premium tier) ────────────────────────────

/// Web Unlocker API: `POST https://api.brightdata.com/request` with a configured
/// `zone` returns the unlocked page (residential egress + full anti-bot). It
/// returns raw HTML, so we reduce it to readable text for the preview.
async fn fetch_brightdata(state: &AppState, target: &str) -> Option<EnhancedPage> {
    if state.enhanced_scrape_zone.is_empty() {
        tracing::warn!("brightdata provider selected but SCRAPE_ENHANCED_ZONE is unset");
        return None;
    }
    let body = json!({
        "zone": state.enhanced_scrape_zone,
        "url": target,
        "format": "raw",
    });
    let resp = state
        .streaming_client
        .post("https://api.brightdata.com/request")
        .bearer_auth(&state.enhanced_scrape_api_key)
        .json(&body)
        .send()
        .await
        .ok()?;
    let status = resp.status().as_u16();
    let html = resp.text().await.ok()?;
    let markdown = html_to_text(&html);
    if markdown.trim().is_empty() {
        return None;
    }
    Some(EnhancedPage {
        markdown: truncate_markdown(&markdown),
        status,
    })
}

// ── shared helpers ──────────────────────────────────────────────────────────

/// Build the gateway preview envelope from an enhanced fetch. `source: "enhanced"`
/// flags that the content came via the stealth proxy tier.
pub(crate) fn build_enhanced_preview(target: &str, page: &EnhancedPage) -> Value {
    json!({
        "url": target,
        "title": preview_title(&page.markdown, target),
        "description": "",
        "markdown": page.markdown,
        "source": "enhanced",
        "quarry": { "status": page.status },
    })
}

fn preview_title(markdown: &str, target: &str) -> String {
    markdown
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.trim_start_matches('#').trim())
        .filter(|line| !line.is_empty())
        .map(|line| line.chars().take(120).collect::<String>())
        .unwrap_or_else(|| host_label(target))
}

fn host_label(target: &str) -> String {
    Url::parse(target)
        .ok()
        .and_then(|u| {
            u.host_str()
                .map(|h| h.trim_start_matches("www.").to_owned())
        })
        .unwrap_or_else(|| target.to_owned())
}

fn truncate_markdown(value: &str) -> String {
    value.trim().chars().take(MARKDOWN_CAP).collect()
}

/// Minimal, dependency-free HTML → readable text for previews: drop script/style
/// blocks, turn block-level tags into line breaks, strip remaining tags, decode
/// the common entities, and collapse whitespace. Good enough to preview; the
/// product flow does structured extraction separately. Byte indexing is
/// boundary-safe because every cut sits on an ASCII `<`/`>` delimiter.
fn html_to_text(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let hb = html.as_bytes();
    let n = hb.len();
    let mut out = String::with_capacity(n / 2);
    let mut i = 0;
    while i < n {
        if hb[i] == b'<' {
            if lower[i..].starts_with("<script") || lower[i..].starts_with("<style") {
                let close = if lower[i..].starts_with("<script") {
                    "</script>"
                } else {
                    "</style>"
                };
                match lower[i..].find(close) {
                    Some(rel) => i += rel + close.len(),
                    None => i = n,
                }
                continue;
            }
            let start = i;
            while i < n && hb[i] != b'>' {
                i += 1;
            }
            let tag = &lower[start..i.min(n)];
            if is_block_tag(tag) {
                out.push('\n');
            }
            i = (i + 1).min(n);
            continue;
        }
        let start = i;
        while i < n && hb[i] != b'<' {
            i += 1;
        }
        out.push_str(&html[start..i]);
    }
    decode_and_collapse(&out)
}

fn is_block_tag(tag: &str) -> bool {
    const BLOCK: [&str; 16] = [
        "<p", "</p", "<br", "<div", "</div", "<li", "</li", "<tr", "</tr", "<section", "<article",
        "<ul", "<ol", "<table", "<header", "<footer",
    ];
    BLOCK.iter().any(|b| tag.starts_with(b)) || tag.starts_with("<h") || tag.starts_with("</h")
}

fn decode_and_collapse(s: &str) -> String {
    let replaced = s
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'");
    let mut out = String::with_capacity(replaced.len());
    let mut last_blank = true;
    for line in replaced.lines() {
        let trimmed = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if trimmed.is_empty() {
            if !last_blank {
                out.push('\n');
                last_blank = true;
            }
        } else {
            out.push_str(&trimmed);
            out.push('\n');
            last_blank = false;
        }
    }
    out.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrapfly_url_sets_asp_render_and_format() {
        let url = scrapfly_url("k123", "https://www.elkjop.no/cat/mac", "no").unwrap();
        let q: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(url.host_str(), Some("api.scrapfly.io"));
        assert_eq!(q.get("key").map(String::as_str), Some("k123"));
        assert_eq!(
            q.get("url").map(String::as_str),
            Some("https://www.elkjop.no/cat/mac")
        );
        assert_eq!(q.get("asp").map(String::as_str), Some("true"));
        assert_eq!(q.get("render_js").map(String::as_str), Some("true"));
        assert_eq!(q.get("format").map(String::as_str), Some("markdown"));
        assert_eq!(q.get("country").map(String::as_str), Some("no"));
    }

    #[test]
    fn scrapfly_url_omits_country_when_blank() {
        let url = scrapfly_url("k", "https://x.io", "").unwrap();
        assert!(!url.query_pairs().any(|(k, _)| k == "country"));
    }

    #[test]
    fn extract_scrapfly_content_reads_result_envelope() {
        let body =
            json!({ "result": { "content": "# MacBook Air\n\n14 999 kr", "status_code": 200 } });
        let (content, status) = extract_scrapfly_content(&body).unwrap();
        assert!(content.contains("MacBook Air"));
        assert_eq!(status, 200);
    }

    #[test]
    fn extract_scrapfly_content_rejects_empty() {
        assert!(extract_scrapfly_content(&json!({ "result": { "content": "   " } })).is_none());
        assert!(extract_scrapfly_content(&json!({})).is_none());
    }

    #[test]
    fn html_to_text_strips_tags_scripts_and_decodes() {
        let html = "<html><head><style>.x{color:red}</style></head><body>\
            <h1>MacBook Air</h1><script>track()</script>\
            <p>Pris: 14&nbsp;999&nbsp;kr &amp; gratis frakt</p>\
            <div>M3 &lt;chip&gt;</div></body></html>";
        let text = html_to_text(html);
        assert!(text.contains("MacBook Air"));
        assert!(text.contains("Pris: 14 999 kr & gratis frakt"));
        assert!(text.contains("M3 <chip>"));
        assert!(!text.contains("track()"));
        assert!(!text.contains("color:red"));
        assert!(!text.contains('<') || text.contains("M3 <chip>")); // only decoded entity angle brackets remain
    }

    #[test]
    fn build_enhanced_preview_titles_from_first_heading() {
        let page = EnhancedPage {
            markdown: "# Mac-produkter\n\nInnhold".into(),
            status: 200,
        };
        let preview = build_enhanced_preview("https://www.elkjop.no/x", &page);
        assert_eq!(preview["source"], "enhanced");
        assert_eq!(preview["title"], "Mac-produkter");
        assert_eq!(preview["quarry"]["status"], 200);
    }

    #[test]
    fn preview_title_falls_back_to_host() {
        assert_eq!(preview_title("", "https://www.elkjop.no/cat"), "elkjop.no");
    }
}
