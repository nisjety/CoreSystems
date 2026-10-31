//! The Visma API reference, embedded from `reference/api_reference.json`: the Finance ERP API (the
//! service and interactive flavours share one set of operations), SalesOrders v3 and JournalTx v1. The
//! file is derived from Visma's own API documents by `scripts/build_api_reference.py`. It answers two
//! questions offline: which operation a question needs (search), and which query parameter names an
//! endpoint declares, so an unknown filter can be refused instead of being dropped by Visma silently.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::Deserialize;

const JSON: &str = include_str!("../reference/api_reference.json");

/// The API a relative catalog path such as `/v1/customer` belongs to.
const DEFAULT_API: &str = "erp";
/// Words that say nothing about which operation is wanted.
const STOP_WORDS: &[&str] = &[
    "a", "an", "all", "and", "are", "by", "for", "from", "get", "how", "in", "is", "list", "of",
    "on", "or", "the", "to", "what", "which", "with",
];
/// Longest question the search takes into account; the rest is ignored.
const MAX_WORDS: usize = 12;

#[derive(Debug, Deserialize)]
pub struct Reference {
    pub retrieved: String,
    pub apis: BTreeMap<String, Api>,
    pub operations: Vec<Operation>,
}

/// One documented API.
#[derive(Debug, Deserialize)]
pub struct Api {
    pub title: String,
    pub spec_version: String,
    /// Where the specification was downloaded from.
    pub source: String,
    /// Scheme and host, for example `https://salesorder.visma.net`.
    pub hosts: Vec<String>,
    /// Only the Finance ERP API has an interactive (signed-in user) flavour.
    #[serde(default)]
    pub interactive: Option<Interactive>,
}

#[derive(Debug, Deserialize)]
pub struct Interactive {
    /// Whether the interactive API has exactly the operations and parameters of the service API.
    pub same_operations: bool,
    /// The `aud` of an interactive token (Visma's portal calls it the API base URL). Not a route: calls go to the
    /// same host and paths as the service API.
    pub audience: String,
    pub scopes: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct Operation {
    /// Key in [`Reference::apis`].
    pub api: String,
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub operation_id: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Query parameter names (GET operations only).
    #[serde(default)]
    pub query: Vec<String>,
    /// Query parameters the API requires.
    #[serde(default)]
    pub required: Vec<String>,
}

/// One search result and how well it matched.
#[derive(Debug)]
pub struct Hit {
    pub operation: &'static Operation,
    pub score: u32,
}

/// The embedded reference. The file is generated and tested, so a parse failure is a build defect.
pub fn reference() -> &'static Reference {
    static REFERENCE: OnceLock<Reference> = OnceLock::new();
    REFERENCE.get_or_init(|| {
        serde_json::from_str(JSON)
            .expect("reference/api_reference.json is valid (covered by a test)")
    })
}

/// The documented API with this key.
pub fn api(id: &str) -> Option<&'static Api> {
    reference().apis.get(id)
}

/// The API and path a catalog `api_endpoint` points at. An absolute URL goes by its host; a relative path
/// belongs to the Finance ERP API. `None` when no documented API has that host.
pub fn locate(api_endpoint: &str) -> Option<(&'static str, &str)> {
    let endpoint = api_endpoint.split('?').next().unwrap_or(api_endpoint);
    let Some((scheme, rest)) = endpoint.split_once("://") else {
        return Some((DEFAULT_API, endpoint));
    };
    let (host, path) = match rest.find('/') {
        Some(slash) => (&rest[..slash], &rest[slash..]),
        None => (rest, ""),
    };
    let origin = format!("{scheme}://{host}");
    let (id, _) = reference()
        .apis
        .iter()
        .find(|(_, api)| api.hosts.contains(&origin))?;
    Some((id.as_str(), path))
}

/// The GET operation behind a catalog `api_endpoint`, if the reference has one.
pub fn get_operation(api_endpoint: &str) -> Option<&'static Operation> {
    let (api, path) = locate(api_endpoint)?;
    reference()
        .operations
        .iter()
        .find(|op| op.api == api && op.method == "GET" && op.path == path)
}

/// Names in `keys` that the GET operation behind `api_endpoint` does not declare (compared without
/// case). `None` when the reference has no such operation: the caller cannot check, so it must not refuse.
pub fn unknown_query_names<'a>(
    api_endpoint: &str,
    keys: impl IntoIterator<Item = &'a str>,
) -> Option<Vec<String>> {
    let op = get_operation(api_endpoint)?;
    let known: Vec<String> = op
        .query
        .iter()
        .map(|name| name.to_ascii_lowercase())
        .collect();
    Some(
        keys.into_iter()
            .filter(|key| !known.contains(&key.to_ascii_lowercase()))
            .map(str::to_string)
            .collect(),
    )
}

/// The spelling the API declares for the query name `name` (which is compared without case), so the request
/// carries the name exactly as Visma documents it, whatever case the caller wrote. `None` when the reference
/// has no such operation or does not declare the name.
pub fn declared_spelling(api_endpoint: &str, name: &str) -> Option<&'static str> {
    get_operation(api_endpoint)?
        .query
        .iter()
        .find(|declared| declared.eq_ignore_ascii_case(name))
        .map(String::as_str)
}

/// Operations that match at least one word of `question`, best first, optionally only one HTTP method
/// and one API. Words are matched against the path (weight 3), the operation id and summary (2), the
/// tags and query parameter names (1).
pub fn search(question: &str, method: Option<&str>, api: Option<&str>, limit: usize) -> Vec<Hit> {
    let words: Vec<String> = words(question).into_iter().take(MAX_WORDS).collect();
    if words.is_empty() {
        return Vec::new();
    }
    let method = method.map(str::to_ascii_uppercase);
    let mut hits: Vec<Hit> = reference()
        .operations
        .iter()
        .filter(|op| method.as_deref().is_none_or(|m| op.method == m))
        .filter(|op| api.is_none_or(|a| op.api == a))
        .filter_map(|op| {
            let score = score(op, &words);
            (score > 0).then_some(Hit {
                operation: op,
                score,
            })
        })
        .collect();
    hits.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then(a.operation.path.len().cmp(&b.operation.path.len()))
            .then(a.operation.path.cmp(&b.operation.path))
            .then(a.operation.method.cmp(&b.operation.method))
            .then(a.operation.api.cmp(&b.operation.api))
    });
    hits.truncate(limit);
    hits
}

/// A path segment names a word, allowing the plural (`salesorders` names `salesorder`).
fn names(segment: &str, word: &str) -> bool {
    segment == word || segment.strip_suffix('s') == Some(word)
}

/// `api`, `v1`, `v3`: leading path segments that are not resources.
fn is_prefix(segment: &str) -> bool {
    segment == "api"
        || (segment.len() > 1
            && segment.starts_with('v')
            && segment[1..].chars().all(|c| c.is_ascii_digit()))
}

fn score(op: &Operation, words: &[String]) -> u32 {
    let path = op.path.to_ascii_lowercase();
    let segments: Vec<String> = op
        .path
        .split('/')
        .filter(|s| !s.is_empty() && !s.starts_with('{'))
        .map(str::to_ascii_lowercase)
        .collect();
    let id = op.operation_id.to_ascii_lowercase();
    let summary = op.summary.to_ascii_lowercase();
    let tags: Vec<String> = op.tags.iter().map(|t| t.to_ascii_lowercase()).collect();
    let params: Vec<String> = op.query.iter().map(|p| p.to_ascii_lowercase()).collect();
    let matched: u32 = words
        .iter()
        .map(|word| {
            let mut s = 0;
            if path.contains(word.as_str()) {
                s += 3;
            }
            if segments.iter().any(|segment| names(segment, word)) {
                s += 5;
            }
            if id.contains(word.as_str()) {
                s += 2;
            }
            if summary.contains(word.as_str()) {
                s += 2;
            }
            if tags.iter().any(|t| t.contains(word.as_str())) {
                s += 1;
            }
            if params.iter().any(|p| p.contains(word.as_str())) {
                s += 1;
            }
            s
        })
        .sum();
    // Visma names resources in compounds (`salesorder`, `supplierinvoice`): consecutive words of the
    // question joined together can name a path segment.
    let candidates: Vec<String> = (0..words.len())
        .flat_map(|start| {
            (start + 1..=(start + 3).min(words.len())).map(move |end| words[start..end].concat())
        })
        .collect();
    let compounds = candidates
        .iter()
        .filter(|c| segments.iter().any(|segment| names(segment, c)))
        .count();
    let compound_bonus = u32::try_from(compounds)
        .unwrap_or(u32::MAX)
        .saturating_mul(8);
    // The resource an operation acts on is its last segment: `/v1/salesorder` acts on the sales order,
    // `/v1/salesorder/{n}/attachment` on an attachment of one.
    let on_resource = segments.last().map_or(0, |last| {
        let word_hit = u32::from(words.iter().any(|w| names(last, w)));
        let compound_hit = u32::from(candidates.iter().any(|c| names(last, c)));
        word_hit * 5 + compound_hit * 8
    });
    // Sub-resources rank below their base resource. `/api/v3/` and `/v1/` are prefixes, not levels.
    let resources = segments.iter().skip_while(|s| is_prefix(s)).count();
    let sub_resource_levels = u32::try_from(resources.saturating_sub(1)).unwrap_or(u32::MAX);
    matched
        .saturating_add(compound_bonus)
        .saturating_add(on_resource)
        .saturating_sub(3 * sub_resource_levels)
}

/// Lower-case words of a question. Camel case splits too, so `lastModifiedDateTime` gives four words.
fn words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut after_lower = false;
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            if ch.is_uppercase() && after_lower && !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            after_lower = ch.is_lowercase();
            current.extend(ch.to_lowercase());
        } else {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            after_lower = false;
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out.retain(|word| word.chars().count() >= 2 && !STOP_WORDS.contains(&word.as_str()));
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SALESORDER_LIST: &str = "https://salesorder.visma.net/api/v3/SalesOrders";

    #[test]
    fn the_embedded_reference_documents_the_three_apis() {
        let reference = reference();
        for id in ["erp", "salesorder_v3", "journaltx_v1"] {
            assert!(reference.apis.contains_key(id), "{id} missing");
            assert!(
                reference.operations.iter().any(|op| op.api == id),
                "{id} has no operations"
            );
        }
        assert!(
            reference.operations.len() >= 550,
            "{}",
            reference.operations.len()
        );
        assert_eq!(
            reference.retrieved.len(),
            10,
            "a YYYY-MM-DD date: {}",
            reference.retrieved
        );
        let erp = api("erp").expect("erp");
        assert!(erp.title.contains("Finance"));
        assert!(!erp.spec_version.is_empty());
    }

    #[test]
    fn a_declared_filter_is_returned_in_its_own_spelling() {
        assert_eq!(
            declared_spelling("/v1/customer", "LASTMODIFIEDDATETIME"),
            Some("lastModifiedDateTime")
        );
        assert_eq!(
            declared_spelling("/v1/customer", "lastmodifieddatetime"),
            Some("lastModifiedDateTime")
        );
        assert_eq!(declared_spelling("/v1/customer", "customerClass"), None);
        assert_eq!(declared_spelling("/v1/no-such-endpoint", "x"), None);
    }

    #[test]
    fn the_interactive_erp_api_is_recorded_with_its_scopes() {
        let interactive = api("erp")
            .and_then(|a| a.interactive.as_ref())
            .expect("interactive info");
        assert!(interactive.same_operations);
        assert_eq!(
            interactive.audience,
            "https://api.finance.visma.net/erp/interactive"
        );
        assert!(interactive
            .scopes
            .iter()
            .any(|s| s == "vismanet_erp_interactive_api:read"));
        assert!(api("salesorder_v3").is_some_and(|a| a.interactive.is_none()));
    }

    #[test]
    fn a_catalog_endpoint_is_located_by_host_and_path() {
        assert_eq!(locate("/v1/customer"), Some(("erp", "/v1/customer")));
        assert_eq!(
            locate("https://api.finance.visma.net/v1/customer"),
            Some(("erp", "/v1/customer"))
        );
        assert_eq!(
            locate(SALESORDER_LIST),
            Some(("salesorder_v3", "/api/v3/SalesOrders"))
        );
        assert_eq!(
            locate("https://salesorder.visma.net/api/v3/SalesOrders?pageSize=1"),
            Some(("salesorder_v3", "/api/v3/SalesOrders"))
        );
        assert_eq!(locate("https://elsewhere.example/v1/customer"), None);
    }

    #[test]
    fn a_get_operation_lists_its_query_parameters_and_required_ones() {
        let customers = get_operation("/v1/customer").expect("customers is in the reference");
        for name in [
            "lastModifiedDateTime",
            "status",
            "expandPaymentMethods",
            "pageSize",
        ] {
            assert!(customers.query.iter().any(|q| q == name), "{name} missing");
        }
        assert!(customers.required.is_empty());
        let budgets = get_operation("/v1/budget").expect("budgets is in the reference");
        assert!(
            budgets.required.iter().any(|q| q == "branch"),
            "{:?}",
            budgets.required
        );
        let orders = get_operation(SALESORDER_LIST).expect("sales orders v3 is in the reference");
        for name in [
            "customerId",
            "status",
            "modifiedSince",
            "pageIndex",
            "expand",
        ] {
            assert!(orders.query.iter().any(|q| q == name), "{name} missing");
        }
    }

    #[test]
    fn unknown_names_are_found_without_case_and_only_for_known_endpoints() {
        let unknown = unknown_query_names(
            "/v1/customer",
            ["status", "NAME", "lastModifiedDateTimeX", "Status"],
        );
        assert_eq!(unknown, Some(vec!["lastModifiedDateTimeX".to_string()]));
        assert_eq!(
            unknown_query_names(SALESORDER_LIST, ["customerId", "customerClass"]),
            Some(vec!["customerClass".to_string()])
        );
        assert_eq!(
            unknown_query_names("/v1/does-not-exist", ["anything"]),
            None
        );
    }

    #[test]
    fn a_question_finds_its_endpoint_first() {
        let hits = search("customer", Some("GET"), Some("erp"), 5);
        assert_eq!(
            hits.first().map(|h| h.operation.path.as_str()),
            Some("/v1/customer"),
            "{hits:?}"
        );
        assert!(hits
            .iter()
            .all(|h| h.operation.method == "GET" && h.operation.api == "erp"));
    }

    #[test]
    fn each_api_can_be_searched_on_its_own() {
        let orders = search("sales orders", Some("GET"), Some("salesorder_v3"), 3);
        assert_eq!(
            orders.first().map(|h| h.operation.path.as_str()),
            Some("/api/v3/SalesOrders"),
            "{orders:?}"
        );
        assert!(orders.iter().all(|h| h.operation.api == "salesorder_v3"));
        let vouchers = search("vouchers", None, Some("journaltx_v1"), 3);
        assert_eq!(
            vouchers.first().map(|h| h.operation.path.as_str()),
            Some("/api/v1/vouchers"),
            "{vouchers:?}"
        );
        assert!(search("customer", None, Some("no-such-api"), 3).is_empty());
    }

    #[test]
    fn method_filters_and_limits_are_applied() {
        let posts = search("sales order", Some("post"), None, 3);
        assert!(!posts.is_empty() && posts.len() <= 3);
        assert!(
            posts.iter().all(|h| h.operation.method == "POST"),
            "{posts:?}"
        );
        assert!(search("zzqx", None, None, 5).is_empty());
        assert!(
            search("the and for", None, None, 5).is_empty(),
            "stop words only match nothing"
        );
    }

    #[test]
    fn a_base_resource_outranks_its_sub_resources_whatever_the_path_prefix() {
        let v3 = search("create sales order", Some("POST"), Some("salesorder_v3"), 3);
        assert_eq!(
            v3.first().map(|h| h.operation.path.as_str()),
            Some("/api/v3/SalesOrders"),
            "{v3:?}"
        );
        let erp = search("create sales order", Some("POST"), Some("erp"), 3);
        assert_eq!(
            erp.first().map(|h| h.operation.path.as_str()),
            Some("/v1/salesorder"),
            "{erp:?}"
        );
    }

    #[test]
    fn camel_case_and_punctuation_split_into_words() {
        assert_eq!(
            words("lastModifiedDateTime"),
            vec!["last", "modified", "date", "time"]
        );
        assert_eq!(
            words("Sales-order, lines?"),
            vec!["sales", "order", "lines"]
        );
        assert!(words("a I x").is_empty(), "single letters are not words");
    }

    #[test]
    fn prefix_segments_are_recognised() {
        for prefix in ["api", "v1", "v3", "v12"] {
            assert!(is_prefix(prefix), "{prefix}");
        }
        for resource in ["vouchers", "v", "vat", "salesorders"] {
            assert!(!is_prefix(resource), "{resource}");
        }
    }
}
