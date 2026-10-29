//! Schema.org JSON-LD extraction.
//!
//! Most modern publishers (news sites, ecommerce, blogs, SaaS landing
//! pages) embed schema.org metadata as JSON-LD inside
//! `<script type="application/ld+json">` blocks. This is gold for
//! downstream consumers — the publisher has already told us the
//! `Article`'s `headline`, `author`, `datePublished`, the `Product`'s
//! `sku`, `offers.price`, the `Organization`'s `logo` and `sameAs`
//! social handles, etc. Deriving these from raw HTML is heuristic and
//! error-prone; reading them from JSON-LD is exact.
//!
//! Scope of this module:
//!
//! - Find every `<script type="application/ld+json">` in the document
//! - Parse each block as JSON, tolerating both bare objects and
//!   `@graph` arrays
//! - Surface the **commonly useful** fields as a `JsonLdSummary` so
//!   consumers (the onboarding wizard, the wiki ingester, the
//!   retrieval index) don't have to re-walk arbitrary JSON
//! - Stay defensive: malformed JSON in one block must not break
//!   extraction of the others. Schema.org publishers ship a lot of
//!   subtly invalid JSON in production.
//!
//! Out of scope: full schema.org class hierarchy. We pick the
//! interesting types (`Article`, `BlogPosting`, `NewsArticle`,
//! `Product`, `Organization`, `WebPage`, `WebSite`, `BreadcrumbList`,
//! `Person`) and only surface their hot fields. Everything else is
//! returned in `raw` for callers who need to dig.

use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::OnceLock;

/// Summary of useful fields lifted from a page's JSON-LD blocks. All
/// fields are best-effort — publishers ship missing/partial data
/// frequently.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct JsonLdSummary {
    /// Headline for an Article/BlogPosting/NewsArticle. Falls back to
    /// `name` when `headline` is absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headline: Option<String>,
    /// One-line description from `description`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Author display name. Lifted from `author.name` when `author`
    /// is an object, or from `author` directly when it's a bare
    /// string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// ISO-8601 publish date — `datePublished`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_published: Option<String>,
    /// ISO-8601 last-modified — `dateModified`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_modified: Option<String>,
    /// Image URL — first usable from `image` (string, object with
    /// `url`, or array of either).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// Publisher / organization name. Article.publisher.name OR a
    /// top-level Organization's name when the page-level type is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    /// For Product pages: SKU.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sku: Option<String>,
    /// Product offer price as a printable string (currency + amount).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<String>,
    /// Product offer availability (`InStock`, `OutOfStock`, etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub availability: Option<String>,
    /// All distinct `@type` values seen across blocks. Useful for
    /// downstream routing — e.g. "this is a Product page, route to
    /// the product ingest pipeline".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub types: Vec<String>,
    /// Raw JSON-LD blocks as parsed `Value`s, in document order.
    /// Callers who need fields we don't lift can walk these. Kept on
    /// the summary so we don't re-parse downstream.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub raw: Vec<Value>,
}

fn ld_script_selector() -> &'static Selector {
    static SEL: OnceLock<Selector> = OnceLock::new();
    SEL.get_or_init(|| {
        Selector::parse(r#"script[type="application/ld+json"]"#).expect("static selector")
    })
}

/// Extract a summary of schema.org metadata from a document.
///
/// Returns `None` when no JSON-LD blocks were found OR when every
/// block was malformed. Returns `Some(summary)` even if only one
/// field could be lifted — partial data is better than none.
pub fn extract(html: &str) -> Option<JsonLdSummary> {
    let doc = Html::parse_document(html);
    let mut summary = JsonLdSummary::default();
    let mut any = false;

    for el in doc.select(ld_script_selector()) {
        let raw = el.text().collect::<String>();
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Some sites wrap their JSON-LD in HTML comments or have
        // a leading BOM; trim aggressively.
        let cleaned = trimmed
            .trim_start_matches('\u{feff}')
            .trim_start_matches("<!--")
            .trim_end_matches("-->")
            .trim();
        let parsed: Value = match serde_json::from_str(cleaned) {
            Ok(v) => v,
            Err(_) => continue, // tolerate malformed blocks; common in the wild
        };
        any = true;
        // The block can be a single object, an array of objects, or
        // a `@graph` envelope. Walk all top-level items.
        for item in iter_items(&parsed) {
            absorb(&mut summary, item);
            summary.raw.push(item.clone());
        }
    }

    if !any {
        return None;
    }
    Some(summary)
}

fn iter_items(v: &Value) -> Box<dyn Iterator<Item = &Value> + '_> {
    if let Value::Array(arr) = v {
        return Box::new(arr.iter());
    }
    if let Some(graph) = v.get("@graph").and_then(|g| g.as_array()) {
        return Box::new(graph.iter());
    }
    Box::new(std::iter::once(v))
}

fn absorb(summary: &mut JsonLdSummary, item: &Value) {
    if let Some(ty) = type_string(item) {
        if !summary.types.iter().any(|t| t == &ty) {
            summary.types.push(ty.clone());
        }
        match ty.as_str() {
            "Article" | "BlogPosting" | "NewsArticle" | "TechArticle" | "ScholarlyArticle" => {
                absorb_article(summary, item);
            }
            "Product" => absorb_product(summary, item),
            "Organization" | "Corporation" | "LocalBusiness" => {
                if summary.publisher.is_none() {
                    summary.publisher = string_field(item, "name");
                }
                if summary.image.is_none() {
                    summary.image = image_field(item);
                }
            }
            "WebPage" | "WebSite" => {
                if summary.description.is_none() {
                    summary.description = string_field(item, "description");
                }
                if summary.headline.is_none() {
                    summary.headline = string_field(item, "name");
                }
            }
            _ => {}
        }
    }
    // Even if we didn't recognise the @type, opportunistically fill
    // missing fields from anything that looks right.
    if summary.headline.is_none() {
        summary.headline = string_field(item, "headline").or_else(|| string_field(item, "name"));
    }
    if summary.description.is_none() {
        summary.description = string_field(item, "description");
    }
    if summary.image.is_none() {
        summary.image = image_field(item);
    }
}

fn absorb_article(summary: &mut JsonLdSummary, item: &Value) {
    summary.headline = summary
        .headline
        .clone()
        .or_else(|| string_field(item, "headline"))
        .or_else(|| string_field(item, "name"));
    summary.description = summary
        .description
        .clone()
        .or_else(|| string_field(item, "description"));
    summary.date_published = summary
        .date_published
        .clone()
        .or_else(|| string_field(item, "datePublished"));
    summary.date_modified = summary
        .date_modified
        .clone()
        .or_else(|| string_field(item, "dateModified"));
    summary.image = summary.image.clone().or_else(|| image_field(item));
    summary.author = summary.author.clone().or_else(|| author_field(item));
    if summary.publisher.is_none() {
        if let Some(publisher) = item.get("publisher") {
            summary.publisher = string_field(publisher, "name");
        }
    }
}

fn absorb_product(summary: &mut JsonLdSummary, item: &Value) {
    summary.headline = summary
        .headline
        .clone()
        .or_else(|| string_field(item, "name"));
    summary.description = summary
        .description
        .clone()
        .or_else(|| string_field(item, "description"));
    summary.image = summary.image.clone().or_else(|| image_field(item));
    summary.sku = summary.sku.clone().or_else(|| string_field(item, "sku"));

    if let Some(offers) = item.get("offers") {
        let offers_iter: Box<dyn Iterator<Item = &Value>> = match offers {
            Value::Array(a) => Box::new(a.iter()),
            _ => Box::new(std::iter::once(offers)),
        };
        for offer in offers_iter {
            if summary.price.is_none() {
                let amount =
                    string_field(offer, "price").or_else(|| number_field_as_string(offer, "price"));
                let currency = string_field(offer, "priceCurrency");
                if let Some(amt) = amount {
                    summary.price = Some(match currency {
                        Some(cur) => format!("{cur} {amt}"),
                        None => amt,
                    });
                }
            }
            if summary.availability.is_none() {
                summary.availability = string_field(offer, "availability")
                    .map(|s| s.rsplit('/').next().unwrap_or(&s).to_string());
            }
        }
    }
}

fn type_string(v: &Value) -> Option<String> {
    match v.get("@type")? {
        Value::String(s) => Some(s.clone()),
        // schema.org allows arrays of types; take the first as the
        // primary classification.
        Value::Array(arr) => arr.iter().find_map(|x| x.as_str().map(String::from)),
        _ => None,
    }
}

fn string_field(v: &Value, key: &str) -> Option<String> {
    match v.get(key)? {
        Value::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        }
        _ => None,
    }
}

fn number_field_as_string(v: &Value, key: &str) -> Option<String> {
    match v.get(key)? {
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn image_field(v: &Value) -> Option<String> {
    let img = v.get("image")?;
    match img {
        Value::String(s) => non_empty(s),
        Value::Object(_) => string_field(img, "url").or_else(|| string_field(img, "contentUrl")),
        Value::Array(arr) => arr.iter().find_map(|x| match x {
            Value::String(s) => non_empty(s),
            Value::Object(_) => string_field(x, "url").or_else(|| string_field(x, "contentUrl")),
            _ => None,
        }),
        _ => None,
    }
}

fn author_field(v: &Value) -> Option<String> {
    let a = v.get("author")?;
    match a {
        Value::String(s) => non_empty(s),
        Value::Object(_) => string_field(a, "name"),
        Value::Array(arr) => arr.iter().find_map(|x| match x {
            Value::String(s) => non_empty(s),
            Value::Object(_) => string_field(x, "name"),
            _ => None,
        }),
        _ => None,
    }
}

fn non_empty(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_article_summary() {
        let html = r##"
        <html><head>
        <script type="application/ld+json">
        {
          "@context": "https://schema.org",
          "@type": "Article",
          "headline": "Norge i 2026",
          "description": "Politisk analyse",
          "datePublished": "2026-05-01T12:00:00+02:00",
          "dateModified": "2026-05-02T08:00:00+02:00",
          "author": { "@type": "Person", "name": "Ola Nordmann" },
          "publisher": { "@type": "Organization", "name": "Aftenposten" },
          "image": "https://aftenposten.no/img/x.jpg"
        }
        </script></head><body></body></html>"##;
        let s = extract(html).expect("summary");
        assert_eq!(s.headline.as_deref(), Some("Norge i 2026"));
        assert_eq!(s.author.as_deref(), Some("Ola Nordmann"));
        assert_eq!(s.publisher.as_deref(), Some("Aftenposten"));
        assert!(s.date_published.is_some());
        assert!(s.image.is_some());
        assert_eq!(s.types, vec!["Article".to_string()]);
    }

    #[test]
    fn extracts_product_summary() {
        let html = r##"
        <script type="application/ld+json">
        {
          "@context": "https://schema.org",
          "@type": "Product",
          "name": "Vitamin C Serum",
          "sku": "VS-001",
          "image": ["https://x/a.jpg", "https://x/b.jpg"],
          "offers": {
            "@type": "Offer",
            "price": 299.00,
            "priceCurrency": "NOK",
            "availability": "https://schema.org/InStock"
          }
        }
        </script>"##;
        let s = extract(html).expect("summary");
        assert_eq!(s.headline.as_deref(), Some("Vitamin C Serum"));
        assert_eq!(s.sku.as_deref(), Some("VS-001"));
        // serde_json normalizes `299.00` → `"299.0"`; we just check
        // the price string carries both currency and amount.
        let price = s.price.as_deref().expect("price");
        assert!(price.starts_with("NOK "), "price = {price}");
        assert!(price.contains("299"), "price = {price}");
        assert_eq!(s.availability.as_deref(), Some("InStock"));
    }

    #[test]
    fn handles_graph_envelope() {
        let html = r##"
        <script type="application/ld+json">
        {
          "@context": "https://schema.org",
          "@graph": [
            {"@type": "WebSite", "name": "SkinSecret.no"},
            {"@type": "WebPage", "description": "Koreansk hudpleie"}
          ]
        }
        </script>"##;
        let s = extract(html).expect("summary");
        assert!(s.types.contains(&"WebSite".to_string()));
        assert!(s.types.contains(&"WebPage".to_string()));
        assert_eq!(s.description.as_deref(), Some("Koreansk hudpleie"));
    }

    #[test]
    fn tolerates_malformed_block() {
        let html = r##"
        <script type="application/ld+json">{ not json }</script>
        <script type="application/ld+json">{"@type":"Article","headline":"Survives"}</script>
        "##;
        let s = extract(html).expect("summary survives bad sibling");
        assert_eq!(s.headline.as_deref(), Some("Survives"));
    }

    #[test]
    fn returns_none_when_no_ld_json() {
        let html = "<html><head></head><body><p>just html</p></body></html>";
        assert!(extract(html).is_none());
    }
}
