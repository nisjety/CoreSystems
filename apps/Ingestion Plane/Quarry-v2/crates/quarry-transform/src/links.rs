//! Extract outbound links from HTML.

use scraper::{Html, Selector};
use url::Url;

use quarry_core::output::Link;

pub fn extract(html: &str, base: &Url) -> Vec<Link> {
    let doc = Html::parse_document(html);
    let sel = Selector::parse("a[href]").expect("a[href] selector");
    doc.select(&sel)
        .filter_map(|a| {
            let href = a.value().attr("href")?;
            let abs = base.join(href).ok()?;
            Some(Link {
                href: abs.to_string(),
                text: Some(a.text().collect::<String>().trim().to_string())
                    .filter(|t| !t.is_empty()),
                rel: a.value().attr("rel").map(ToString::to_string),
            })
        })
        .collect()
}
