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
//! refused, and so is a URL with no host the rules can name. A check sends org-core canonical hosts only. The host a page
//! ended on is checked the same way (`final_url_refusal`). When the extract reply names no final URL, the requested host
//! is checked in its place.

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

/// The environment name of the Space floor switch (F-f03-03). The browser broker reads the same name.
const SPACE_FLOOR_ENV: &str = "BROWSER_SPACE_FLOOR_ENABLED";

/// The Space floor switch, read once so it cannot change under a request. Only the literal `true` turns it on. On, a check
/// that names a Space sends that Space to org-core; off, every check is the organisation's own, as it was before.
#[must_use]
pub fn space_floor_enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| space_floor_enabled_from(std::env::var(SPACE_FLOOR_ENV).ok().as_deref()))
}

/// The Space floor switch's value: the exact literal `true` is on, and anything else (blank, `1`, `on`, a padded `true`) is off.
#[must_use]
pub fn space_floor_enabled_from(value: Option<&str>) -> bool {
    value == Some("true")
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

/// The resolve client: one HTTP client and the answers this process holds, keyed by organisation, Space and canonical host
/// (the empty Space is the organisation's own answer). A refresh replaces an entry whole. The revision rides on each verdict
/// rather than in the key, because a revision is known only from an answer, so it cannot be part of the key that finds one.
#[derive(Debug)]
pub struct SiteChecker {
    http: reqwest::Client,
    entries: DashMap<(String, String, String), Entry>,
    /// BROWSER_SPACE_FLOOR_ENABLED, read once when the checker is built. Off, a Space is never named to org-core.
    space_floor: bool,
}

impl Default for SiteChecker {
    fn default() -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(RESOLVE_TIMEOUT)
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            entries: DashMap::new(),
            space_floor: space_floor_enabled(),
        }
    }
}

impl SiteChecker {
    /// The verdicts for `hosts` as of now.
    pub async fn verdicts(&self, owner: &SiteOwner, org_id: &str, hosts: &[String]) -> SiteVerdicts {
        self.verdicts_at(owner, org_id, hosts, Instant::now()).await
    }

    /// The verdicts for `hosts` in `space_id` as of now (F-f03-03). An empty `space_id` gives the organisation's own
    /// verdicts, as [`SiteChecker::verdicts`] does. A non-empty one is named to org-core only while the Space floor switch is on.
    pub async fn verdicts_in_space(
        &self,
        owner: &SiteOwner,
        org_id: &str,
        space_id: &str,
        hosts: &[String],
    ) -> SiteVerdicts {
        self.verdicts_in_space_at(owner, org_id, space_id, hosts, Instant::now())
            .await
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
        self.verdicts_in_space_at(owner, org_id, "", hosts, now)
            .await
    }

    /// [`SiteChecker::verdicts_at`] for one Space. The switch decides whether the Space is used at all: off, the call is the
    /// organisation's own, with its own cache entries, exactly as before the Space floor. A blank Space is no Space.
    async fn verdicts_in_space_at(
        &self,
        owner: &SiteOwner,
        org_id: &str,
        space_id: &str,
        hosts: &[String],
        now: Instant,
    ) -> SiteVerdicts {
        let space = if self.space_floor {
            space_id.trim()
        } else {
            ""
        };
        let distinct: BTreeSet<&str> = hosts.iter().map(String::as_str).collect();
        let mut by_host = HashMap::new();
        let mut missing: Vec<&str> = Vec::new();
        for host in distinct {
            match self.fresh(org_id, space, host, now) {
                Some(verdict) => {
                    by_host.insert(host.to_owned(), verdict);
                }
                None => missing.push(host),
            }
        }
        for batch in missing.chunks(MAX_RESOLVE_HOSTS) {
            let batch: Vec<String> = batch.iter().map(|host| (*host).to_owned()).collect();
            match self.resolve(owner, org_id, space, &batch).await {
                Ok(resolution) => {
                    for host in batch {
                        let verdict = verdict_for(&host, &resolution);
                        self.entries.insert(
                            (org_id.to_owned(), space.to_owned(), host.clone()),
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

    /// The held verdict for a host in a Space while its answer is within [`POLICY_TTL`].
    fn fresh(&self, org_id: &str, space: &str, host: &str, now: Instant) -> Option<HostVerdict> {
        let entry = self
            .entries
            .get(&(org_id.to_owned(), space.to_owned(), host.to_owned()))?;
        (now.saturating_duration_since(entry.fetched) <= POLICY_TTL).then_some(entry.verdict)
    }

    /// One resolve call for at most [`MAX_RESOLVE_HOSTS`] canonical hosts. Any failure is an error, and the caller treats
    /// every host it named as unreadable. The Space is named in the query only when it is non-empty and the switch is on
    /// (F-f03-03); the caller has already left it empty when the switch is off.
    async fn resolve(
        &self,
        owner: &SiteOwner,
        org_id: &str,
        space_id: &str,
        hosts: &[String],
    ) -> Result<SiteResolution, String> {
        if !routable_org_id(org_id) {
            return Err("the organisation id does not name a resolve path".to_owned());
        }
        let base = owner.base_url.trim().trim_end_matches('/');
        if base.is_empty() || owner.service_token.trim().is_empty() {
            return Err("org-core is not configured for this service".to_owned());
        }
        let mut query: Vec<(&str, &str)> =
            hosts.iter().map(|host| ("host", host.as_str())).collect();
        if self.space_floor && !space_id.is_empty() {
            query.push(("space_id", space_id));
        }
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
/// refusal's typed message is returned for the model. `space_id` is the run's Space, empty outside one (F-f03-03).
pub async fn fetch_refusal(
    mode: WebDomainPolicyMode,
    checker: &SiteChecker,
    owner: &SiteOwner,
    org_id: &str,
    space_id: &str,
    urls: &[&str],
    tool: &str,
) -> Option<String> {
    if mode == WebDomainPolicyMode::Off {
        return None;
    }
    let hosts: Vec<String> = urls.iter().filter_map(|url| host_of_url(url)).collect();
    let verdicts = checker
        .verdicts_in_space(owner, org_id, space_id, &hosts)
        .await;
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

/// The refusal a fetch meets when the page it read ended on a host the org's rules do not allow, for example an allowed
/// address that redirected onto a blocked one. `requested` is the URL the call named, which [`fetch_refusal`] has already
/// checked; `final_url` is the address the extract reply says the page ended on. Under `off` nothing is read. A final URL
/// on the requested URL's canonical host passes without a call to org-core. Otherwise the refusal is logged with the
/// closed fields of [`fetch_refusal`] (no path, no query), and under `enforce` the typed message for the FINAL host is
/// returned. Under `shadow` it is logged and `None` is returned. An answer that cannot be read, or a final URL with no
/// host the rules can name, counts as refused.
///
/// An empty or blank `final_url` is no final URL: it is unknown, and it is judged as the `requested` URL. The rules judge
/// the requested URL's host in its place, with the same fail-closed reads. Under `enforce` a host the rules deny, an answer
/// that cannot be read, or a `requested` URL with no host refuses the page. Under `shadow` the same judgement is made and
/// logged, and nothing is refused. `space_id` is the run's Space, empty outside one: the judged host is read for it (F-f03-03).
#[allow(clippy::too_many_arguments)]
pub async fn final_url_refusal(
    mode: WebDomainPolicyMode,
    checker: &SiteChecker,
    owner: &SiteOwner,
    org_id: &str,
    space_id: &str,
    requested: &str,
    final_url: &str,
    tool: &str,
) -> Option<String> {
    if mode == WebDomainPolicyMode::Off {
        return None;
    }
    let final_url = final_url.trim();
    let unknown = final_url.is_empty();
    // An unknown final URL is judged as the requested URL. The same-host pass below does not apply to it, because that
    // pass lets the requested host through without a read.
    let judged = if unknown { requested.trim() } else { final_url };
    let judged_host = host_of_url(judged);
    if !unknown && judged_host.is_some() && judged_host == host_of_url(requested) {
        return None;
    }
    let hosts: Vec<String> = judged_host.into_iter().collect();
    let verdicts = checker
        .verdicts_in_space(owner, org_id, space_id, &hosts)
        .await;
    let refusal = refusal_for(judged, &verdicts)?;
    FinalUrlLog::new(&refusal, mode, !unknown).emit(org_id, tool);
    match mode {
        WebDomainPolicyMode::Enforce => Some(refusal.message(tool)),
        WebDomainPolicyMode::Off | WebDomainPolicyMode::Shadow => None,
    }
}

/// The closed fields a refused final URL is logged with: the reason, the judged host, whether the refusal is enforced and
/// whether the page's final URL was known. It carries no path, no query and no page content.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FinalUrlLog {
    reason: &'static str,
    host: String,
    enforced: bool,
    final_known: bool,
}

#[cfg(test)]
thread_local! {
    /// Every final-URL log line this thread emitted, so a test reads what was logged without a tracing subscriber.
    static FINAL_URL_LOGS: std::cell::RefCell<Vec<FinalUrlLog>> = const { std::cell::RefCell::new(Vec::new()) };
}

impl FinalUrlLog {
    fn new(refusal: &SiteRefusal, mode: WebDomainPolicyMode, final_known: bool) -> Self {
        Self {
            reason: refusal.reason(),
            host: refusal.host().unwrap_or("").to_owned(),
            enforced: mode == WebDomainPolicyMode::Enforce,
            final_known,
        }
    }

    fn emit(self, org_id: &str, tool: &str) {
        tracing::info!(
            %org_id,
            %tool,
            reason = self.reason,
            host = self.host.as_str(),
            enforced = self.enforced,
            final_known = self.final_known,
            "org web domain policy refused the page a web call ended on"
        );
        #[cfg(test)]
        FINAL_URL_LOGS.with(|logs| logs.borrow_mut().push(self));
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
    space_id: &str,
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
    let verdicts = checker
        .verdicts_in_space(owner, org_id, space_id, &hosts)
        .await;
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

    /// The refusal a web call over these URLs meets under the switch, or `None` when it may proceed. This is the
    /// organisation's own answer: [`SitePolicy::fetch_refusal_in_space`] with no Space.
    pub async fn fetch_refusal(&self, org_id: &str, urls: &[&str], tool: &str) -> Option<String> {
        self.fetch_refusal_in_space(org_id, "", urls, tool).await
    }

    /// [`SitePolicy::fetch_refusal`] for a call made in `space_id` (F-f03-03). A Space is named to org-core only while the
    /// Space floor switch is on; an empty one is the organisation's own answer.
    pub async fn fetch_refusal_in_space(
        &self,
        org_id: &str,
        space_id: &str,
        urls: &[&str],
        tool: &str,
    ) -> Option<String> {
        fetch_refusal(
            self.mode,
            &self.checker,
            &self.owner,
            org_id,
            space_id,
            urls,
            tool,
        )
        .await
    }

    /// The refusal a fetch meets when the page it read ended on a host the org's rules do not allow, under the switch.
    /// This is the organisation's own answer: [`SitePolicy::final_url_refusal_in_space`] with no Space.
    pub async fn final_url_refusal(
        &self,
        org_id: &str,
        requested: &str,
        final_url: &str,
        tool: &str,
    ) -> Option<String> {
        self.final_url_refusal_in_space(org_id, "", requested, final_url, tool)
            .await
    }

    /// [`SitePolicy::final_url_refusal`] for a fetch made in `space_id` (F-f03-03).
    pub async fn final_url_refusal_in_space(
        &self,
        org_id: &str,
        space_id: &str,
        requested: &str,
        final_url: &str,
        tool: &str,
    ) -> Option<String> {
        final_url_refusal(
            self.mode,
            &self.checker,
            &self.owner,
            org_id,
            space_id,
            requested,
            final_url,
            tool,
        )
        .await
    }

    /// Drops the search hits on refused hosts under the switch, and returns how many were refused. This is the
    /// organisation's own answer: [`SitePolicy::drop_denied_results_in_space`] with no Space.
    pub async fn drop_denied_results(&self, org_id: &str, results: &mut Vec<Value>) -> usize {
        self.drop_denied_results_in_space(org_id, "", results).await
    }

    /// [`SitePolicy::drop_denied_results`] for a search made in `space_id` (F-f03-03).
    pub async fn drop_denied_results_in_space(
        &self,
        org_id: &str,
        space_id: &str,
        results: &mut Vec<Value>,
    ) -> usize {
        drop_denied_results(
            self.mode,
            &self.checker,
            &self.owner,
            org_id,
            space_id,
            results,
        )
        .await
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

    /// org-core's answer for one allowed host (`example.com`) and one denied host (`blocked.example`).
    async fn allowed_and_blocked_hosts(server: &MockServer) {
        answer(
            server,
            200,
            json!({"organization_id": ORG, "revision": 6, "decisions": [
                {"host": "example.com", "effective": "ask", "cap": "act", "source": "default"},
                {"host": "blocked.example", "effective": "deny", "cap": "none", "source": "org_block"}
            ]}),
        )
        .await;
    }

    /// Takes the final-URL log lines this test's thread emitted. A `#[tokio::test]` runs on its own thread's
    /// current-thread runtime, so no other test's lines are in it.
    fn take_final_url_logs() -> Vec<FinalUrlLog> {
        FINAL_URL_LOGS.with(|logs| std::mem::take(&mut *logs.borrow_mut()))
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

    #[tokio::test]
    async fn a_redirect_onto_a_blocked_host_is_refused_under_enforce_and_logged_under_shadow() {
        let server = MockServer::start().await;
        answer(
            &server,
            200,
            json!({"organization_id": ORG, "revision": 6, "decisions": [
                {"host": "example.com", "effective": "ask", "cap": "act", "source": "default"},
                {"host": "blocked.example", "effective": "deny", "cap": "none", "source": "org_block"}
            ]}),
        )
        .await;
        let base = server.uri();
        let enforce = SitePolicy::new(
            WebDomainPolicyMode::Enforce,
            owner(&base),
            Arc::new(SiteChecker::default()),
        );
        let refusal = enforce
            .final_url_refusal(
                ORG,
                "https://example.com/start",
                "https://blocked.example/landing?q=private-query",
                "web_fetch",
            )
            .await
            .expect("a redirect onto a blocked host is refused");
        assert!(refusal.starts_with("org_site_blocked: "), "{refusal}");
        assert!(
            refusal.contains("'blocked.example'"),
            "names the FINAL host: {refusal}"
        );
        assert!(refusal.contains("'web_fetch'"), "{refusal}");
        assert!(
            !refusal.contains("/landing") && !refusal.contains("private-query"),
            "no path or query in the refusal: {refusal}"
        );

        let before = requests(&server).await;
        let shadow = SitePolicy::new(
            WebDomainPolicyMode::Shadow,
            owner(&base),
            Arc::new(SiteChecker::default()),
        );
        assert_eq!(
            shadow
                .final_url_refusal(
                    ORG,
                    "https://example.com/start",
                    "https://blocked.example/landing",
                    "web_fetch",
                )
                .await,
            None,
            "shadow refuses nothing"
        );
        assert!(
            requests(&server).await > before,
            "shadow still asks org-core"
        );
    }

    #[tokio::test]
    async fn off_and_a_same_host_final_url_ask_org_core_nothing() {
        let server = MockServer::start().await;
        allowed_and_blocked_hosts(&server).await;
        let base = server.uri();
        for (mode, requested, final_url) in [
            (
                WebDomainPolicyMode::Off,
                "https://example.com/start",
                "https://blocked.example/landing",
            ),
            (
                WebDomainPolicyMode::Off,
                "https://blocked.example/start",
                "",
            ),
            (
                WebDomainPolicyMode::Enforce,
                "https://example.com/start",
                "https://EXAMPLE.com/other",
            ),
        ] {
            let policy = SitePolicy::new(mode, owner(&base), Arc::new(SiteChecker::default()));
            assert_eq!(
                policy
                    .final_url_refusal(ORG, requested, final_url, "web_fetch")
                    .await,
                None,
                "{mode:?} {requested} -> {final_url:?}"
            );
        }
        assert_eq!(requests(&server).await, 0, "none of these needs org-core");
    }

    #[tokio::test]
    async fn a_blank_final_url_judges_the_requested_host_under_enforce() {
        let server = MockServer::start().await;
        allowed_and_blocked_hosts(&server).await;
        let base = server.uri();
        for final_url in ["", "   ", "\t\n"] {
            let before = requests(&server).await;
            let policy = SitePolicy::new(
                WebDomainPolicyMode::Enforce,
                owner(&base),
                Arc::new(SiteChecker::default()),
            );
            assert_eq!(
                policy
                    .final_url_refusal(ORG, "https://example.com/start", final_url, "web_fetch")
                    .await,
                None,
                "an allowed requested host passes: {final_url:?}"
            );
            assert_eq!(
                requests(&server).await,
                before + 1,
                "the requested host is read once: {final_url:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_blank_final_url_on_a_denied_requested_host_is_refused_under_enforce() {
        let server = MockServer::start().await;
        allowed_and_blocked_hosts(&server).await;
        let base = server.uri();
        for final_url in ["", "  "] {
            let policy = SitePolicy::new(
                WebDomainPolicyMode::Enforce,
                owner(&base),
                Arc::new(SiteChecker::default()),
            );
            let refusal = policy
                .final_url_refusal(
                    ORG,
                    "https://blocked.example/start?q=secret",
                    final_url,
                    "web_fetch",
                )
                .await
                .expect("the requested host is denied, so the page is refused");
            assert!(refusal.starts_with("org_site_blocked: "), "{refusal}");
            assert!(
                refusal.contains("'blocked.example'"),
                "names the requested host: {refusal}"
            );
            assert!(refusal.contains("'web_fetch'"), "{refusal}");
            assert!(
                !refusal.contains("secret"),
                "no query in the refusal: {refusal}"
            );
        }
    }

    #[tokio::test]
    async fn a_blank_final_url_is_refused_when_org_core_cannot_answer() {
        let server = MockServer::start().await;
        answer(&server, 503, json!({"error": "down"})).await;
        let base = server.uri();
        let policy = SitePolicy::new(
            WebDomainPolicyMode::Enforce,
            owner(&base),
            Arc::new(SiteChecker::default()),
        );
        let refusal = policy
            .final_url_refusal(ORG, "https://example.org/a", "", "web_fetch")
            .await
            .expect("an answer that cannot be read refuses the page (fail closed)");
        assert!(refusal.starts_with("org_site_blocked: "), "{refusal}");
        assert!(refusal.contains("cannot be read"), "{refusal}");
        assert_eq!(
            requests(&server).await,
            1,
            "the requested host is asked once"
        );

        let unset = SitePolicy::new(
            WebDomainPolicyMode::Enforce,
            SiteOwner::new(&base, "execution-core", ""),
            Arc::new(SiteChecker::default()),
        );
        assert!(unset
            .final_url_refusal(ORG, "https://example.org/a", "", "web_fetch")
            .await
            .is_some_and(|message| message.contains("cannot be read")));
        assert_eq!(requests(&server).await, 1, "no token, so no second call");
    }

    #[tokio::test]
    async fn a_blank_final_url_for_a_requested_url_with_no_host_is_refused_under_enforce() {
        let server = MockServer::start().await;
        allowed_and_blocked_hosts(&server).await;
        let base = server.uri();
        let policy = SitePolicy::new(
            WebDomainPolicyMode::Enforce,
            owner(&base),
            Arc::new(SiteChecker::default()),
        );
        let refusal = policy
            .final_url_refusal(ORG, "not a url", "", "web_fetch")
            .await;
        assert!(refusal.is_some_and(|message| message.contains("cannot be checked")));
        assert_eq!(requests(&server).await, 0, "no host, so nothing is asked");
    }

    #[tokio::test]
    async fn a_blank_final_url_on_a_denied_requested_host_is_judged_but_not_refused_under_shadow() {
        let server = MockServer::start().await;
        allowed_and_blocked_hosts(&server).await;
        let base = server.uri();
        let policy = SitePolicy::new(
            WebDomainPolicyMode::Shadow,
            owner(&base),
            Arc::new(SiteChecker::default()),
        );
        take_final_url_logs();
        let before = requests(&server).await;
        let refusal = policy
            .final_url_refusal(
                ORG,
                "https://blocked.example/start?q=secret",
                "",
                "web_fetch",
            )
            .await;
        assert_eq!(refusal, None, "shadow refuses nothing");
        assert_eq!(
            requests(&server).await,
            before + 1,
            "shadow judges the requested host, once"
        );

        // The judgement is logged once: the requested host, the page's final URL unknown, and nothing refused. The log
        // carries neither the path nor the query of the requested URL.
        let logged = take_final_url_logs();
        assert_eq!(
            logged,
            vec![FinalUrlLog {
                reason: "blocked",
                host: "blocked.example".to_owned(),
                enforced: false,
                final_known: false,
            }],
            "logged once"
        );
        let text = format!("{logged:?}");
        assert!(!text.contains("/start"), "no path in the log: {text}");
        assert!(!text.contains("secret"), "no query in the log: {text}");
    }

    #[tokio::test]
    async fn a_final_url_with_no_host_is_refused_under_enforce_without_a_request() {
        let server = MockServer::start().await;
        answer(
            &server,
            200,
            json!({"organization_id": ORG, "revision": 6, "decisions": []}),
        )
        .await;
        let base = server.uri();
        let policy = SitePolicy::new(
            WebDomainPolicyMode::Enforce,
            owner(&base),
            Arc::new(SiteChecker::default()),
        );
        let refusal = policy
            .final_url_refusal(ORG, "https://example.com/start", "not a url", "web_fetch")
            .await;
        assert!(refusal.is_some_and(|message| message.contains("cannot be checked")));
        assert_eq!(requests(&server).await, 0, "no host, no call");
    }

    #[test]
    fn the_switch_off_policy_checks_nothing() {
        let policy = SitePolicy::disabled();
        assert_eq!(policy.mode, WebDomainPolicyMode::Off);
    }

    impl SiteChecker {
        /// This checker with the Space floor switch set, as the process would read it. Crate-visible, so the web tools'
        /// tests build a checker with the switch on without the process environment.
        pub(crate) fn with_space_floor(mut self, on: bool) -> Self {
            self.space_floor = on;
            self
        }
    }

    /// The `space_id` each resolve call the server received named, `None` for a call that named none.
    async fn space_ids_named(server: &MockServer) -> Vec<Option<String>> {
        server
            .received_requests()
            .await
            .unwrap_or_default()
            .iter()
            .map(|request| {
                request
                    .url
                    .query_pairs()
                    .find(|(key, _)| key == "space_id")
                    .map(|(_, value)| value.into_owned())
            })
            .collect()
    }

    /// Answers as org-core does for a Space: `space-a` is allowed, and every other call (the organisation's own, or
    /// `space-b`) is denied. Each host of the call gets the same answer.
    struct AnswerBySpace;

    impl wiremock::Respond for AnswerBySpace {
        fn respond(&self, request: &wiremock::Request) -> ResponseTemplate {
            let space = request
                .url
                .query_pairs()
                .find(|(key, _)| key == "space_id")
                .map(|(_, value)| value.into_owned());
            let effective = if space.as_deref() == Some("space-a") {
                "act"
            } else {
                "deny"
            };
            let decisions: Vec<Value> = request
                .url
                .query_pairs()
                .filter(|(key, _)| key == "host")
                .map(|(_, host)| {
                    json!({"host": host, "effective": effective, "cap": "act", "source": "default"})
                })
                .collect();
            ResponseTemplate::new(200).set_body_json(
                json!({"organization_id": ORG, "revision": 4, "decisions": decisions}),
            )
        }
    }

    async fn mount_answer_by_space(server: &MockServer) {
        Mock::given(method("GET"))
            .and(path(RESOLVE_PATH))
            .and(header("x-service-id", "execution-core"))
            .and(header("x-service-token", "test-token"))
            .respond_with(AnswerBySpace)
            .mount(server)
            .await;
    }

    #[test]
    fn the_space_floor_switch_reads_only_the_literal_true() {
        assert!(space_floor_enabled_from(Some("true")));
        for value in [
            None,
            Some(""),
            Some("TRUE"),
            Some("1"),
            Some("on"),
            Some(" true"),
        ] {
            assert!(
                !space_floor_enabled_from(value),
                "{value:?} must not turn the Space floor on"
            );
        }
    }

    /// F-f03-03: a Space is named to org-core only while the switch is on. Off, the call is the organisation's own, and it
    /// shares that answer's cache entry.
    #[tokio::test]
    async fn a_space_is_named_to_org_core_only_under_the_switch() {
        let server = MockServer::start().await;
        mount_answer_by_space(&server).await;
        let base = server.uri();
        let hosts = vec!["example.com".to_owned()];

        let on = SiteChecker::default().with_space_floor(true);
        on.verdicts_in_space(&owner(&base), ORG, "space-a", &hosts)
            .await;
        assert_eq!(
            space_ids_named(&server).await,
            vec![Some("space-a".to_owned())]
        );

        let off = SiteChecker::default().with_space_floor(false);
        off.verdicts_in_space(&owner(&base), ORG, "space-a", &hosts)
            .await;
        let own = off.verdicts_in_space(&owner(&base), ORG, "", &hosts).await;
        assert!(
            !own.allows_host("example.com"),
            "the organisation's own answer is a deny"
        );
        assert_eq!(
            space_ids_named(&server).await,
            vec![Some("space-a".to_owned()), None],
            "off names no Space, and the organisation's own call is served from the entry the Space call left"
        );
    }

    /// F-f03-03 and F-f10-31's mirror: an allow held for one Space never answers another Space's call or the organisation's
    /// own call for the same host, and each Space keeps its own entry.
    #[tokio::test]
    async fn an_allow_held_for_one_space_is_not_another_answer() {
        let server = MockServer::start().await;
        mount_answer_by_space(&server).await;
        let base = server.uri();
        let hosts = vec!["example.com".to_owned()];
        let checker = SiteChecker::default().with_space_floor(true);

        // (space, allowed, org-core calls made so far)
        let steps = [
            ("space-a", true, 1),
            ("", false, 2),
            ("space-a", true, 2),
            ("space-b", false, 3),
            ("", false, 3),
        ];
        for (space, allowed, calls) in steps {
            let verdicts = checker
                .verdicts_in_space(&owner(&base), ORG, space, &hosts)
                .await;
            assert_eq!(
                verdicts.allows_host("example.com"),
                allowed,
                "space {space:?}"
            );
            assert_eq!(
                requests(&server).await,
                calls,
                "space {space:?}: org-core calls"
            );
        }
    }

    /// F-f03-03 at the policy's surface: the Space methods name the Space to org-core only under the switch, and a blank
    /// Space is the organisation's own answer.
    #[tokio::test]
    async fn the_space_methods_name_a_space_only_under_the_switch() {
        let server = MockServer::start().await;
        mount_answer_by_space(&server).await;
        let base = server.uri();

        let on = SitePolicy::new(
            WebDomainPolicyMode::Enforce,
            owner(&base),
            Arc::new(SiteChecker::default().with_space_floor(true)),
        );
        assert_eq!(
            on.fetch_refusal_in_space(ORG, "space-a", &["https://example.com/a"], "web_fetch")
                .await,
            None,
            "the Space allows the host"
        );
        assert!(
            on.fetch_refusal_in_space(ORG, "   ", &["https://example.com/a"], "web_fetch")
                .await
                .is_some(),
            "a blank Space is the organisation's own answer, which denies the host"
        );

        let off = SitePolicy::new(
            WebDomainPolicyMode::Enforce,
            owner(&base),
            Arc::new(SiteChecker::default().with_space_floor(false)),
        );
        assert!(
            off.fetch_refusal_in_space(ORG, "space-a", &["https://example.com/a"], "web_fetch")
                .await
                .is_some(),
            "switched off, the Space is not named, so the organisation's own answer denies the host"
        );
        assert_eq!(
            space_ids_named(&server).await,
            vec![Some("space-a".to_owned()), None, None],
            "only the switched-on Space call names its Space"
        );
    }
}
