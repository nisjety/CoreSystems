//! The org's web domain rules as the BFF's web search and crawl meet them (EP-9, verification O-23, D-f10-3).
//!
//! The gateway cannot hold an org-core scope, so it asks Model Gateway, which already holds the checker its own web
//! tools use and the switch (`WEB_DOMAIN_POLICY_MODE`):
//!
//! * `POST /v1/web/domain-check` takes up to 32 distinct hosts and answers `{ mode, verdicts: { host: allow | deny } }`.
//! * `POST /v1/web/domain-rules` answers the org's rules for a crawl to carry to every link it follows.
//!
//! `WEB_DOMAIN_POLICY_BFF` (read once into [`crate::config::SpaceFeatureFlags::web_domain_policy_bff`]) is this layer's
//! own switch, default off. Off, nothing here calls Model Gateway and every handler is exactly what it was. On, a
//! verdict binds only when Model Gateway says its mode is `enforce`: under `shadow` the would-be refusals are counted
//! in the log and nothing is dropped, and under `off` Model Gateway reads nothing and allows every host. When the
//! switch is on and the rules cannot be read at all (Model Gateway unreachable, an unreadable answer), the call fails
//! closed with a typed 503: an outage narrows the web surface and never widens it.
//!
//! Only canonical hosts go to Model Gateway: no path, no query, no page content. The log carries counts, never a host.

use std::collections::{BTreeSet, HashMap};

use axum::{
    http::{HeaderMap, StatusCode},
    Json,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    config::AppState,
    domains::chat::shared::{model_token, proxy_model_json},
    envelope::error,
    middleware::AuthenticatedUser,
};

/// Model Gateway's host check.
pub(crate) const CHECK_ROUTE: &str = "/v1/web/domain-check";

/// Model Gateway's read of the org's rules, for a crawl.
pub(crate) const RULES_ROUTE: &str = "/v1/web/domain-rules";

/// The most distinct hosts Model Gateway checks in one call.
const MAX_HOSTS_PER_CALL: usize = 32;

/// The code of a request the org's rules stop.
pub(crate) const BLOCKED_CODE: &str = "org_site_blocked";

/// The code of a request the rules could not be read for.
pub(crate) const UNAVAILABLE_CODE: &str = "org_site_check_unavailable";

/// The code of an answer stream that would send text with no source named (a cached replay), under enforcement.
pub(crate) const SOURCES_UNKNOWN_CODE: &str = "org_site_sources_unknown";

/// Why an answer lost its synthesised text when a source of it was dropped.
pub(crate) const ANSWER_REASON: &str = "org_domain_rules";

/// The canonical host a URL names, read the way Model Gateway reads it: an absolute `http` or `https` URL with no
/// userinfo and a plain dotted hostname (an IP literal and a single-label name have none). `None` is refused under
/// enforcement, because the rules cannot be applied to it.
pub(crate) fn host_of_url(raw: &str) -> Option<String> {
    let parsed = url::Url::parse(raw.trim()).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return None;
    }
    match parsed.host()? {
        url::Host::Domain(domain) if domain.contains('.') => Some(domain.to_ascii_lowercase()),
        _ => None,
    }
}

/// What Model Gateway said about a set of hosts.
#[derive(Debug, Clone)]
pub(crate) struct Checked {
    enforce: bool,
    allowed: HashMap<String, bool>,
}

impl Checked {
    /// Whether the verdicts bind: Model Gateway's mode is `enforce`. Under `shadow` they are only counted.
    pub(crate) fn binds(&self) -> bool {
        self.enforce
    }

    /// Whether the rules let this URL through. A URL with no host the rules can name, and a host Model Gateway gave no
    /// verdict for, are not allowed.
    pub(crate) fn allows(&self, url: &str) -> bool {
        host_of_url(url).is_some_and(|host| self.allowed.get(&host).copied().unwrap_or(false))
    }
}

/// The rules could not be read, so the call does not run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Unreadable;

/// The 503 of a call the rules could not be read for.
pub(crate) fn unavailable_response() -> (StatusCode, Json<Value>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(error(
            UNAVAILABLE_CODE,
            "The organisation's web domain rules cannot be read right now, so this request is not run.",
        )),
    )
}

/// The 403 of a request for a host the org's rules do not allow.
pub(crate) fn blocked_response() -> (StatusCode, Json<Value>) {
    (
        StatusCode::FORBIDDEN,
        Json(error(
            BLOCKED_CODE,
            "Your organisation does not allow web access to this site.",
        )),
    )
}

/// The verdicts for the hosts of `urls`, or `Ok(None)` when nothing is to be checked: the switch is off, `urls` names no
/// host, or Model Gateway's own mode is `off`. A URL with no host the rules can name is given no verdict (so it is not
/// allowed under enforcement) and does not travel.
///
/// # Errors
/// [`Unreadable`] when any call to Model Gateway fails or its answer is not the closed shape.
pub(crate) async fn check_urls(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    urls: &[String],
) -> Result<Option<Checked>, Unreadable> {
    if !state.space_features.web_domain_policy_bff {
        return Ok(None);
    }
    let hosts: BTreeSet<String> = urls.iter().filter_map(|url| host_of_url(url)).collect();
    // A URL with no host is still a URL the rules cannot be applied to, so the caller must reach `allows`, which
    // refuses it. With nothing to ask, the answer is an empty set of verdicts that binds as Model Gateway's mode does,
    // so it is read once for the mode alone.
    let hosts: Vec<String> = hosts.into_iter().collect();
    let token = model_token(state, user, headers).await;
    let url = format!("{}{CHECK_ROUTE}", state.model_gateway_url);
    let mut allowed: HashMap<String, bool> = HashMap::new();
    let mut enforce = false;
    let mut mode_seen = false;
    let batches: Vec<&[String]> = if hosts.is_empty() {
        vec![&[][..]]
    } else {
        hosts.chunks(MAX_HOSTS_PER_CALL).collect()
    };
    for batch in batches {
        let (status, Json(body)) = proxy_model_json(
            state,
            Method::POST,
            &url,
            Some(json!({ "hosts": batch })),
            token.as_deref(),
            user,
        )
        .await;
        if status != StatusCode::OK {
            return Err(Unreadable);
        }
        let mode = body.get("mode").and_then(Value::as_str).ok_or(Unreadable)?;
        match mode {
            "off" => return Ok(None),
            "shadow" => {}
            "enforce" => enforce = true,
            _ => return Err(Unreadable),
        }
        mode_seen = true;
        let verdicts = body
            .get("verdicts")
            .and_then(Value::as_object)
            .ok_or(Unreadable)?;
        for host in batch {
            match verdicts.get(host).and_then(Value::as_str) {
                Some("allow") => {
                    allowed.insert(host.clone(), true);
                }
                Some("deny") => {
                    allowed.insert(host.clone(), false);
                }
                _ => return Err(Unreadable),
            }
        }
    }
    if !mode_seen {
        return Err(Unreadable);
    }
    Ok(Some(Checked { enforce, allowed }))
}

/// Drops from `items` every entry whose `url_field` names a URL the rules do not allow, and returns how many it
/// dropped. Under `shadow` it counts them and drops none.
pub(crate) fn retain_allowed(checked: &Checked, items: &mut Vec<Value>, url_field: &str) -> usize {
    let refused = items
        .iter()
        .filter(|item| !item_allowed(checked, item, url_field))
        .count();
    if checked.binds() {
        items.retain(|item| item_allowed(checked, item, url_field));
    }
    refused
}

fn item_allowed(checked: &Checked, item: &Value, url_field: &str) -> bool {
    item.get(url_field)
        .and_then(Value::as_str)
        .is_some_and(|url| checked.allows(url))
}

/// The URLs the arrays of `data` name under their URL fields, for one check.
fn urls_of(data: &Value, arrays: &[(&str, &str)]) -> Vec<String> {
    arrays
        .iter()
        .flat_map(|(array, field)| {
            data.get(*array)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(move |item| item.get(*field).and_then(Value::as_str).map(str::to_owned))
        })
        .collect()
}

/// Applies the rules to the hits of one search payload: each `(array, url_field)` pair names an array of `data` whose
/// entries are dropped when their URL is refused. Returns how many it DROPPED: `0` with the payload untouched when the
/// switch is off or Model Gateway's mode is `off`, and `0` under `shadow`, which logs the count it would have dropped.
///
/// # Errors
/// [`Unreadable`] when the rules could not be read.
pub(crate) async fn filter_hits(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    data: &mut Value,
    arrays: &[(&str, &str)],
) -> Result<usize, Unreadable> {
    if !state.space_features.web_domain_policy_bff {
        return Ok(0);
    }
    let urls = urls_of(data, arrays);
    if urls.is_empty() {
        return Ok(0);
    }
    let Some(checked) = check_urls(state, user, headers, &urls).await? else {
        return Ok(0);
    };
    let mut refused = 0;
    for (array, field) in arrays {
        if let Some(items) = data.get_mut(*array).and_then(Value::as_array_mut) {
            refused += retain_allowed(&checked, items, field);
        }
    }
    if refused > 0 {
        tracing::info!(
            refused,
            enforced = checked.binds(),
            "org web domain policy (BFF): search hits on hosts the organisation does not allow"
        );
    }
    Ok(if checked.binds() { refused } else { 0 })
}

/// Takes the synthesised answer off a search payload that lost a source, because the text was built from pages the
/// organisation does not allow and cannot be cut back to the sources that remain. The citations are already filtered.
pub(crate) fn withhold_answer(data: &mut Value) {
    let Some(object) = data.as_object_mut() else {
        return;
    };
    if object.get("answer").is_some_and(|answer| !answer.is_null()) {
        object.insert("answer".to_owned(), Value::Null);
        object.insert("answerStatus".to_owned(), json!("unavailable"));
        object.insert("answerReason".to_owned(), json!(ANSWER_REASON));
    }
}

/// The rules a crawl carries to Quarry, as the `domain_rules` object of its request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CrawlRules(pub(crate) Value);

/// Reads the org's rules for a crawl. `Ok(None)` is no rule to carry: the switch is off, Model Gateway's mode is not
/// `enforce`, or the org's rules are open with nothing blocked (so the request stays what it was).
///
/// # Errors
/// [`Unreadable`] when the rules could not be read: the crawl does not start.
pub(crate) async fn crawl_rules(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
) -> Result<Option<CrawlRules>, Unreadable> {
    if !state.space_features.web_domain_policy_bff {
        return Ok(None);
    }
    let token = model_token(state, user, headers).await;
    let url = format!("{}{RULES_ROUTE}", state.model_gateway_url);
    let (status, Json(body)) = proxy_model_json(
        state,
        Method::POST,
        &url,
        Some(json!({})),
        token.as_deref(),
        user,
    )
    .await;
    if status != StatusCode::OK {
        return Err(Unreadable);
    }
    match body.get("mode").and_then(Value::as_str) {
        Some("off" | "shadow") => return Ok(None),
        Some("enforce") => {}
        _ => return Err(Unreadable),
    }
    let rules = body
        .get("rules")
        .and_then(Value::as_object)
        .ok_or(Unreadable)?;
    let mode = rules
        .get("mode")
        .and_then(Value::as_str)
        .ok_or(Unreadable)?;
    let hosts = |key: &str| -> Result<Vec<String>, Unreadable> {
        rules
            .get(key)
            .and_then(Value::as_array)
            .ok_or(Unreadable)?
            .iter()
            .map(|host| host.as_str().map(str::to_owned).ok_or(Unreadable))
            .collect()
    };
    let deny = hosts("deny")?;
    let allow = hosts("allow")?;
    if mode == "open" && deny.is_empty() {
        return Ok(None);
    }
    Ok(Some(CrawlRules(
        json!({ "mode": mode, "deny": deny, "allow": allow }),
    )))
}

/// The org's rules at the start of a crawl: the seed host is judged first, then the rules ride on the request as
/// `domain_rules` for Quarry to apply to every link it follows. `None` lets the crawl start. A seed on a refused host is
/// a typed 403 under enforcement, and a rules read that fails is a typed 503: the crawl does not start, because a crawl
/// that cannot read the rules would follow every link. Off (the default), the body is returned as it came.
pub(crate) async fn guard_crawl(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    body: &mut Value,
) -> Option<(StatusCode, Json<Value>)> {
    if !state.space_features.web_domain_policy_bff {
        return None;
    }
    let seed = body
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    match check_urls(state, user, headers, std::slice::from_ref(&seed)).await {
        Err(Unreadable) => return Some(unavailable_response()),
        Ok(Some(checked)) if !checked.allows(&seed) => {
            tracing::info!(
                enforced = checked.binds(),
                "org web domain policy (BFF): a crawl seed on a host the organisation does not allow"
            );
            if checked.binds() {
                return Some(blocked_response());
            }
        }
        Ok(_) => {}
    }
    attach_crawl_rules(state, user, headers, body).await
}

/// The rules half of [`guard_crawl`], for a crawl that names no seed URL of its own (a re-crawl of a stored source): the
/// org's rules ride on the request as `domain_rules`, so Quarry applies them to every link it follows. `None` lets the
/// crawl start; a rules read that fails is a typed 503 and the crawl does not start. Off (the default), the body is
/// returned as it came.
pub(crate) async fn attach_crawl_rules(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    body: &mut Value,
) -> Option<(StatusCode, Json<Value>)> {
    match crawl_rules(state, user, headers).await {
        Err(Unreadable) => Some(unavailable_response()),
        Ok(Some(rules)) => {
            if let Some(object) = body.as_object_mut() {
                object.insert("domain_rules".to_owned(), rules.0);
            }
            None
        }
        Ok(None) => None,
    }
}

/// One decided gate of an answer stream.
enum StreamGate {
    Pass,
    /// A named source is on a host the rules refuse.
    Block,
    /// Text is coming and no source is named, so no host can be judged (a cached replay carries none).
    SourcesUnknown,
    Unreadable,
}

/// The name and the data (its `data:` lines joined) of one SSE event.
fn sse_event_parts(event: &str) -> (Option<String>, String) {
    let mut name = None;
    let mut data: Vec<&str> = Vec::new();
    for line in event.lines() {
        if let Some(value) = line.strip_prefix("event:") {
            name = Some(value.trim().to_owned());
        } else if let Some(value) = line.strip_prefix("data:") {
            data.push(value.strip_prefix(' ').unwrap_or(value));
        }
    }
    (name, data.join("\n"))
}

/// The SSE `error` event the guard ends a stream with. It carries a code and a message and no source.
fn sse_error_event(code: &str, message: &str) -> bytes::Bytes {
    let body = json!({ "message": message, "reason": code, "code": code });
    bytes::Bytes::from(format!("event: error\ndata: {body}\n\n"))
}

/// The URLs a `citations` event names.
fn citation_urls(data: &str) -> Vec<String> {
    serde_json::from_str::<Value>(data)
        .ok()
        .and_then(|value| value.as_array().cloned())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("url").and_then(Value::as_str).map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// Whether a `delta` or `done` event carries any answer text. An event whose data is not the closed shape is taken to
/// carry text: the guard never waves through what it cannot read.
fn event_carries_text(name: &str, data: &str) -> bool {
    let key = if name == "delta" { "delta" } else { "answer" };
    match serde_json::from_str::<Value>(data) {
        Ok(value) => match value.get(key) {
            None | Some(Value::Null) => false,
            Some(Value::String(text)) => !text.is_empty(),
            Some(_) => true,
        },
        Err(_) => true,
    }
}

/// Judges the hosts of the sources an answer names. `Pass` when the switch is on but Model Gateway's mode is `off`, when
/// every source is allowed, and under `shadow` (which counts the refused ones in the log).
async fn gate_sources(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    urls: &[String],
) -> StreamGate {
    match check_urls(state, user, headers, urls).await {
        Ok(None) => StreamGate::Pass,
        Ok(Some(checked)) => {
            let refused = urls.iter().filter(|url| !checked.allows(url)).count();
            if refused > 0 {
                tracing::info!(
                    refused,
                    enforced = checked.binds(),
                    "org web domain policy (BFF): answer sources on hosts the organisation does not allow"
                );
            }
            if checked.binds() && refused > 0 {
                StreamGate::Block
            } else {
                StreamGate::Pass
            }
        }
        Err(Unreadable) => StreamGate::Unreadable,
    }
}

/// Judges answer text that arrives with no source named: Quarry replays a cached answer as an empty `citations` event
/// and then the text, so the pages it was built from are unknown and may since have been blocked. Only Model Gateway's
/// mode is read (an empty host list). Under `enforce` the text is refused; under `shadow` it is counted and passes; with
/// the mode `off` it passes.
async fn gate_unnamed_sources(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
) -> StreamGate {
    match check_urls(state, user, headers, &[]).await {
        Ok(None) => StreamGate::Pass,
        Ok(Some(checked)) => {
            tracing::info!(
                enforced = checked.binds(),
                "org web domain policy (BFF): answer text with no named source, so no source host can be judged"
            );
            if checked.binds() {
                StreamGate::SourcesUnknown
            } else {
                StreamGate::Pass
            }
        }
        Err(Unreadable) => StreamGate::Unreadable,
    }
}

/// The gate one event decides, or `None` when the stream is still held for a later event.
async fn gate_event(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    name: Option<&str>,
    data: &str,
) -> Option<StreamGate> {
    match name {
        Some("citations") => {
            let urls = citation_urls(data);
            if urls.is_empty() {
                // No source named: keep holding until the next event shows whether text follows.
                None
            } else {
                Some(gate_sources(state, user, headers, &urls).await)
            }
        }
        // Text with no source named before it.
        Some(kind @ ("delta" | "done")) if event_carries_text(kind, data) => {
            Some(gate_unnamed_sources(state, user, headers).await)
        }
        // An error, or a finished stream with no text (a true no-source answer), arrives with nothing to judge.
        Some("error" | "done") => Some(StreamGate::Pass),
        _ => None,
    }
}

/// Applies the org's rules to an answer stream. Quarry sends the `citations` event first and the synthesised text after
/// it, so the sources are known before any text is. The guard holds the stream until that event, reads its hosts, and
/// then either forwards the stream unchanged or, when a source is a host the rules refuse under enforcement, ends it
/// with a typed `error` event before any text goes out: text built from a blocked page cannot be cut back afterwards.
/// A `citations` event that names no URL is not taken as "no source": Quarry replays a cached answer (keyed by org and
/// query, kept an hour) with an empty list, and the page it was built from may have been blocked since. The guard keeps
/// holding; when text follows it asks Model Gateway for its mode, and under `enforce` ends the stream with
/// `org_site_sources_unknown` before any text goes out, under `shadow` it counts and passes. A true no-source answer
/// (no text at all) passes unchanged. A rules read that fails ends the stream with `org_site_check_unavailable`. With
/// the switch off, a non-OK upstream, or Model Gateway's mode not `enforce`, the stream is forwarded as it came.
pub(crate) async fn guard_answer_stream(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    response: axum::response::Response,
) -> axum::response::Response {
    use futures_util::StreamExt;

    if !state.space_features.web_domain_policy_bff || response.status() != StatusCode::OK {
        return response;
    }
    let (parts, body) = response.into_parts();
    let (state, user, headers) = (state.clone(), user.clone(), headers.clone());
    let mut upstream = body.into_data_stream();
    let guarded = async_stream::stream! {
        let mut held: Vec<u8> = Vec::new();
        let mut decided = false;
        while let Some(chunk) = upstream.next().await {
            let bytes = match chunk {
                Ok(bytes) => bytes,
                Err(error) => {
                    yield Err(std::io::Error::other(error));
                    return;
                }
            };
            if decided {
                yield Ok::<bytes::Bytes, std::io::Error>(bytes);
                continue;
            }
            held.extend_from_slice(&bytes);
            // Read each complete event held so far; the first event that can be judged decides.
            let mut cursor = 0;
            while !decided {
                let Some(offset) = held[cursor..].windows(2).position(|pair| pair == b"\n\n") else {
                    break;
                };
                let end = cursor + offset + 2;
                let text = String::from_utf8_lossy(&held[cursor..end]).into_owned();
                cursor = end;
                let (name, data) = sse_event_parts(&text);
                let Some(gate) = gate_event(&state, &user, &headers, name.as_deref(), &data).await else {
                    continue;
                };
                match gate {
                    StreamGate::Pass => {
                        decided = true;
                        yield Ok(bytes::Bytes::from(std::mem::take(&mut held)));
                    }
                    StreamGate::Block => {
                        yield Ok(sse_error_event(
                            BLOCKED_CODE,
                            "Your organisation does not allow web access to a source of this answer.",
                        ));
                        return;
                    }
                    StreamGate::SourcesUnknown => {
                        yield Ok(sse_error_event(
                            SOURCES_UNKNOWN_CODE,
                            "This answer names no source, so your organisation's web domain rules cannot be applied to it.",
                        ));
                        return;
                    }
                    StreamGate::Unreadable => {
                        yield Ok(sse_error_event(
                            UNAVAILABLE_CODE,
                            "The organisation's web domain rules cannot be read right now.",
                        ));
                        return;
                    }
                }
            }
        }
        if !held.is_empty() {
            yield Ok(bytes::Bytes::from(held));
        }
    };
    axum::response::Response::from_parts(parts, axum::body::Body::from_stream(guarded))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_host_is_read_the_way_model_gateway_reads_it() {
        assert_eq!(
            host_of_url("https://Example.ORG/a?b=1#c").as_deref(),
            Some("example.org")
        );
        assert_eq!(
            host_of_url("http://example.org:8080/").as_deref(),
            Some("example.org")
        );
        for refused in [
            "ftp://example.org/",
            "https://user:pw@example.org/",
            "https://127.0.0.1/",
            "https://[::1]/",
            "https://localhost/",
            "example.org",
            "",
        ] {
            assert_eq!(host_of_url(refused), None, "{refused}");
        }
    }

    #[test]
    fn an_url_with_no_verdict_is_not_allowed() {
        let checked = Checked {
            enforce: true,
            allowed: HashMap::from([
                ("ok.example".to_owned(), true),
                ("no.example".to_owned(), false),
            ]),
        };
        assert!(checked.allows("https://ok.example/x"));
        assert!(!checked.allows("https://no.example/x"));
        assert!(
            !checked.allows("https://other.example/x"),
            "a host with no verdict is refused"
        );
        assert!(!checked.allows("https://10.0.0.1/x"));
    }

    #[test]
    fn enforcement_drops_the_refused_and_shadow_only_counts() {
        let allowed = HashMap::from([
            ("ok.example".to_owned(), true),
            ("no.example".to_owned(), false),
        ]);
        let items = || {
            vec![
                json!({"url": "https://ok.example/1"}),
                json!({"url": "https://no.example/2"}),
                json!({"title": "no url"}),
            ]
        };
        let mut enforced = items();
        let dropped = retain_allowed(
            &Checked {
                enforce: true,
                allowed: allowed.clone(),
            },
            &mut enforced,
            "url",
        );
        assert_eq!(dropped, 2, "a hit with no URL counts as refused");
        assert_eq!(enforced, vec![json!({"url": "https://ok.example/1"})]);
        let mut shadow = items();
        let counted = retain_allowed(
            &Checked {
                enforce: false,
                allowed,
            },
            &mut shadow,
            "url",
        );
        assert_eq!(counted, 2);
        assert_eq!(shadow.len(), 3, "shadow drops nothing");
    }
}

/// The crawl half of O-23: the seed host is judged, and the org's rules ride on the request.
#[cfg(test)]
mod crawl_tests {
    use super::*;
    use crate::middleware::AuthorizedMembership;
    use wiremock::matchers::{method, path, path_regex};
    use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

    const BLOCKED: &str = "blocked.example";

    struct DomainCheck {
        mode: &'static str,
    }

    impl Respond for DomainCheck {
        fn respond(&self, request: &Request) -> ResponseTemplate {
            let body: Value = serde_json::from_slice(&request.body).unwrap_or_default();
            let mut verdicts = serde_json::Map::new();
            for host in body["hosts"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                let verdict = if host == BLOCKED { "deny" } else { "allow" };
                verdicts.insert(host.to_owned(), json!(verdict));
            }
            ResponseTemplate::new(200)
                .set_body_json(json!({ "mode": self.mode, "verdicts": verdicts }))
        }
    }

    fn user() -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "user-o23c".to_owned(),
            user_email: "user@o23c.invalid".to_owned(),
            user_name: "User".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-o23c".to_owned()),
            authorized_membership: Some(AuthorizedMembership {
                organization_id: "org-o23c".to_owned(),
                role: "member".to_owned(),
            }),
        }
    }

    /// Model Gateway with a host check in `mode` and a rules answer `rules` (status and body).
    async fn model(mode: &'static str, rules_status: u16, rules: Value) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(CHECK_ROUTE))
            .respond_with(DomainCheck { mode })
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path(RULES_ROUTE))
            .respond_with(ResponseTemplate::new(rules_status).set_body_json(rules))
            .mount(&server)
            .await;
        server
    }

    async fn state(switch: bool, model: &MockServer) -> (AppState, MockServer) {
        let auth = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path_regex(r"^/api/[a-z-]+/token$"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "token": "t" })))
            .mount(&auth)
            .await;
        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth.uri();
        state.model_gateway_url = model.uri();
        state.space_features.web_domain_policy_bff = switch;
        (state, auth)
    }

    async fn calls(server: &MockServer) -> usize {
        server.received_requests().await.unwrap_or_default().len()
    }

    fn enforce_rules() -> Value {
        json!({
            "mode": "enforce",
            "rules": { "mode": "allowlist", "deny": ["bad.example"], "allow": ["good.example"], "revision": 4 }
        })
    }

    async fn guard(state: &AppState, seed: &str) -> (Option<(StatusCode, Value)>, Value) {
        let mut body = json!({ "url": seed, "max_pages": 10 });
        let refusal = guard_crawl(state, &user(), &HeaderMap::new(), &mut body).await;
        (refusal.map(|(status, Json(value))| (status, value)), body)
    }

    #[tokio::test]
    async fn the_rules_ride_on_the_request_for_quarry_to_apply_to_every_link() {
        let model = model("enforce", 200, enforce_rules()).await;
        let (state, _auth) = state(true, &model).await;
        let (refusal, body) = guard(&state, "https://good.example/").await;
        assert!(refusal.is_none());
        assert_eq!(
            body["domain_rules"],
            json!({ "mode": "allowlist", "deny": ["bad.example"], "allow": ["good.example"] }),
            "the revision stays behind; Quarry needs the hosts and the mode"
        );
        assert_eq!(body["url"], "https://good.example/");
        assert_eq!(body["max_pages"], 10);
    }

    #[tokio::test]
    async fn a_seed_on_a_denied_host_is_refused_before_anything_starts() {
        let model = model("enforce", 200, enforce_rules()).await;
        let (state, _auth) = state(true, &model).await;
        let (refusal, body) = guard(&state, &format!("https://{BLOCKED}/start")).await;
        let (status, value) = refusal.expect("refused");
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(value["error"]["code"], BLOCKED_CODE);
        assert!(body.get("domain_rules").is_none());
    }

    #[tokio::test]
    async fn rules_that_cannot_be_read_stop_the_crawl() {
        for rules in [
            (
                503,
                json!({ "error": { "code": "org_site_rules_unreadable" } }),
            ),
            (200, json!({ "mode": "enforce" })),
            (
                200,
                json!({ "mode": "enforce", "rules": { "mode": "open", "deny": "x", "allow": [] } }),
            ),
            (200, json!({ "mode": "surprise", "rules": null })),
        ] {
            let model = model("enforce", rules.0, rules.1.clone()).await;
            let (state, _auth) = state(true, &model).await;
            let (refusal, body) = guard(&state, "https://good.example/").await;
            let (status, value) = refusal
                .unwrap_or_else(|| panic!("a crawl that cannot read the rules starts: {rules:?}"));
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{rules:?}");
            assert_eq!(value["error"]["code"], UNAVAILABLE_CODE, "{rules:?}");
            assert!(body.get("domain_rules").is_none());
        }
        // Model Gateway unreachable.
        let dead = MockServer::start().await;
        let (mut state, _auth) = state(true, &dead).await;
        state.model_gateway_url = "http://127.0.0.1:9".to_owned();
        let (refusal, _) = guard(&state, "https://good.example/").await;
        assert_eq!(
            refusal.map(|(status, _)| status),
            Some(StatusCode::SERVICE_UNAVAILABLE)
        );
    }

    #[tokio::test]
    async fn an_open_org_with_nothing_blocked_sends_no_rules() {
        let open = json!({ "mode": "enforce", "rules": { "mode": "open", "deny": [], "allow": ["x.example"], "revision": 1 } });
        let model = model("enforce", 200, open).await;
        let (state, _auth) = state(true, &model).await;
        let (refusal, body) = guard(&state, "https://good.example/").await;
        assert!(refusal.is_none());
        assert!(
            body.get("domain_rules").is_none(),
            "the request stays what it was"
        );
    }

    #[tokio::test]
    async fn shadow_and_off_change_nothing_and_carry_no_rules() {
        for mode in ["shadow", "off"] {
            let model = model(mode, 200, json!({ "mode": mode, "rules": null })).await;
            let (state, _auth) = state(true, &model).await;
            let (refusal, body) = guard(&state, &format!("https://{BLOCKED}/start")).await;
            assert!(refusal.is_none(), "{mode}: a denied seed is only logged");
            assert!(body.get("domain_rules").is_none(), "{mode}");
        }
    }

    #[tokio::test]
    async fn the_switch_off_asks_nothing_and_leaves_the_body_alone() {
        let model = model("enforce", 200, enforce_rules()).await;
        let (state, _auth) = state(false, &model).await;
        let (refusal, body) = guard(&state, &format!("https://{BLOCKED}/start")).await;
        assert!(refusal.is_none());
        assert_eq!(
            body,
            json!({ "url": format!("https://{BLOCKED}/start"), "max_pages": 10 })
        );
        assert_eq!(calls(&model).await, 0, "off calls nothing");
    }

    #[tokio::test]
    async fn a_hit_batch_over_thirty_two_hosts_is_checked_in_batches() {
        let model = model("enforce", 200, enforce_rules()).await;
        let (state, _auth) = state(true, &model).await;
        let results: Vec<Value> = (0..40)
            .map(|n| json!({ "url": format!("https://site{n:02}.example.com/") }))
            .collect();
        let mut data = json!({ "results": results });
        let dropped = filter_hits(
            &state,
            &user(),
            &HeaderMap::new(),
            &mut data,
            &[("results", "url")],
        )
        .await
        .expect("read");
        assert_eq!(dropped, 0);
        assert_eq!(data["results"].as_array().map(Vec::len), Some(40));
        assert_eq!(calls(&model).await, 2, "40 hosts take two calls");
    }
}
