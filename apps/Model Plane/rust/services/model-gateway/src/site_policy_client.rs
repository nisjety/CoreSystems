//! The org's domain rules as the gateway's web tools enforce them (Wave 3 EP-9 W3).
//!
//! org-core owns the rules (DEC-19). Before a web tool reaches a host, this module asks org-core whether the rules
//! allow that host, and keeps each answer for [`POLICY_TTL`], so a turn that reads one site several times asks once.
//! The pure rules (the canonical host, the reader of a URL, the verdict from an answer, the switch) live in
//! `mp_contracts::site_policy`. This file holds the HTTP call, the cache and the two entry points the tool loop uses:
//! a fetch and a search.
//!
//! `WEB_DOMAIN_POLICY_MODE` decides how far this binds. Under `off` nothing is read and the request is exactly what it
//! was. Under `shadow` the same refusals are logged and nothing is refused. Under `enforce` a host whose answer cannot
//! be read is refused, and so is a URL with no host the rules can name, so an outage of org-core narrows the web tools
//! and never widens them. A check sends org-core canonical hosts only: no path, no query and no content.

use std::collections::{BTreeSet, HashMap};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use mp_contracts::site_policy::{
    host_of_url, routable_org_id, verdict_for, HostVerdict, SiteResolution, WebDomainPolicyMode,
    MAX_RESOLVE_HOSTS, WEB_DOMAIN_POLICY_MODE_ENV,
};

use mp_contracts::model_plane::v1::WebSearchResult;

use crate::quarry::SearchReply;
use crate::state::AppState;

/// How long one host's answer is served before org-core is asked again.
pub const POLICY_TTL: Duration = Duration::from_secs(30);

/// The longest a resolve call may take. An answer that comes later is an unreadable one.
const RESOLVE_TIMEOUT: Duration = Duration::from_secs(5);

/// The code of a web call the org's domain rules stop. It is a typed tool error: the model reads the code and the
/// host, and the words after it are copy.
pub const SITE_BLOCKED_CODE: &str = "org_site_blocked";

/// The process's switch, read once so it cannot change under a request.
#[must_use]
pub fn policy_mode() -> WebDomainPolicyMode {
    #[cfg(test)]
    {
        if let Some(mode) = test_seam::mode() {
            return mode;
        }
    }
    static MODE: OnceLock<WebDomainPolicyMode> = OnceLock::new();
    *MODE.get_or_init(|| {
        WebDomainPolicyMode::from_env_value(std::env::var(WEB_DOMAIN_POLICY_MODE_ENV).ok().as_deref())
    })
}

/// A test seam for the switch. The process's switch is read once, so a test cannot set it through the environment
/// without changing every other test in the binary. A test names the mode its own thread sees instead, and the guard
/// restores what the thread saw before.
#[cfg(test)]
pub(crate) mod test_seam {
    use std::cell::Cell;

    use mp_contracts::site_policy::WebDomainPolicyMode;

    thread_local! {
        static MODE: Cell<Option<WebDomainPolicyMode>> = const { Cell::new(None) };
    }

    /// The mode this thread's calls see, when a test has named one.
    pub(crate) fn mode() -> Option<WebDomainPolicyMode> {
        MODE.with(Cell::get)
    }

    /// Makes `mode` what this thread's calls see until the guard is dropped.
    pub(crate) fn set(mode: WebDomainPolicyMode) -> Guard {
        Guard(MODE.with(|cell| cell.replace(Some(mode))))
    }

    /// Restores the mode this thread saw before [`set`] named one.
    pub(crate) struct Guard(Option<WebDomainPolicyMode>);

    impl Drop for Guard {
        fn drop(&mut self) {
            MODE.with(|cell| cell.set(self.0));
        }
    }
}

/// The process's checker, so every turn shares one cache.
#[must_use]
pub fn shared_checker() -> &'static SiteChecker {
    static CHECKER: OnceLock<SiteChecker> = OnceLock::new();
    CHECKER.get_or_init(SiteChecker::default)
}

/// Where org-core is, and the service identity the gateway presents to it (`x-service-id`, `x-service-token`).
#[derive(Debug, Clone, Copy)]
pub struct SiteOwner<'a> {
    /// org-core's base address.
    pub base_url: &'a str,
    /// The gateway's service identity in org-core.
    pub service_id: &'a str,
    /// The matching secret. Empty means org-core is not configured, so every host is unreadable.
    pub service_token: &'a str,
}

impl<'a> SiteOwner<'a> {
    /// The owner the gateway's own configuration names.
    #[must_use]
    pub fn of(state: &'a AppState) -> Self {
        Self {
            base_url: &state.org_core_base_url,
            service_id: &state.org_core_service_id,
            service_token: &state.org_core_service_token,
        }
    }
}

/// One host's answer and when org-core gave it.
#[derive(Debug, Clone, Copy)]
struct Entry {
    verdict: HostVerdict,
    fetched: Instant,
}

/// The verdicts one call read. A host with no verdict is one org-core could not answer for, and it is refused.
#[derive(Debug, Default)]
pub struct SiteVerdicts {
    by_host: HashMap<String, HostVerdict>,
}

impl SiteVerdicts {
    /// The verdict org-core gave a canonical host, or `None` when it gave none.
    #[must_use]
    pub fn verdict(&self, host: &str) -> Option<HostVerdict> {
        self.by_host.get(host).copied()
    }

    /// Whether the rules let a fetch of this canonical host through.
    #[must_use]
    pub fn allows_host(&self, host: &str) -> bool {
        self.verdict(host).is_some_and(|verdict| verdict.allowed)
    }

    /// Whether the rules let a fetch of, or a hit on, this URL through. A URL with no host the rules can name is refused.
    #[must_use]
    pub fn allows_url(&self, url: &str) -> bool {
        host_of_url(url).is_some_and(|host| self.allows_host(&host))
    }
}

/// The resolve client: one HTTP client and the answers this process holds, keyed by organisation and canonical host.
/// A refresh replaces an entry whole. The revision rides on each verdict rather than in the key, because a revision
/// is known only from an answer, so it cannot be part of the key that finds one.
#[derive(Debug)]
pub struct SiteChecker {
    http: reqwest::Client,
    entries: DashMap<(String, String), Entry>,
}

impl Default for SiteChecker {
    fn default() -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(RESOLVE_TIMEOUT)
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            entries: DashMap::new(),
        }
    }
}

impl SiteChecker {
    /// The verdicts for `hosts` as of now.
    pub async fn verdicts(&self, owner: &SiteOwner<'_>, org_id: &str, hosts: &[String]) -> SiteVerdicts {
        self.verdicts_at(owner, org_id, hosts, Instant::now()).await
    }

    /// The verdicts for `hosts` as of `now`: each host from the cache while its answer is fresh, and the rest asked of
    /// org-core in batches of [`MAX_RESOLVE_HOSTS`]. A host org-core cannot answer for has no verdict. An unreadable
    /// answer is not cached, so the next call asks again.
    pub async fn verdicts_at(
        &self,
        owner: &SiteOwner<'_>,
        org_id: &str,
        hosts: &[String],
        now: Instant,
    ) -> SiteVerdicts {
        let distinct: BTreeSet<&str> = hosts.iter().map(String::as_str).collect();
        let mut by_host = HashMap::new();
        let mut missing: Vec<&str> = Vec::new();
        for host in distinct {
            match self.fresh(org_id, host, now) {
                Some(verdict) => {
                    by_host.insert(host.to_owned(), verdict);
                }
                None => missing.push(host),
            }
        }
        for batch in missing.chunks(MAX_RESOLVE_HOSTS) {
            let batch: Vec<String> = batch.iter().map(|host| (*host).to_owned()).collect();
            match self.resolve(owner, org_id, &batch).await {
                Ok(resolution) => {
                    for host in batch {
                        let verdict = verdict_for(&host, &resolution);
                        self.entries.insert(
                            (org_id.to_owned(), host.clone()),
                            Entry {
                                verdict,
                                fetched: now,
                            },
                        );
                        by_host.insert(host, verdict);
                    }
                }
                Err(error) => {
                    tracing::warn!(%org_id, hosts = batch.len(), %error, "org web domain policy cannot be read");
                }
            }
        }
        SiteVerdicts { by_host }
    }

    /// The held verdict for a host while its answer is within [`POLICY_TTL`].
    fn fresh(&self, org_id: &str, host: &str, now: Instant) -> Option<HostVerdict> {
        let entry = self.entries.get(&(org_id.to_owned(), host.to_owned()))?;
        (now.saturating_duration_since(entry.fetched) <= POLICY_TTL).then_some(entry.verdict)
    }

    /// One resolve call for at most [`MAX_RESOLVE_HOSTS`] canonical hosts. Any failure is an error, and the caller
    /// treats every host it named as unreadable.
    async fn resolve(
        &self,
        owner: &SiteOwner<'_>,
        org_id: &str,
        hosts: &[String],
    ) -> Result<SiteResolution, String> {
        if !routable_org_id(org_id) {
            return Err("the organisation id does not name a resolve path".to_owned());
        }
        let base = owner.base_url.trim().trim_end_matches('/');
        if base.is_empty() || owner.service_token.trim().is_empty() {
            return Err("org-core is not configured for this service".to_owned());
        }
        let query: Vec<(&str, &str)> = hosts.iter().map(|host| ("host", host.as_str())).collect();
        let response = self
            .http
            .get(format!("{base}/internal/orgs/{org_id}/browser-sites/resolve"))
            .query(&query)
            .header("x-service-id", owner.service_id)
            .header("x-service-token", owner.service_token)
            .send()
            .await
            .map_err(|error| format!("org-core resolve request failed: {error}"))?;
        let status = response.status();
        if !status.is_success() {
            return Err(format!("org-core resolve returned HTTP {status}"));
        }
        let resolution: SiteResolution = response
            .json()
            .await
            .map_err(|error| format!("org-core resolve answer is unreadable: {error}"))?;
        // org-core scopes the read by the caller's organisation, so an answer for another one is a defect: refuse it
        // rather than hold one organisation's rules under another's id.
        if !resolution.organization_id.is_empty() && resolution.organization_id != org_id {
            return Err(format!(
                "org-core answered for organisation '{}', not '{org_id}'",
                resolution.organization_id
            ));
        }
        Ok(resolution)
    }
}

/// Why one URL is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SiteRefusal {
    /// The URL names no host the rules can read, so they cannot be applied to it.
    NoHost,
    /// org-core's answer for the host could not be read.
    Unreadable(String),
    /// The org's rules do not allow the host.
    Blocked(String),
}

impl SiteRefusal {
    fn reason(&self) -> &'static str {
        match self {
            Self::NoHost => "no_host",
            Self::Unreadable(_) => "unreadable",
            Self::Blocked(_) => "blocked",
        }
    }

    fn host(&self) -> Option<&str> {
        match self {
            Self::NoHost => None,
            Self::Unreadable(host) | Self::Blocked(host) => Some(host.as_str()),
        }
    }

    fn message(&self, tool: &str) -> String {
        match self {
            Self::NoHost => format!(
                "{}: '{tool}' cannot be checked against the organisation's site policy, so it is not run",
                SITE_BLOCKED_CODE
            ),
            Self::Unreadable(host) => format!(
                "{}: the organisation's site policy for '{host}' cannot be read right now, so '{tool}' is not run",
                SITE_BLOCKED_CODE
            ),
            Self::Blocked(host) => format!(
                "{}: the organisation does not allow web access to '{host}', so '{tool}' is not run",
                SITE_BLOCKED_CODE
            ),
        }
    }
}

/// The first refusal the rules give one URL, or `None` when they let it through.
fn refusal_for(url: &str, verdicts: &SiteVerdicts) -> Option<SiteRefusal> {
    let Some(host) = host_of_url(url) else {
        return Some(SiteRefusal::NoHost);
    };
    match verdicts.verdict(&host) {
        Some(verdict) if verdict.allowed => None,
        Some(_) => Some(SiteRefusal::Blocked(host)),
        None => Some(SiteRefusal::Unreadable(host)),
    }
}

/// The refusal a web call meets under the switch, when one of its URLs is one the org's rules stop. `None` means the
/// call may proceed. Under `off` nothing is read. Under `shadow` the refusal is logged and `None` is returned. Under
/// `enforce` the refusal's typed message is returned for the model.
pub async fn fetch_refusal(
    mode: WebDomainPolicyMode,
    checker: &SiteChecker,
    owner: &SiteOwner<'_>,
    org_id: &str,
    urls: &[&str],
    tool: &str,
) -> Option<String> {
    if mode == WebDomainPolicyMode::Off {
        return None;
    }
    let hosts: Vec<String> = urls.iter().filter_map(|url| host_of_url(url)).collect();
    let verdicts = checker.verdicts(owner, org_id, &hosts).await;
    let refusal = urls.iter().find_map(|url| refusal_for(url, &verdicts))?;
    tracing::info!(
        %org_id,
        %tool,
        reason = refusal.reason(),
        host = refusal.host().unwrap_or(""),
        enforced = mode == WebDomainPolicyMode::Enforce,
        "org web domain policy refused a web call"
    );
    match mode {
        WebDomainPolicyMode::Enforce => Some(refusal.message(tool)),
        WebDomainPolicyMode::Off | WebDomainPolicyMode::Shadow => None,
    }
}

/// The refusal a web call meets when the page it read ended on a host the org's rules do not allow, for example an
/// allowed address that redirected onto a blocked one. `requested` is the URL the call named (already checked by
/// [`fetch_refusal`]); `final_url` is the address the reader says it ended on. `None` means the page may reach the
/// model: under `off`, when `final_url` is empty, when it has the same canonical host as `requested`, or when the rules
/// allow its host. Otherwise the refusal is logged with the closed fields of [`fetch_refusal`] (no path, no query), and
/// under `enforce` the typed message for the FINAL host is returned. Under `shadow` it is logged and `None` is
/// returned. An answer that cannot be read, or a final URL with no host the rules can name, counts as refused.
pub async fn final_url_refusal(
    mode: WebDomainPolicyMode,
    checker: &SiteChecker,
    owner: &SiteOwner<'_>,
    org_id: &str,
    requested: &str,
    final_url: &str,
    tool: &str,
) -> Option<String> {
    if mode == WebDomainPolicyMode::Off {
        return None;
    }
    let final_url = final_url.trim();
    if final_url.is_empty() {
        return None;
    }
    let final_host = host_of_url(final_url);
    if final_host.is_some() && final_host == host_of_url(requested) {
        return None;
    }
    let hosts: Vec<String> = final_host.into_iter().collect();
    let verdicts = checker.verdicts(owner, org_id, &hosts).await;
    let refusal = refusal_for(final_url, &verdicts)?;
    tracing::info!(
        %org_id,
        %tool,
        reason = refusal.reason(),
        host = refusal.host().unwrap_or(""),
        enforced = mode == WebDomainPolicyMode::Enforce,
        "org web domain policy refused the page a web call ended on"
    );
    match mode {
        WebDomainPolicyMode::Enforce => Some(refusal.message(tool)),
        WebDomainPolicyMode::Off | WebDomainPolicyMode::Shadow => None,
    }
}

/// Drops from the search replies every hit on a host the org's rules do not allow, and returns how many it dropped.
/// Under `shadow` it counts them and drops none. Under `off` nothing is read. A hit with no host the rules can name
/// counts as refused, so an unreadable hit does not reach the model under enforcement.
pub async fn drop_denied_replies(
    mode: WebDomainPolicyMode,
    checker: &SiteChecker,
    owner: &SiteOwner<'_>,
    org_id: &str,
    replies: &mut [SearchReply],
) -> usize {
    if mode == WebDomainPolicyMode::Off {
        return 0;
    }
    let hosts: Vec<String> = replies
        .iter()
        .flat_map(|reply| reply.results.iter())
        .filter_map(|hit| host_of_url(&hit.url))
        .collect();
    let verdicts = checker.verdicts(owner, org_id, &hosts).await;
    let enforced = mode == WebDomainPolicyMode::Enforce;
    let mut refused = 0;
    for reply in replies.iter_mut() {
        if enforced {
            let before = reply.results.len();
            reply.results.retain(|hit| verdicts.allows_url(&hit.url));
            refused += before - reply.results.len();
        } else {
            refused += reply
                .results
                .iter()
                .filter(|hit| !verdicts.allows_url(&hit.url))
                .count();
        }
    }
    if refused > 0 {
        tracing::info!(
            %org_id,
            refused,
            enforced,
            "org web domain policy: search hits on hosts the organisation does not allow"
        );
    }
    refused
}

/// The same drop as [`drop_denied_replies`], over the hits of a gRPC `WebSearch` response, which carry the proto's
/// `WebSearchResult` and not a Quarry `SearchReply`. The verdicts, the switch and the rule that a hit with no host the
/// rules can name counts as refused are the same. Under `shadow` it counts the hits and drops none. Under `off` nothing
/// is read. Only canonical hosts go to org-core, and the log carries a count, never a URL.
pub async fn drop_denied_results_in(
    mode: WebDomainPolicyMode,
    checker: &SiteChecker,
    owner: &SiteOwner<'_>,
    org_id: &str,
    results: &mut Vec<WebSearchResult>,
) -> usize {
    if mode == WebDomainPolicyMode::Off {
        return 0;
    }
    let hosts: Vec<String> = results
        .iter()
        .filter_map(|hit| host_of_url(&hit.url))
        .collect();
    let verdicts = checker.verdicts(owner, org_id, &hosts).await;
    let enforced = mode == WebDomainPolicyMode::Enforce;
    let refused = if enforced {
        let before = results.len();
        results.retain(|hit| verdicts.allows_url(&hit.url));
        before - results.len()
    } else {
        results
            .iter()
            .filter(|hit| !verdicts.allows_url(&hit.url))
            .count()
    };
    if refused > 0 {
        tracing::info!(
            %org_id,
            refused,
            enforced,
            "org web domain policy: search hits on hosts the organisation does not allow"
        );
    }
    refused
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quarry::SearchResult;
    use serde_json::json;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const ORG: &str = "org-a";
    const RESOLVE_PATH: &str = "/internal/orgs/org-a/browser-sites/resolve";

    fn owner(base: &str) -> SiteOwner<'_> {
        SiteOwner {
            base_url: base,
            service_id: "model-gateway",
            service_token: "test-token",
        }
    }

    async fn answer(server: &MockServer, status: u16, body: serde_json::Value) {
        Mock::given(method("GET"))
            .and(path(RESOLVE_PATH))
            .and(header("x-service-id", "model-gateway"))
            .and(header("x-service-token", "test-token"))
            .respond_with(ResponseTemplate::new(status).set_body_json(body))
            .mount(server)
            .await;
    }

    async fn requests(server: &MockServer) -> usize {
        server.received_requests().await.unwrap_or_default().len()
    }

    /// A search hit on `url`, for the filter tests.
    fn hit(url: &str) -> SearchResult {
        SearchResult {
            url: url.to_owned(),
            title: String::new(),
            snippet: String::new(),
            source: "brave".to_owned(),
            score: 0.5,
            relevance: None,
            rank: 1,
            highlights: Vec::new(),
            engines: Vec::new(),
            score_kind: None,
            raw: serde_json::Value::Null,
        }
    }

    #[tokio::test]
    async fn off_reads_nothing_and_refuses_nothing() {
        let server = MockServer::start().await;
        answer(&server, 200, json!({"revision": 1, "decisions": []})).await;
        let base = server.uri();
        let refusal = fetch_refusal(
            WebDomainPolicyMode::Off,
            &SiteChecker::default(),
            &owner(&base),
            ORG,
            &["https://example.org/a"],
            "fetch_url",
        )
        .await;
        assert_eq!(refusal, None);
        assert_eq!(requests(&server).await, 0, "off makes no call to org-core");
    }

    #[tokio::test]
    async fn enforce_refuses_a_blocked_host_by_name_and_lets_an_allowed_host_through() {
        let server = MockServer::start().await;
        answer(
            &server,
            200,
            json!({"organization_id": ORG, "revision": 4, "decisions": [
                {"host": "example.org", "effective": "deny", "cap": "none", "source": "org_block"},
                {"host": "example.com", "effective": "ask", "cap": "act", "source": "default"}
            ]}),
        )
        .await;
        let base = server.uri();
        let checker = SiteChecker::default();
        let refusal = fetch_refusal(
            WebDomainPolicyMode::Enforce,
            &checker,
            &owner(&base),
            ORG,
            &["https://example.org/page"],
            "fetch_url",
        )
        .await
        .expect("a blocked host is refused");
        assert!(refusal.starts_with("org_site_blocked: "), "{refusal}");
        assert!(refusal.contains("'example.org'"), "{refusal}");
        assert!(refusal.contains("'fetch_url'"), "{refusal}");
        let allowed = fetch_refusal(
            WebDomainPolicyMode::Enforce,
            &checker,
            &owner(&base),
            ORG,
            &["https://example.com/"],
            "fetch_url",
        )
        .await;
        assert_eq!(allowed, None);
    }

    #[tokio::test]
    async fn enforce_refuses_a_host_org_core_cannot_answer_for_and_shadow_only_logs_it() {
        let server = MockServer::start().await;
        answer(&server, 503, json!({"error": "down"})).await;
        let base = server.uri();
        let checker = SiteChecker::default();
        let refusal = fetch_refusal(
            WebDomainPolicyMode::Enforce,
            &checker,
            &owner(&base),
            ORG,
            &["https://example.org/"],
            "fetch_url",
        )
        .await
        .expect("an unreadable host is refused");
        assert!(refusal.contains("cannot be read"), "{refusal}");
        let shadow = fetch_refusal(
            WebDomainPolicyMode::Shadow,
            &checker,
            &owner(&base),
            ORG,
            &["https://example.org/"],
            "fetch_url",
        )
        .await;
        assert_eq!(shadow, None, "shadow refuses nothing");
    }

    #[tokio::test]
    async fn shadow_lets_a_blocked_host_through_and_still_reads_the_rules() {
        let server = MockServer::start().await;
        answer(
            &server,
            200,
            json!({"organization_id": ORG, "revision": 9, "decisions": [
                {"host": "example.org", "effective": "deny", "cap": "none", "source": "org_block"}
            ]}),
        )
        .await;
        let base = server.uri();
        let refusal = fetch_refusal(
            WebDomainPolicyMode::Shadow,
            &SiteChecker::default(),
            &owner(&base),
            ORG,
            &["https://example.org/"],
            "fetch_url",
        )
        .await;
        assert_eq!(refusal, None);
        assert_eq!(requests(&server).await, 1, "shadow reads the rules to log them");
    }

    #[tokio::test]
    async fn a_url_with_no_host_is_refused_under_enforce_without_a_request() {
        let server = MockServer::start().await;
        answer(&server, 200, json!({"revision": 1, "decisions": []})).await;
        let base = server.uri();
        let refusal = fetch_refusal(
            WebDomainPolicyMode::Enforce,
            &SiteChecker::default(),
            &owner(&base),
            ORG,
            &["ftp://example.org/file"],
            "fetch_url",
        )
        .await;
        assert!(
            refusal.is_some_and(|message| message.contains("cannot be checked")),
            "a URL with no host the rules can name is refused"
        );
        assert_eq!(requests(&server).await, 0);
    }

    #[tokio::test]
    async fn a_blank_organisation_is_refused_under_enforce_without_a_request() {
        let server = MockServer::start().await;
        answer(&server, 200, json!({"revision": 1, "decisions": []})).await;
        let base = server.uri();
        let refusal = fetch_refusal(
            WebDomainPolicyMode::Enforce,
            &SiteChecker::default(),
            &owner(&base),
            "",
            &["https://example.org/"],
            "fetch_url",
        )
        .await;
        assert!(refusal.is_some_and(|message| message.contains("cannot be read")));
        assert_eq!(requests(&server).await, 0, "an id that names no path is never sent");
    }

    #[tokio::test]
    async fn an_answer_is_served_from_the_cache_for_thirty_seconds() {
        let server = MockServer::start().await;
        answer(
            &server,
            200,
            json!({"organization_id": ORG, "revision": 2, "decisions": [
                {"host": "example.org", "effective": "ask", "cap": "act", "source": "default"}
            ]}),
        )
        .await;
        let base = server.uri();
        let checker = SiteChecker::default();
        let hosts = vec!["example.org".to_owned()];
        let t0 = Instant::now();
        assert!(checker
            .verdicts_at(&owner(&base), ORG, &hosts, t0)
            .await
            .allows_host("example.org"));
        assert!(checker
            .verdicts_at(&owner(&base), ORG, &hosts, t0 + Duration::from_secs(29))
            .await
            .allows_host("example.org"));
        assert_eq!(requests(&server).await, 1, "the second read is served from the cache");
        assert!(checker
            .verdicts_at(&owner(&base), ORG, &hosts, t0 + Duration::from_secs(31))
            .await
            .allows_host("example.org"));
        assert_eq!(requests(&server).await, 2, "a stale answer is asked again");
    }

    #[tokio::test]
    async fn resolve_names_at_most_thirty_two_hosts_per_call() {
        let server = MockServer::start().await;
        answer(&server, 200, json!({"organization_id": ORG, "revision": 1, "decisions": []})).await;
        let base = server.uri();
        let hosts: Vec<String> = (0..33).map(|n| format!("site{n:02}.example.com")).collect();
        SiteChecker::default()
            .verdicts_at(&owner(&base), ORG, &hosts, Instant::now())
            .await;
        let received = server.received_requests().await.unwrap_or_default();
        assert_eq!(received.len(), 2, "33 hosts take two calls");
        let named_in_first = received[0]
            .url
            .query_pairs()
            .filter(|(key, _)| *key == "host")
            .count();
        assert_eq!(named_in_first, MAX_RESOLVE_HOSTS);
    }

    #[tokio::test]
    async fn an_answer_for_another_organisation_is_unreadable() {
        let server = MockServer::start().await;
        answer(
            &server,
            200,
            json!({"organization_id": "org-b", "revision": 1, "decisions": [
                {"host": "example.org", "effective": "ask", "cap": "act", "source": "default"}
            ]}),
        )
        .await;
        let base = server.uri();
        let verdicts = SiteChecker::default()
            .verdicts(&owner(&base), ORG, &["example.org".to_owned()])
            .await;
        assert_eq!(verdicts.verdict("example.org"), None);
    }

    #[tokio::test]
    async fn search_hits_on_refused_hosts_are_dropped_under_enforce_only() {
        let server = MockServer::start().await;
        answer(
            &server,
            200,
            json!({"organization_id": ORG, "revision": 5, "decisions": [
                {"host": "example.org", "effective": "deny", "cap": "none", "source": "org_block"},
                {"host": "example.com", "effective": "read", "cap": "read", "source": "org_cap"}
            ]}),
        )
        .await;
        let base = server.uri();
        let reply = || SearchReply {
            results: vec![
                hit("https://example.org/a"),
                hit("https://example.com/b"),
                hit("ftp://example.com/c"),
            ],
            rerank: None,
            provider: "brave".to_owned(),
        };
        let checker = SiteChecker::default();

        let mut shadow = [reply()];
        let counted = drop_denied_replies(WebDomainPolicyMode::Shadow, &checker, &owner(&base), ORG, &mut shadow).await;
        assert_eq!(counted, 2, "shadow counts the blocked host and the host-less hit");
        assert_eq!(shadow[0].results.len(), 3, "shadow drops nothing");

        let mut enforced = [reply()];
        let dropped = drop_denied_replies(WebDomainPolicyMode::Enforce, &checker, &owner(&base), ORG, &mut enforced).await;
        assert_eq!(dropped, 2);
        let urls: Vec<&str> = enforced[0].results.iter().map(|hit| hit.url.as_str()).collect();
        assert_eq!(urls, ["https://example.com/b"], "only the allowed host's hit remains");
    }

    #[tokio::test]
    async fn off_drops_no_hit_and_reads_nothing() {
        let server = MockServer::start().await;
        answer(&server, 200, json!({"revision": 1, "decisions": []})).await;
        let base = server.uri();
        let mut replies = [SearchReply {
            results: vec![hit("https://example.org/a")],
            rerank: None,
            provider: "brave".to_owned(),
        }];
        let dropped = drop_denied_replies(WebDomainPolicyMode::Off, &SiteChecker::default(), &owner(&base), ORG, &mut replies).await;
        assert_eq!(dropped, 0);
        assert_eq!(replies[0].results.len(), 1);
        assert_eq!(requests(&server).await, 0);
    }
}
