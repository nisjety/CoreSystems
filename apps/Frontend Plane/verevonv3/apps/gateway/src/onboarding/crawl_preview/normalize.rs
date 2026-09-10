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
        // `page_fetched` is quarry's PRE-transform event ({url,status,
        // duration_ms,content_type}, plus `title` when the orchestrator
        // re-emits it) and can never carry text. `page_extracted` is the
        // post-transform event ({url,title,title_source,excerpt,summary,
        // word_count,lang,driver,…}) and is what actually fills the card.
        // Both map to a snippet keyed by URL; the handler's SnippetLedger
        // lets the richer one update the card instead of duplicating it.
        "page_fetched" | "page_extracted" => {
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
    // `.and_then(Value::as_str)` alone treats an upstream `"url": ""` /
    // `"title": ""` as present, since `Some("")` is not `None` — the
    // `unwrap_or_else` title fallback and the early-return `?` on a missing
    // url never fire, and an empty title then fails the frontend's
    // `min(1)` schema, silently dropping every snippet for pages with no
    // `<title>` tag (the common case) even though the crawl itself
    // succeeded. Trim and treat blank the same as absent for both.
    // A plain `fn`, not a closure: closures don't get the same lifetime
    // elision as free functions, and `Fn(&Value) -> Option<&str>` inferred
    // from a closure literal here fixes the output to one too-short lifetime,
    // rejecting reuse across `payload`'s and `url`'s different borrows.
    fn non_blank(value: &Value) -> Option<&str> {
        value.as_str().map(str::trim).filter(|s| !s.is_empty())
    }
    let url = payload
        .get("url")
        .or_else(|| payload.get("href"))
        .and_then(non_blank)?;
    let html_title = payload
        .get("title")
        .or_else(|| payload.get("name"))
        .and_then(non_blank);
    let title = html_title
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| host_label(url));
    // Provenance: `page_extracted` states it (`html` | `model` | `host`);
    // for a bare `page_fetched` derive it from whether a title was present.
    let title_source = payload
        .get("title_source")
        .and_then(non_blank)
        .unwrap_or(if html_title.is_some() { "html" } else { "host" });
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
        "id": payload.get("id").and_then(non_blank).unwrap_or(url),
        "kind": kind,
        "title": title,
        // Blank excerpts become `null` (not `""`) so the frontend's
        // nullish schema and the ledger's "has text" check agree.
        "excerpt": payload.get("excerpt").or_else(|| payload.get("text")).and_then(non_blank),
        "summary": payload.get("summary").and_then(non_blank),
        "titleSource": title_source,
        "wordCount": payload.get("word_count").and_then(Value::as_u64),
        "driver": payload.get("driver").and_then(non_blank),
        "lang": payload.get("lang").and_then(non_blank),
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

#[cfg(test)]
mod tests {
    use super::{normalize_quarry_payload, to_snippet};
    use serde_json::json;

    fn extracted_payload() -> serde_json::Value {
        json!({
            "url": "https://aquatiq.com/",
            "title": "Aquatiq – hygiene for matindustrien",
            "title_source": "model",
            "excerpt": "Vi leverer hygieneløsninger, kjemikalier og kompetanse til næringsmiddelindustrien.",
            "summary": "Leverandør av hygieneløsninger til matindustrien.",
            "word_count": 412,
            "lang": "no",
            "driver": "browser",
            "content_type": "text/html; charset=utf-8",
            "fingerprint": "blake3:abc",
        })
    }

    // The whole point of `page_extracted`: the card gets a real title and
    // real text, with provenance, for BOTH the seed scrape and the live crawl.
    #[test]
    fn page_extracted_maps_to_a_rich_snippet_for_seed_and_live() {
        for source in ["seed", "live"] {
            let mapped = normalize_quarry_payload(extracted_payload(), "page_extracted", source)
                .expect("page_extracted yields a payload");
            assert_eq!(mapped.kind, "snippet");
            assert_eq!(mapped.source.as_deref(), Some(source));
            let snippet = mapped.value.unwrap();
            assert_eq!(snippet["title"], "Aquatiq – hygiene for matindustrien");
            assert_eq!(snippet["titleSource"], "model");
            assert!(snippet["excerpt"]
                .as_str()
                .unwrap()
                .starts_with("Vi leverer"));
            assert_eq!(
                snippet["summary"],
                "Leverandør av hygieneløsninger til matindustrien."
            );
            assert_eq!(snippet["wordCount"], 412);
            assert_eq!(snippet["lang"], "no");
            assert_eq!(snippet["driver"], "browser");
            assert_eq!(snippet["kind"], "text");
            assert_eq!(snippet["source"], source);
            assert_eq!(snippet["id"], "https://aquatiq.com/");
        }
    }

    #[test]
    fn page_extracted_with_host_title_keeps_host_provenance_and_null_summary() {
        let payload = json!({
            "url": "https://aquatiq.com/",
            "title": "aquatiq.com",
            "title_source": "host",
            "excerpt": "Some text",
            "word_count": 2,
            "driver": "static",
        });
        let snippet = to_snippet(&payload, "live").unwrap();
        assert_eq!(snippet["title"], "aquatiq.com");
        assert_eq!(snippet["titleSource"], "host");
        assert!(snippet["summary"].is_null());
        assert!(snippet["lang"].is_null());
    }

    // A pre-transform page_fetched (no text) still yields a snippet, marked
    // with derived provenance, so the wizard shows progress immediately; the
    // ledger later lets page_extracted replace it.
    #[test]
    fn page_fetched_derives_provenance_and_null_excerpt() {
        let bare =
            json!({ "url": "https://aquatiq.com/", "status": 200, "content_type": "text/html" });
        let snippet = to_snippet(&bare, "live").unwrap();
        assert_eq!(snippet["titleSource"], "host");
        assert!(snippet["excerpt"].is_null());
        let titled = json!({ "url": "https://aquatiq.com/om-oss", "title": "Om oss" });
        let snippet = to_snippet(&titled, "live").unwrap();
        assert_eq!(snippet["titleSource"], "html");
    }

    #[test]
    fn blank_excerpt_is_null_not_empty_string() {
        let payload = json!({ "url": "https://aquatiq.com/", "title": "T", "excerpt": "   " });
        let snippet = to_snippet(&payload, "seed").unwrap();
        assert!(snippet["excerpt"].is_null());
    }

    // Regression: a page with no <title> tag comes back from the scraper as
    // `"title": ""`, not an absent field, and `Some("")` does not trigger
    // `unwrap_or_else`. The frontend's zod schema requires a non-empty
    // title, so this silently dropped every snippet from every page without
    // a title -- crawls that fetched real content looked like they scraped
    // nothing.
    #[test]
    fn blank_title_falls_back_to_host_label() {
        let payload = json!({ "url": "https://aquatiq.com/", "title": "" });
        let snippet = to_snippet(&payload, "live").expect("title-less page still yields a snippet");
        assert_eq!(snippet["title"], "aquatiq.com");
        assert_eq!(snippet["url"], "https://aquatiq.com/");
    }

    #[test]
    fn whitespace_only_title_falls_back_to_host_label() {
        let payload = json!({ "url": "https://aquatiq.com/", "title": "   " });
        let snippet = to_snippet(&payload, "live").unwrap();
        assert_eq!(snippet["title"], "aquatiq.com");
    }

    #[test]
    fn blank_id_falls_back_to_url() {
        let payload = json!({ "url": "https://aquatiq.com/", "id": "", "title": "Aquatiq" });
        let snippet = to_snippet(&payload, "live").unwrap();
        assert_eq!(snippet["id"], "https://aquatiq.com/");
    }

    #[test]
    fn blank_url_is_rejected_not_passed_through_empty() {
        let payload = json!({ "url": "", "title": "Aquatiq" });
        assert!(to_snippet(&payload, "live").is_none());
    }

    #[test]
    fn present_title_and_id_are_kept_as_is() {
        let payload = json!({
            "url": "https://aquatiq.com/about",
            "id": "page-42",
            "title": "About Aquatiq",
        });
        let snippet = to_snippet(&payload, "live").unwrap();
        assert_eq!(snippet["title"], "About Aquatiq");
        assert_eq!(snippet["id"], "page-42");
    }
}
