//! Bounded structural evidence only. Visibility owns interpretation and tasks.
use scraper::{ElementRef, Html, Node, Selector};
use serde::Serialize;
use std::collections::BTreeSet;
use url::Url;

#[derive(Debug, Serialize)]
pub(crate) struct Alternate { pub lang: String, pub url: String }
#[derive(Debug, Serialize)]
pub(crate) struct StructuredData {
    pub documents: usize, pub invalid: usize, pub types: Vec<String>, pub partial: bool,
}
#[derive(Debug, Serialize)]
pub(crate) struct SeoEvidence {
    pub version: &'static str,
    pub noindex: bool,
    pub nofollow: bool,
    pub robots_status: u16,
    pub word_count: usize,
    pub hreflang: Vec<Alternate>,
    pub invalid_alternates: usize,
    pub alternates_partial: bool,
    pub structured_data: StructuredData,
}
fn bounded(value: &str, max: usize) -> String {
    value.chars().filter(|c| !c.is_control()).take(max).collect()
}
fn types(value: &serde_json::Value, out: &mut BTreeSet<String>, depth: usize) {
    if depth > 8 || out.len() >= 32 { return; }
    match value {
        serde_json::Value::Object(map) => {
            if let Some(t) = map.get("@type") {
                for s in t.as_array().map(|v| v.iter().filter_map(|x| x.as_str()).collect()).unwrap_or_else(|| t.as_str().into_iter().collect::<Vec<_>>()) {
                    if out.len() < 32 { out.insert(bounded(s, 128)); }
                }
            }
            for v in map.values() { types(v, out, depth + 1); }
        }
        serde_json::Value::Array(values) => for v in values.iter().take(1000) { types(v, out, depth + 1); },
        _ => {}
    }
}
fn directives(value: &str, noindex: &mut bool, nofollow: &mut bool) {
    let value = value.to_ascii_lowercase();
    // Agent-specific X-Robots-Tag values must not be applied to all crawlers.
    let content = if let Some((agent, content)) = value.split_once(':') {
        let agent=agent.trim();
        if agent.contains(',') || agent.chars().any(char::is_whitespace) { &value }
        else if agent != "googlebot" { return; } else { content }
    } else { &value };
    for token in content.split(|c: char| c == ',' || c.is_ascii_whitespace()) {
        *noindex |= token == "noindex" || token == "none";
        *nofollow |= token == "nofollow" || token == "none";
    }
}
pub(crate) fn extract(doc: &Html, html: &str, final_url: &Url, headers: &[(String,String)], robots_status: u16) -> SeoEvidence {
    let mut noindex = false; let mut nofollow = false;
    for meta in doc.select(&Selector::parse("meta[name][content]").unwrap()) {
        if meta.value().attr("name").is_some_and(|n| n.eq_ignore_ascii_case("robots") || n.eq_ignore_ascii_case("googlebot")) {
            directives(meta.value().attr("content").unwrap_or(""), &mut noindex, &mut nofollow);
        }
    }
    for (_, value) in headers.iter().filter(|(name,_)| name.eq_ignore_ascii_case("x-robots-tag")) {
        directives(value, &mut noindex, &mut nofollow);
    }
    let mut word_count = 0;
    if let Some(body) = doc.select(&Selector::parse("body").unwrap()).next() {
        for node in body.descendants() {
            if let Node::Text(text) = node.value() {
                let excluded = node.ancestors().filter_map(ElementRef::wrap).any(|e| matches!(e.value().name(), "script"|"style"|"noscript"|"template"));
                if !excluded { word_count += text.split_whitespace().count(); }
            }
        }
    }
    let variants = quarry_transform::attributes::extract_hreflang_variants(html, final_url.as_str());
    let alternates_partial = variants.len() > 50;
    let mut invalid_alternates = 0;
    let hreflang = variants.into_iter().take(50).filter_map(|v| {
        let parsed = Url::parse(&v.url).ok();
        if v.lang.len() > 40 || parsed.as_ref().is_none_or(|u| !crate::map_routes::public_discovery_url(u)) { invalid_alternates += 1; return None; }
        Some(Alternate { lang: bounded(&v.lang, 40), url: v.url })
    }).collect();
    let scripts = doc.select(&Selector::parse("script[type]").unwrap()).filter(|e| e.value().attr("type").is_some_and(|t| t.trim().eq_ignore_ascii_case("application/ld+json"))).collect::<Vec<_>>();
    let mut schema = StructuredData { documents: 0, invalid: 0, types: vec![], partial: scripts.len() > 32 };
    let mut names = BTreeSet::new();
    for script in scripts.into_iter().take(32) {
        schema.documents += 1;
        let text = script.text().collect::<String>();
        if text.len() > 65536 { schema.partial = true; continue; }
        match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(value) if value.is_object() || value.is_array() => types(&value, &mut names, 0),
            _ => schema.invalid += 1,
        }
    }
    schema.types = names.into_iter().collect();
    SeoEvidence { version: "seo_page_v1", noindex, nofollow, robots_status, word_count, hreflang, invalid_alternates, alternates_partial, structured_data: schema }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extracts_observed_directives_alternates_and_json_without_leaking_content() {
        let html = r#"<meta name="robots" content="noindex, follow"><link rel="alternate" hreflang="nb-NO" href="/no"><body>Visible public words<script type="application/ld+json">{"@graph":[{"@type":"Organization","name":"privateMarker"}]}</script><script type="application/ld+json">invalid</script><style>secret css</style></body>"#;
        let v = extract(&Html::parse_document(html), html, &Url::parse("https://example.com/").unwrap(), &[("x-robots-tag".into(), "googlebot: nofollow".into())], 200);
        assert!(v.noindex && v.nofollow); assert_eq!(v.word_count, 3);
        assert_eq!(v.hreflang[0].url, "https://example.com/no");
        assert_eq!(v.structured_data.invalid, 1); assert_eq!(v.structured_data.types, vec!["Organization"]);
        assert!(!serde_json::to_string(&v).unwrap().contains("privateMarker"));
    }
    #[test]
    fn colon_valued_directives_do_not_hide_an_earlier_noindex() {
        let mut noindex=false;let mut nofollow=false;
        directives("noindex, unavailable_after: 1 Jan 2030", &mut noindex,&mut nofollow);assert!(noindex);
        noindex=false;directives("bingbot: noindex", &mut noindex,&mut nofollow);assert!(!noindex);
    }
    #[test]
    fn does_not_apply_another_bots_header_or_call_missing_schema_invalid() {
        let v = extract(&Html::parse_document("<p>hello</p>"), "", &Url::parse("https://example.com/").unwrap(), &[("X-Robots-Tag".into(), "bingbot: noindex".into())], 404);
        assert!(!v.noindex); assert_eq!(v.structured_data.documents,0); assert_eq!(v.structured_data.invalid,0);
    }
}
