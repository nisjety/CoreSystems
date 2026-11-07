//! The org's domain rules as the execution core's web tools enforce them (Wave 3 EP-9 W3).
//!
//! org-core owns the rules (DEC-19). Before `web_fetch`, `web_search` or one of the page-acquisition tools (`knowledge.map`,
//! `knowledge.crawl_site`, `knowledge.batch`) reaches a host, this module asks org-core whether the rules allow it, and
//! keeps each answer for [`POLICY_TTL`]. The pure rules (the canonical host, the reader of a URL, the verdict from an
//! answer, the switch) live in `mp_contracts::site_policy`. This crate keeps its own resolve client, because the two
//! loops stay separate; it reads its owner from this service's environment: `ORG_CORE_URL`, `EXECUTION_CORE_SERVICE_ID`
//! and `EXECUTION_ORG_CORE_SERVICE_TOKEN`.
//!
//! `WEB_DOMAIN_POLICY_MODE` decides how far this binds. Under `off` nothing is read and the request is exactly what it was.
//! Under `shadow` the same refusals are logged and nothing is refused. Under `enforce` a host whose answer cannot be read is
//! refused, and so is a URL with no host the rules can name. A check sends org-core canonical hosts only.

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use mp_contracts::site_policy::{
    host_of_url, routable_org_id, verdict_for, HostVerdict, SiteResolution, WebDomainPolicyMode,
    MAX_RESOLVE_HOSTS, WEB_DOMAIN_POLICY_MODE_ENV,
};
use serde_json::Value;

const DEFAULT_ORG_CORE_URL: &str = "http://org-core:8080";
const DEFAULT_SERVICE_ID: &str = "execution-core";

/// How long one host's answer is served before org-core is asked again.
pub const POLICY_TTL: Duration = Duration::from_secs(30);

/// The longest a resolve call may take. An answer that comes later is an unreadable one.
const RESOLVE_TIMEOUT: Duration = Duration::from_secs(5);

/// The code of a web call the org's domain rules stop. It is a typed tool error: the model reads the code and the host, and
/// the words after it are copy.
pub const SITE_BLOCKED_CODE: &str = "org_site_blocked";

/// The process's switch, read once so it cannot change under a request.
#[must_use]
pub fn policy_mode() -> WebDomainPolicyMode {
    static MODE: OnceLock<WebDomainPolicyMode> = OnceLock::new();
    *MODE.get_or_init(|| {
        WebDomainPolicyMode::from_env_value(std::env::var(WEB_DOMAIN_POLICY_MODE_ENV).ok().as_deref())
    })
}

/// The process's checker, so every run shares one cache.
fn shared_checker() -> Arc<SiteChecker> {
    static CHECKER: OnceLock<Arc<SiteChecker>> = OnceLock::new();
    Arc::clone(CHECKER.get_or_init(|| Arc::new(SiteChecker::default())))
}

/// Where org-core is, and the service identity this service presents to it.
#[derive(Debug, Clone)]
pub struct SiteOwner {
    base_url: String,
    service_id: String,
    service_token: String,
}

impl SiteOwner {
    /// The owner this service's environment names. An unset token is kept as empty, which leaves every host unreadable.
    #[must_use]
    pub fn from_env() -> Self {
        let base_url = std::env::var("ORG_CORE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_ORG_CORE_URL.to_owned());
        let service_id = std::env::var("EXECUTION_CORE_SERVICE_ID")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_SERVICE_ID.to_owned());
        let service_token = std::env::var("EXECUTION_ORG_CORE_SERVICE_TOKEN").unwrap_or_default();
        Self {
            base_url,
            service_id,
            service_token,
        }
    }

    /// An owner with the given address and identity.
    #[cfg(test)]
    #[must_use]
    pub fn new(base_url: &str, service_id: &str, service_token: &str) -> Self {
        Self {
            base_url: base_url.to_owned(),
            service_id: service_id.to_owned(),
            service_token: service_token.to_owned(),
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

/// The resolve client: one HTTP client and the answers this process holds, keyed by organisation and canonical host. A
/// refresh replaces an entry whole. The revision rides on each verdict rather than in the key, because a revision is known
/// only from an answer, so it cannot be part of the key that finds one.
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
    pub async fn verdicts(&self, owner: &SiteOwner, org_id: &str, hosts: &[String]) -> SiteVerdicts {
        self.verdicts_at(owner, org_id, hosts, Instant::now()).await
    }

    /// The verdicts for `hosts` as of `now`: each host from the cache while its answer is fresh, and the rest asked of org-core
    /// in batches of [`MAX_RESOLVE_HOSTS`]. A host org-core cannot answer for has no verdict. An unreadable answer is not
    /// cached, so the next call asks again.
    pub async fn verdicts_at(
        &self,
        owner: &SiteOwner,
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

    /// One resolve call for at most [`MAX_RESOLVE_HOSTS`] canonical hosts. Any failure is an error, and the caller treats
    /// every host it named as unreadable.
    async fn resolve(
        &self,
        owner: &SiteOwner,
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
            .header("x-service-id", owner.service_id.as_str())
            .header("x-service-token", owner.service_token.as_str())
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
        // org-core scopes the read by the caller's organisation, so an answer for another one is a defect: refuse it rather than
        // hold one organisation's rules under another's id.
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

/// The refusal a web call meets under the switch, when one of its URLs is one the org's rules stop. `None` means the call may
/// proceed. Under `off` nothing is read. Under `shadow` the refusal is logged and `None` is returned. Under `enforce` the
/// refusal's typed message is returned for the model.
pub async fn fetch_refusal(
    mode: WebDomainPolicyMode,
    checker: &SiteChecker,
    owner: &SiteOwner,
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

fn hit_url(hit: &Value) -> Option<&str> {
    hit.get("url").and_then(Value::as_str)
}

/// Drops from a search's result list every hit on a host the org's rules do not allow, and returns how many it dropped. Under
/// `shadow` it counts them and drops none. Under `off` nothing is read. A hit with no host the rules can name counts as refused.
pub async fn drop_denied_results(
    mode: WebDomainPolicyMode,
    checker: &SiteChecker,
    owner: &SiteOwner,
    org_id: &str,
    results: &mut Vec<Value>,
) -> usize {
    if mode == WebDomainPolicyMode::Off {
        return 0;
    }
    let hosts: Vec<String> = results
        .iter()
        .filter_map(hit_url)
        .filter_map(host_of_url)
        .collect();
    let verdicts = checker.verdicts(owner, org_id, &hosts).await;
    let allowed = |hit: &Value| hit_url(hit).is_some_and(|url| verdicts.allows_url(url));
    let enforced = mode == WebDomainPolicyMode::Enforce;
    let refused = results.iter().filter(|hit| !allowed(*hit)).count();
    if enforced {
        results.retain(|hit| allowed(hit));
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

/// What one web call checks against the org's rules: the switch, the owner and the shared checker. Production builds it from
/// the environment; a test builds one with the switch and owner it needs.
#[derive(Clone)]
pub struct SitePolicy {
    mode: WebDomainPolicyMode,
    owner: SiteOwner,
    checker: Arc<SiteChecker>,
}

impl SitePolicy {
    /// The policy this process applies, with the owner read from its environment now.
    #[must_use]
    pub fn from_env() -> Self {
        Self {
            mode: policy_mode(),
            owner: SiteOwner::from_env(),
            checker: shared_checker(),
        }
    }

    /// A policy that checks nothing: the switch is off, so nothing is read and nothing changes.
    #[cfg(test)]
    #[must_use]
    pub fn disabled() -> Self {
        Self {
            mode: WebDomainPolicyMode::Off,
            owner: SiteOwner::new("", "", ""),
            checker: shared_checker(),
        }
    }

    /// A policy with the given switch, owner and checker.
    #[cfg(test)]
    #[must_use]
    pub fn new(mode: WebDomainPolicyMode, owner: SiteOwner, checker: Arc<SiteChecker>) -> Self {
        Self {
            mode,
            owner,
            checker,
        }
    }

    /// The refusal a web call over these URLs meets under the switch, or `None` when it may proceed.
    pub async fn fetch_refusal(&self, org_id: &str, urls: &[&str], tool: &str) -> Option<String> {
        fetch_refusal(self.mode, &self.checker, &self.owner, org_id, urls, tool).await
    }

    /// Drops the search hits on refused hosts under the switch, and returns how many were refused.
    pub async fn drop_denied_results(&self, org_id: &str, results: &mut Vec<Value>) -> usize {
        drop_denied_results(self.mode, &self.checker, &self.owner, org_id, results).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const ORG: &str = "org-a";
    const RESOLVE_PATH: &str = "/internal/orgs/org-a/browser-sites/resolve";

    fn owner(base: &str) -> SiteOwner {
        SiteOwner::new(base, "execution-core", "test-token")
    }

    async fn answer(server: &MockServer, status: u16, body: Value) {
        Mock::given(method("GET"))
            .and(path(RESOLVE_PATH))
            .and(header("x-service-id", "execution-core"))
            .and(header("x-service-token", "test-token"))
            .respond_with(ResponseTemplate::new(status).set_body_json(body))
            .mount(server)
            .await;
    }

    async fn requests(server: &MockServer) -> usize {
        server.received_requests().await.unwrap_or_default().len()
    }

    #[tokio::test]
    async fn off_reads_nothing_and_refuses_nothing() {
        let server = MockServer::start().await;
        answer(&server, 200, json!({"revision": 1, "decisions": []})).await;
        let base = server.uri();
        let policy = SitePolicy::new(
            WebDomainPolicyMode::Off,
            owner(&base),
            Arc::new(SiteChecker::default()),
        );
        assert_eq!(
            policy
                .fetch_refusal(ORG, &["https://example.org/a"], "web_fetch")
                .await,
            None
        );
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
        let policy = SitePolicy::new(
            WebDomainPolicyMode::Enforce,
            owner(&base),
            Arc::new(SiteChecker::default()),
        );
        let refusal = policy
            .fetch_refusal(ORG, &["https://example.org/page"], "web_fetch")
            .await
            .expect("a blocked host is refused");
        assert!(refusal.starts_with("org_site_blocked: "), "{refusal}");
        assert!(refusal.contains("'example.org'"), "{refusal}");
        assert!(refusal.contains("'web_fetch'"), "{refusal}");
        assert_eq!(
            policy
                .fetch_refusal(ORG, &["https://example.com/"], "web_fetch")
                .await,
            None
        );
    }

    #[tokio::test]
    async fn enforce_refuses_a_host_org_core_cannot_answer_for_and_shadow_only_logs_it() {
        let server = MockServer::start().await;
        answer(&server, 503, json!({"error": "down"})).await;
        let base = server.uri();
        let checker = Arc::new(SiteChecker::default());
        let enforce = SitePolicy::new(WebDomainPolicyMode::Enforce, owner(&base), Arc::clone(&checker));
        let refusal = enforce
            .fetch_refusal(ORG, &["https://example.org/"], "web_fetch")
            .await
            .expect("an unreadable host is refused");
        assert!(refusal.contains("cannot be read"), "{refusal}");
        let shadow = SitePolicy::new(WebDomainPolicyMode::Shadow, owner(&base), checker);
        assert_eq!(
            shadow
                .fetch_refusal(ORG, &["https://example.org/"], "web_fetch")
                .await,
            None,
            "shadow refuses nothing"
        );
    }

    #[tokio::test]
    async fn a_url_with_no_host_is_refused_under_enforce_without_a_request() {
        let server = MockServer::start().await;
        answer(&server, 200, json!({"revision": 1, "decisions": []})).await;
        let base = server.uri();
        let policy = SitePolicy::new(
            WebDomainPolicyMode::Enforce,
            owner(&base),
            Arc::new(SiteChecker::default()),
        );
        let refusal = policy
            .fetch_refusal(ORG, &["https://user@example.org/"], "web_fetch")
            .await;
        assert!(refusal.is_some_and(|message| message.contains("cannot be checked")));
        assert_eq!(requests(&server).await, 0);
    }

    #[tokio::test]
    async fn an_unset_token_leaves_every_host_unreadable_under_enforce() {
        let server = MockServer::start().await;
        answer(&server, 200, json!({"revision": 1, "decisions": []})).await;
        let base = server.uri();
        let policy = SitePolicy::new(
            WebDomainPolicyMode::Enforce,
            SiteOwner::new(&base, "execution-core", ""),
            Arc::new(SiteChecker::default()),
        );
        let refusal = policy
            .fetch_refusal(ORG, &["https://example.org/"], "web_fetch")
            .await;
        assert!(refusal.is_some_and(|message| message.contains("cannot be read")));
        assert_eq!(requests(&server).await, 0, "no token, no call");
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
    async fn search_results_on_refused_hosts_are_dropped_under_enforce_only() {
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
        let results = || {
            vec![
                json!({"url": "https://example.org/a", "title": "A"}),
                json!({"url": "https://example.com/b", "title": "B"}),
                json!({"url": "ftp://example.com/c", "title": "C"}),
            ]
        };
        let checker = Arc::new(SiteChecker::default());

        let shadow = SitePolicy::new(WebDomainPolicyMode::Shadow, owner(&base), Arc::clone(&checker));
        let mut kept = results();
        assert_eq!(shadow.drop_denied_results(ORG, &mut kept).await, 2);
        assert_eq!(kept.len(), 3, "shadow drops nothing");

        let enforce = SitePolicy::new(WebDomainPolicyMode::Enforce, owner(&base), checker);
        let mut enforced = results();
        assert_eq!(enforce.drop_denied_results(ORG, &mut enforced).await, 2);
        let urls: Vec<&str> = enforced.iter().filter_map(hit_url).collect();
        assert_eq!(urls, ["https://example.com/b"], "only the allowed host's hit remains");
    }

    #[tokio::test]
    async fn off_drops_no_result_and_reads_nothing() {
        let server = MockServer::start().await;
        answer(&server, 200, json!({"revision": 1, "decisions": []})).await;
        let base = server.uri();
        let policy = SitePolicy::new(
            WebDomainPolicyMode::Off,
            owner(&base),
            Arc::new(SiteChecker::default()),
        );
        let mut results = vec![json!({"url": "https://example.org/a", "title": "A"})];
        assert_eq!(policy.drop_denied_results(ORG, &mut results).await, 0);
        assert_eq!(results.len(), 1);
        assert_eq!(requests(&server).await, 0);
    }

    #[test]
    fn the_switch_off_policy_checks_nothing() {
        let policy = SitePolicy::disabled();
        assert_eq!(policy.mode, WebDomainPolicyMode::Off);
    }
}
