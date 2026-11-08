//! Reciprocal Rank Fusion (RRF) for hybrid search (OSS-parity P0 1C).
//!
//! Merges Quarry's lexical recall (`TantivyLocalIndex` / SERP `SearchResult`s)
//! with Data Plane semantic hits (`VectorHit`) into one ranking. RRF needs no
//! score calibration between the two systems — it fuses by rank position:
//!
//!   fused(d) = Σ_lists 1 / (k + rank_list(d))
//!
//! `k=60` is the canonical constant from the original RRF paper (Cormack et al.).
//!
//! A page is one result however its URL was spelled: both legs are merged under
//! [`dedupe_key`]. A metasearch row keeps the engines that returned it and its
//! fused per-engine score, so the engine-agreement signal survives the fusion.

use std::collections::HashMap;

use url::form_urlencoded;

use crate::serp::SearchResult;
use crate::vector_index::VectorHit;

/// Canonical RRF dampening constant.
pub const RRF_K: f32 = 60.0;

/// One page as the fused list holds it.
struct Page {
    /// The URL exactly as the first list that named the page gave it.
    url: String,
    score: f32,
    title: Option<String>,
    snippet: Option<String>,
    engines: Vec<String>,
    fusion_score: Option<f32>,
}

/// What one list says about one page.
struct Mention<'a> {
    url: &'a str,
    title: Option<&'a str>,
    snippet: Option<&'a str>,
    engines: &'a [String],
    fusion_score: Option<f32>,
}

/// The pages seen so far, in first-seen order, with an index by dedupe key.
#[derive(Default)]
struct Fusion {
    index: HashMap<String, usize>,
    pages: Vec<Page>,
}

impl Fusion {
    fn add(&mut self, rank: usize, k: f32, mention: Mention<'_>) {
        let key = dedupe_key(mention.url);
        let at = match self.index.get(&key).copied() {
            Some(at) => at,
            None => {
                let at = self.pages.len();
                self.pages.push(Page {
                    url: mention.url.to_owned(),
                    score: 0.0,
                    title: None,
                    snippet: None,
                    engines: Vec::new(),
                    fusion_score: None,
                });
                self.index.insert(key, at);
                at
            }
        };
        let page = &mut self.pages[at];
        page.score += 1.0 / (k + rank as f32);
        fill(&mut page.title, mention.title);
        fill(&mut page.snippet, mention.snippet);
        for engine in mention.engines {
            let name = engine.trim();
            if !name.is_empty() && !page.engines.iter().any(|known| known.as_str() == name) {
                page.engines.push(name.to_owned());
            }
        }
        if page.fusion_score.is_none() {
            page.fusion_score = mention.fusion_score;
        }
    }
}

/// Takes the first offered text that is not blank.
fn fill(slot: &mut Option<String>, offered: Option<&str>) {
    if slot.is_none() {
        *slot = offered
            .filter(|text| !text.trim().is_empty())
            .map(str::to_owned);
    }
}

/// Fuse lexical + vector result lists via RRF. A page is one result under
/// [`dedupe_key`]. The first list that names a page supplies its URL, and
/// metadata comes from the first list that has a non-blank value. Engines are
/// the union of the engines that returned the page, and the fused score is the
/// first one given. A vector hit adds no engine: the Data Plane's index is
/// Quarry's own crawl, not an upstream engine. Ties in the fused score keep
/// first-seen order (lexical list first). Returns the top `limit` as
/// `SearchResult`s tagged `provider = "hybrid"`, with `score` and `highlights`
/// unset: those are reranker outputs, and the reranker runs after fusion.
pub fn rrf_fuse(
    lexical: &[SearchResult],
    vector: &[VectorHit],
    k: f32,
    limit: usize,
) -> Vec<SearchResult> {
    let mut fusion = Fusion::default();
    for (i, r) in lexical.iter().enumerate() {
        fusion.add(
            i + 1,
            k,
            Mention {
                url: &r.url,
                title: r.title.as_deref(),
                snippet: r.snippet.as_deref(),
                engines: &r.engines,
                fusion_score: r.fusion_score,
            },
        );
    }
    for (i, hit) in vector.iter().enumerate() {
        fusion.add(
            i + 1,
            k,
            Mention {
                url: &hit.url,
                title: hit.title.as_deref(),
                snippet: hit.snippet.as_deref(),
                engines: &[],
                fusion_score: None,
            },
        );
    }

    // `sort_by` is stable, so a tie keeps the order in which pages were first seen.
    let mut pages = fusion.pages;
    pages.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    pages
        .into_iter()
        .take(limit)
        .enumerate()
        .map(|(i, page)| SearchResult {
            url: page.url,
            title: page.title,
            snippet: page.snippet,
            rank: (i + 1) as u32,
            provider: "hybrid".into(),
            engines: page.engines,
            fusion_score: page.fusion_score,
            ..Default::default()
        })
        .collect()
}

/// Query parameters that carry campaign or click attribution and never choose
/// which document a URL names. `ref` is listed as the router has always listed
/// it. It is mostly a referral marker, but a site that uses it as a selector can
/// have two of its pages merged.
const TRACKING_PARAMS: &[&str] = &[
    "fbclid",
    "gclid",
    "gbraid",
    "wbraid",
    "dclid",
    "msclkid",
    "yclid",
    "twclid",
    "igshid",
    "mc_cid",
    "mc_eid",
    "li_fat_id",
    "mkt_tok",
    "ref",
    "ref_src",
    "_hsenc",
    "_hsmi",
];

fn is_tracking_param(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.starts_with("utm_") || TRACKING_PARAMS.contains(&name.as_str())
}

/// The key under which two result URLs name one page. Two spellings of a page get
/// one key, so the page is ranked once. The key is never shown: callers keep the
/// URL they were given.
///
/// Folded: `http` and `https` (one document), host case, a `www.` prefix, a
/// trailing dot on the host, the default ports 80 and 443, one trailing slash on
/// the path, the fragment, userinfo, the order of query parameters, and the
/// tracking parameters ([`TRACKING_PARAMS`] and every `utm_*`).
///
/// Kept apart: path case, every other parameter and its value, the query's
/// encoding (`%26` is not `&`), the order of a repeated name, and any other port.
/// Unparseable input is trimmed and lowercased, so an identical malformed string
/// still dedupes.
pub fn dedupe_key(raw: &str) -> String {
    let trimmed = raw.trim();
    let Ok(parsed) = url::Url::parse(trimmed) else {
        return trimmed.to_lowercase();
    };
    let scheme = match parsed.scheme() {
        "http" | "https" => "http",
        other => other,
    };
    let host_owned = parsed.host_str().unwrap_or_default().to_lowercase();
    let host = host_owned.strip_suffix('.').unwrap_or(host_owned.as_str());
    let host = host.strip_prefix("www.").unwrap_or(host);
    let port = match parsed.port() {
        Some(port) if port != 80 && port != 443 => format!(":{port}"),
        _ => String::new(),
    };
    let path = parsed.path();
    let path = path.strip_suffix('/').unwrap_or(path);
    let query = canonical_query(parsed.query().unwrap_or_default());
    let query = if query.is_empty() {
        String::new()
    } else {
        format!("?{query}")
    };
    format!("{scheme}://{host}{port}{path}{query}")
}

/// The query without its tracking parameters, its pairs sorted by name, and each
/// pair re-encoded, so one set of pairs has one spelling. A repeated name keeps
/// its own order, since a server may read it as a list. The bytes are kept as
/// they were sent: a value that is not UTF-8 (a Latin-1 `%E6`, say) is not turned
/// into U+FFFD, so two distinct values never share a spelling.
fn canonical_query(query: &str) -> String {
    let mut pairs: Vec<(Vec<u8>, Vec<u8>)> = query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            (decode_component(name), decode_component(value))
        })
        .filter(|(name, _)| !is_tracking_param(&String::from_utf8_lossy(name)))
        .collect();
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    pairs
        .iter()
        .map(|(name, value)| {
            format!(
                "{}={}",
                form_urlencoded::byte_serialize(name).collect::<String>(),
                form_urlencoded::byte_serialize(value).collect::<String>(),
            )
        })
        .collect::<Vec<String>>()
        .join("&")
}

/// One query component, decoded as `application/x-www-form-urlencoded` decodes
/// it but kept as bytes: `+` is a space and each `%XX` is one byte. Nothing is
/// replaced here, so two different byte sequences never read alike.
fn decode_component(component: &str) -> Vec<u8> {
    let bytes = component.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                match (hex_value(bytes[i + 1]), hex_value(bytes[i + 2])) {
                    (Some(high), Some(low)) => {
                        out.push((high << 4) | low);
                        i += 2;
                    }
                    _ => out.push(b'%'),
                }
            }
            byte => out.push(byte),
        }
        i += 1;
    }
    out
}

fn hex_value(digit: u8) -> Option<u8> {
    match digit {
        b'0'..=b'9' => Some(digit - b'0'),
        b'a'..=b'f' => Some(digit - b'a' + 10),
        b'A'..=b'F' => Some(digit - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A metasearch row as `serp::SearXNGSearch` reports it: the engines that
    /// returned the page and the fused per-engine score.
    fn metasearch(url: &str, engines: &[&str], fusion_score: Option<f32>) -> SearchResult {
        SearchResult {
            url: url.into(),
            title: Some(url.into()),
            snippet: None,
            rank: 1,
            provider: "searxng".into(),
            engines: engines.iter().map(|e| (*e).to_owned()).collect(),
            fusion_score,
            ..Default::default()
        }
    }

    #[test]
    fn engines_and_fusion_score_survive_the_fusion() {
        let lex = vec![metasearch(
            "https://a.example/",
            &["bing", "duckduckgo"],
            Some(0.25),
        )];
        let fused = rrf_fuse(&lex, &[vh("https://b.example/")], RRF_K, 10);
        let a = fused
            .iter()
            .find(|r| r.url == "https://a.example/")
            .unwrap();
        assert_eq!(a.engines, vec!["bing", "duckduckgo"]);
        assert_eq!(a.fusion_score, Some(0.25));
    }

    #[test]
    fn a_vector_only_page_names_no_engine_and_has_no_fusion_score() {
        let lex = vec![metasearch("https://a.example/", &["bing"], Some(0.5))];
        let fused = rrf_fuse(&lex, &[vh("https://b.example/")], RRF_K, 10);
        let b = fused
            .iter()
            .find(|r| r.url == "https://b.example/")
            .unwrap();
        assert!(b.engines.is_empty());
        assert_eq!(b.fusion_score, None);
    }

    #[test]
    fn the_vector_leg_adds_no_engine_to_a_page_it_shares_with_the_serp() {
        // The Data Plane's index is Quarry's own crawl, not an upstream engine, so
        // a hit from it must not raise the engine-agreement count.
        let lex = vec![metasearch("https://a.example/", &["bing"], Some(0.5))];
        let fused = rrf_fuse(&lex, &[vh("https://a.example/")], RRF_K, 10);
        assert_eq!(fused.len(), 1);
        assert_eq!(fused[0].engines, vec!["bing"]);
    }

    #[test]
    fn engines_of_a_page_named_twice_are_merged_without_repeats() {
        let lex = vec![
            metasearch("https://a.example/", &["bing", "duckduckgo"], None),
            metasearch("https://a.example/", &["duckduckgo", "google cse"], None),
        ];
        let fused = rrf_fuse(&lex, &[], RRF_K, 10);
        assert_eq!(fused.len(), 1);
        assert_eq!(fused[0].engines, vec!["bing", "duckduckgo", "google cse"]);
    }

    #[test]
    fn the_fusion_score_comes_from_the_first_list_that_has_one() {
        let lex = vec![
            metasearch("https://a.example/", &["bing"], None),
            metasearch("https://a.example/", &["duckduckgo"], Some(0.2)),
            metasearch("https://a.example/", &["google cse"], Some(0.9)),
        ];
        let fused = rrf_fuse(&lex, &[], RRF_K, 10);
        assert_eq!(fused[0].fusion_score, Some(0.2));
    }

    #[test]
    fn a_blank_title_does_not_block_the_next_list() {
        let mut blank = metasearch("https://a.example/", &["bing"], None);
        blank.title = Some("   ".into());
        let mut hit = vh("https://a.example/");
        hit.title = Some("Pricing".into());
        let fused = rrf_fuse(&[blank], &[hit], RRF_K, 10);
        assert_eq!(fused[0].title.as_deref(), Some("Pricing"));
    }

    #[test]
    fn one_page_spelled_differently_in_the_two_legs_is_one_result() {
        let lex = vec![metasearch(
            "https://Docs.Example.test/pricing/?utm_source=bing",
            &["bing", "duckduckgo"],
            Some(0.3),
        )];
        let fused = rrf_fuse(&lex, &[vh("http://docs.example.test/pricing")], RRF_K, 10);
        assert_eq!(
            fused.len(),
            1,
            "the stored and the SERP spelling are one page"
        );
        assert_eq!(
            fused[0].url,
            "https://Docs.Example.test/pricing/?utm_source=bing"
        );
        assert_eq!(fused[0].engines, vec!["bing", "duckduckgo"]);
    }

    #[test]
    fn dedupe_key_folds_the_spellings_of_one_page() {
        let base = dedupe_key("https://docs.example.test/pricing");
        for variant in [
            "http://docs.example.test/pricing",
            "https://DOCS.example.TEST/pricing",
            "https://docs.example.test:443/pricing",
            "http://docs.example.test:80/pricing",
            "https://docs.example.test./pricing",
            "https://docs.example.test/pricing/",
            "https://docs.example.test/pricing#plans",
            "https://docs.example.test/pricing?utm_source=news&utm_medium=email",
            "https://docs.example.test/pricing?gclid=abc",
            "https://docs.example.test/pricing?gbraid=abc",
            "https://user@docs.example.test/pricing",
            "  https://docs.example.test/pricing  ",
        ] {
            assert_eq!(dedupe_key(variant), base, "variant: {variant}");
        }
    }

    #[test]
    fn dedupe_key_keeps_distinct_pages_apart() {
        let base = dedupe_key("https://docs.example.test/pricing?plan=pro");
        for other in [
            "https://docs.example.test/Pricing?plan=pro",
            "https://docs.example.test/pricing?plan=team",
            "https://docs.example.test/pricing",
            "https://docs.example.test:8443/pricing?plan=pro",
            "https://other.example.test/pricing?plan=pro",
        ] {
            assert_ne!(dedupe_key(other), base, "distinct page: {other}");
        }
    }

    #[test]
    fn an_encoded_ampersand_is_part_of_a_value_not_a_separator() {
        assert_ne!(
            dedupe_key("https://docs.example.test/search?x=1%26y%3D2"),
            dedupe_key("https://docs.example.test/search?x=1&y=2"),
        );
    }

    #[test]
    fn query_order_is_not_identity_but_a_repeated_name_keeps_its_order() {
        assert_eq!(
            dedupe_key("https://docs.example.test/search?b=2&a=1"),
            dedupe_key("https://docs.example.test/search?a=1&b=2"),
        );
        assert_ne!(
            dedupe_key("https://docs.example.test/search?tag=a&tag=b"),
            dedupe_key("https://docs.example.test/search?tag=b&tag=a"),
        );
    }

    #[test]
    fn plus_and_percent_twenty_are_the_same_space_in_a_query() {
        assert_eq!(
            dedupe_key("https://docs.example.test/search?q=solar+panels"),
            dedupe_key("https://docs.example.test/search?q=solar%20panels"),
        );
    }

    #[test]
    fn values_that_are_not_utf8_stay_apart() {
        // %E5 and %E6 are Latin-1 bytes; read as text, both would become U+FFFD.
        assert_ne!(
            dedupe_key("https://docs.example.test/search?q=%E5"),
            dedupe_key("https://docs.example.test/search?q=%E6"),
        );
        // The case of the hex digits does not change the byte: %e5 is %E5.
        assert_eq!(
            dedupe_key("https://docs.example.test/search?q=%e5"),
            dedupe_key("https://docs.example.test/search?q=%E5"),
        );
    }

    #[test]
    fn distinct_pages_with_non_utf8_values_stay_two_results() {
        let fused = rrf_fuse(
            &[sr("https://docs.example.test/search?q=%FF")],
            &[vh("https://docs.example.test/search?q=%FE")],
            RRF_K,
            10,
        );
        assert_eq!(fused.len(), 2, "two distinct pages must not merge");
    }

    fn sr(url: &str) -> SearchResult {
        SearchResult {
            url: url.into(),
            title: Some(url.into()),
            snippet: None,
            rank: 0,
            provider: "lex".into(),
            ..Default::default()
        }
    }
    fn vh(url: &str) -> VectorHit {
        VectorHit {
            url: url.into(),
            title: None,
            snippet: Some(format!("snip-{url}")),
            document_id: None,
        }
    }

    #[test]
    fn item_in_both_lists_ranks_first() {
        // "b" appears in both → highest fused score.
        let lex = vec![sr("a"), sr("b"), sr("c")];
        let vec = vec![vh("b"), vh("d")];
        let fused = rrf_fuse(&lex, &vec, RRF_K, 10);
        assert_eq!(fused[0].url, "b");
        assert_eq!(fused[0].provider, "hybrid");
    }

    #[test]
    fn union_includes_vector_only_items() {
        let lex = vec![sr("a")];
        let vec = vec![vh("z")];
        let fused = rrf_fuse(&lex, &vec, RRF_K, 10);
        let urls: Vec<&str> = fused.iter().map(|r| r.url.as_str()).collect();
        assert!(urls.contains(&"a") && urls.contains(&"z"));
    }

    #[test]
    fn empty_vector_preserves_lexical_order() {
        let lex = vec![sr("a"), sr("b"), sr("c")];
        let fused = rrf_fuse(&lex, &[], RRF_K, 10);
        let urls: Vec<&str> = fused.iter().map(|r| r.url.as_str()).collect();
        assert_eq!(urls, vec!["a", "b", "c"]);
    }

    #[test]
    fn metadata_merges_snippet_from_vector() {
        let lex = vec![sr("a")]; // snippet None
        let vec = vec![vh("a")]; // snippet Some
        let fused = rrf_fuse(&lex, &vec, RRF_K, 10);
        assert_eq!(fused[0].snippet.as_deref(), Some("snip-a"));
    }

    #[test]
    fn limit_truncates_and_reranks() {
        let lex = vec![sr("a"), sr("b"), sr("c")];
        let fused = rrf_fuse(&lex, &[], RRF_K, 2);
        assert_eq!(fused.len(), 2);
        assert_eq!(fused[0].rank, 1);
        assert_eq!(fused[1].rank, 2);
    }
}
