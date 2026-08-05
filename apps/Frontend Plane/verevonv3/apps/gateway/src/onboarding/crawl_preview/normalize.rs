use serde_json::{json, Value};
use url::Url;

use crate::onboarding::crawl_preview::types::CrawlPayload;

pub(super) fn normalize_quarry_payload(
    payload: Value,
    event_type: &str,
    source: &str,
) -> Option<CrawlPayload> {
    let data = payload.as_object()?.clone();
    match event_type {
        "branding_extracted" => {
            let branding = normalize_branding(&Value::Object(data))?;
            Some(CrawlPayload {
                kind: "branding".into(),
                source: Some(source.into()),
                value: Some(branding),
            })
        }
        "page_fetched" => {
            let snippet = to_snippet(&Value::Object(data.clone()), source)?;
            Some(CrawlPayload {
                kind: "snippet".into(),
                source: Some(source.into()),
                value: Some(snippet),
            })
        }
        "run_failed" => Some(CrawlPayload {
            kind: "warning".into(),
            source: Some(source.into()),
            value: Some(
                json!({ "code": "crawl_failed", "message": data.get("error").and_then(Value::as_str).unwrap_or("Crawl failed.") }),
            ),
        }),
        "run_cancelled" => Some(CrawlPayload {
            kind: "progress".into(),
            source: Some(source.into()),
            value: Some(json!({ "status": "cancelled", "pages": 0, "elements": 0 })),
        }),
        "run_completed" => Some(CrawlPayload {
            kind: "progress".into(),
            source: Some(source.into()),
            value: Some(json!({ "status": "completed", "pages": 0, "elements": 0 })),
        }),
        _ => None,
    }
}

fn to_snippet(payload: &Value, source: &str) -> Option<Value> {
    let url = payload
        .get("url")
        .or_else(|| payload.get("href"))
        .and_then(Value::as_str)?;
    let title = payload
        .get("title")
        .or_else(|| payload.get("name"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| host_label(url));
    let content_type = payload
        .get("content_type")
        .and_then(Value::as_str)
        .unwrap_or("text/html");
    let kind = if content_type.starts_with("image/") {
        "image"
    } else if content_type.contains("pdf")
        || content_type.contains("word")
        || content_type.contains("excel")
    {
        "file"
    } else {
        "text"
    };
    Some(json!({
        "id": payload.get("id").and_then(Value::as_str).unwrap_or(url),
        "kind": kind,
        "title": title,
        "excerpt": payload.get("text").or_else(|| payload.get("excerpt")).and_then(Value::as_str),
        "url": url,
        "contentType": content_type,
        "source": source,
        "elementCount": payload.get("links").and_then(Value::as_u64).unwrap_or(0),
    }))
}

fn normalize_branding(payload: &Value) -> Option<Value> {
    let root = payload.get("branding").unwrap_or(payload);
    let static_signals = root.get("static_signals").unwrap_or(root);
    let site_name = static_signals.get("site_name").and_then(Value::as_str);
    let theme_color = static_signals.get("theme_color").and_then(Value::as_str);
    let favicon = static_signals.get("favicon").and_then(Value::as_str);
    let logo_candidate = root.get("logo_candidate").and_then(Value::as_str);
    let palette = root
        .get("palette")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    if site_name.is_none()
        && theme_color.is_none()
        && favicon.is_none()
        && logo_candidate.is_none()
        && palette.is_empty()
    {
        return None;
    }

    Some(json!({
        "siteName": site_name,
        "themeColor": theme_color,
        "favicon": favicon,
        "logoCandidate": logo_candidate,
        "palette": palette,
    }))
}

fn host_label(input: &str) -> String {
    Url::parse(input)
        .ok()
        .and_then(|url| url.host_str().map(ToOwned::to_owned))
        .unwrap_or_else(|| input.to_owned())
}
