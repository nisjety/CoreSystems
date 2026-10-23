//! WS7.G (6a): the web policy as a backstop inside the security engine.
//!
//! The edge decides a request's policy once and gates the URLs it knows about. A page run
//! can still end up somewhere the edge never saw: a redirect moves the fetch off the
//! requested URL, a crawl discovers a link. `PolicySecurityEngine` puts the same host
//! rules where those URLs arrive, without a new `PageRunner` field: it is a
//! `SecurityEngine`, so it sits in the `security` slot the runner already has.
//!
//! * `preflight` and `check_discovered` apply the resolved policy's host rules (deny
//!   always, allow lists for an autonomous principal) and then ask the wrapped engine, so
//!   every rule of the wrapped engine (private hosts, blocklists, URL reputation) stays.
//! * `allow_private_hosts` is the wrapped engine's answer, unchanged.
//! * `recheck_final_url` tells the pipeline to preflight the final URL of a fetch again
//!   when a redirect moved it. With the flag off the engine behaves exactly as the
//!   wrapped one plus the host rules.
//!
//! A block reason starts with [`BLOCK_PREFIX`] and carries the denial code and the layer
//! that decided it. It never carries the host, a path or a query. An observer (the edge's
//! counter) is told about each enforced denial; this crate has no metrics of its own.

use std::sync::Arc;

use async_trait::async_trait;
use quarry_core::web_policy::{PolicyDenial, ResolvedPolicy};
use quarry_security::{SecurityEngine, Verdict};
use url::Url;

/// Every block this engine raises starts with this text.
pub const BLOCK_PREFIX: &str = "policy_denied:";

/// Told about each denial the engine raises (the edge counts it).
pub type DenialObserver = Arc<dyn Fn(&PolicyDenial) + Send + Sync>;

/// A security engine that holds a run to its resolved web policy, then defers to the
/// engine it wraps.
#[derive(Clone)]
pub struct PolicySecurityEngine {
    inner: Arc<dyn SecurityEngine>,
    resolved: ResolvedPolicy,
    recheck_final_url: bool,
    observer: Option<DenialObserver>,
}

impl PolicySecurityEngine {
    /// Wrap `inner` with the host rules of `resolved`. The final-URL recheck is off until
    /// [`PolicySecurityEngine::with_final_url_check`] turns it on.
    pub fn new(inner: Arc<dyn SecurityEngine>, resolved: ResolvedPolicy) -> Self {
        Self {
            inner,
            resolved,
            recheck_final_url: false,
            observer: None,
        }
    }

    /// Turn the pipeline's final-URL recheck on or off for runs under this engine.
    pub fn with_final_url_check(mut self, on: bool) -> Self {
        self.recheck_final_url = on;
        self
    }

    /// Report each denial to `observer`.
    pub fn with_observer(mut self, observer: DenialObserver) -> Self {
        self.observer = Some(observer);
        self
    }

    /// The block verdict for `url` under the host rules, or `None` when they admit it.
    fn deny(&self, url: &Url) -> Option<Verdict> {
        let denial = self.resolved.check_url(url.as_str()).err()?;
        if let Some(observer) = &self.observer {
            observer(&denial);
        }
        Some(Verdict::block(block_reason(&denial)))
    }
}

/// The reason text of a denial: the prefix, the closed denial code and, for a domain rule,
/// the layer that decided. No host, no URL.
pub fn block_reason(denial: &PolicyDenial) -> String {
    match denial {
        PolicyDenial::DomainDenied {
            decision_source, ..
        }
        | PolicyDenial::DomainNotAllowed {
            decision_source, ..
        } => format!(
            "{BLOCK_PREFIX} {} ({})",
            denial.code(),
            decision_source.as_str()
        ),
        _ => format!("{BLOCK_PREFIX} {}", denial.code()),
    }
}

#[async_trait]
impl SecurityEngine for PolicySecurityEngine {
    async fn preflight(&self, url: &Url) -> Verdict {
        if let Some(verdict) = self.deny(url) {
            return verdict;
        }
        self.inner.preflight(url).await
    }

    async fn check_discovered(&self, parent: &Url, child: &Url) -> Verdict {
        if let Some(verdict) = self.deny(child) {
            return verdict;
        }
        self.inner.check_discovered(parent, child).await
    }

    fn allow_private_hosts(&self) -> bool {
        self.inner.allow_private_hosts()
    }

    fn recheck_final_url(&self) -> bool {
        self.recheck_final_url
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use quarry_core::web_policy::{resolve, OrgInput, PolicyInputs, PolicyLayer, Principal};
    use quarry_security::preflight::DefaultEngine;
    use quarry_security::Decision;

    use super::*;

    /// An inner engine that counts the questions it is asked and answers `allow`.
    #[derive(Default)]
    struct Counting {
        preflights: AtomicUsize,
        discovered: AtomicUsize,
        private: bool,
    }

    #[async_trait]
    impl SecurityEngine for Counting {
        async fn preflight(&self, _url: &Url) -> Verdict {
            self.preflights.fetch_add(1, Ordering::SeqCst);
            Verdict::allow()
        }
        async fn check_discovered(&self, _parent: &Url, _child: &Url) -> Verdict {
            self.discovered.fetch_add(1, Ordering::SeqCst);
            Verdict::allow()
        }
        fn allow_private_hosts(&self) -> bool {
            self.private
        }
    }

    fn policy(principal: Principal, allow: Option<&[&str]>, deny: &[&str]) -> ResolvedPolicy {
        let layer = PolicyLayer::new(
            BTreeMap::new(),
            None,
            None,
            allow.map(|list| list.iter().map(|host| host.to_string()).collect()),
            deny.iter().map(|host| host.to_string()).collect(),
        )
        .expect("a valid layer");
        resolve(&PolicyInputs {
            principal,
            zdr: false,
            baseline: PolicyLayer::default(),
            org: OrgInput::Absent,
            grant: None,
            request: Some(layer),
        })
        .expect("the policy resolves")
    }

    fn url(raw: &str) -> Url {
        Url::parse(raw).expect("a test URL")
    }

    #[tokio::test]
    async fn a_denied_host_is_blocked_before_the_wrapped_engine_is_asked() {
        let inner = Arc::new(Counting::default());
        let engine = PolicySecurityEngine::new(
            inner.clone(),
            policy(Principal::Human, None, &["denied.example"]),
        );
        let verdict = engine.preflight(&url("https://denied.example/page?q=secret")).await;
        assert_eq!(verdict.decision, Decision::Block);
        assert!(verdict.reasons[0].starts_with(BLOCK_PREFIX), "{:?}", verdict.reasons);
        let text = verdict.reasons.join(" ");
        assert!(!text.contains("denied.example"), "the reason names no host: {text}");
        assert!(!text.contains("secret"), "the reason names no query: {text}");
        assert_eq!(inner.preflights.load(Ordering::SeqCst), 0);

        // A subdomain of a denied host is denied, an unrelated host reaches the inner engine.
        let sub = engine.preflight(&url("https://www.denied.example/")).await;
        assert_eq!(sub.decision, Decision::Block);
        let other = engine.preflight(&url("https://other.example/")).await;
        assert_eq!(other.decision, Decision::Allow);
        assert_eq!(inner.preflights.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn an_allow_list_binds_an_autonomous_principal_only() {
        let allow = Some(&["only.example"][..]);
        let autonomous = PolicySecurityEngine::new(
            Arc::new(Counting::default()),
            policy(Principal::Autonomous, allow, &[]),
        );
        let outside = autonomous.preflight(&url("https://elsewhere.example/")).await;
        assert_eq!(outside.decision, Decision::Block);
        assert!(outside.reasons[0].contains("domain_not_allowed"));
        assert_eq!(
            autonomous.preflight(&url("https://only.example/")).await.decision,
            Decision::Allow
        );

        let human = PolicySecurityEngine::new(
            Arc::new(Counting::default()),
            policy(Principal::Human, allow, &[]),
        );
        assert_eq!(
            human.preflight(&url("https://elsewhere.example/")).await.decision,
            Decision::Allow,
            "a person is not held to an allow list"
        );
    }

    #[tokio::test]
    async fn a_discovered_link_to_a_denied_host_is_blocked() {
        let inner = Arc::new(Counting::default());
        let engine = PolicySecurityEngine::new(
            inner.clone(),
            policy(Principal::Autonomous, None, &["denied.example"]),
        );
        let parent = url("https://site.example/");
        let blocked = engine
            .check_discovered(&parent, &url("https://denied.example/x"))
            .await;
        assert_eq!(blocked.decision, Decision::Block);
        assert_eq!(inner.discovered.load(Ordering::SeqCst), 0);
        let kept = engine
            .check_discovered(&parent, &url("https://site.example/next"))
            .await;
        assert_eq!(kept.decision, Decision::Allow);
        assert_eq!(inner.discovered.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn policy_security_engine_delegates_private_host_rules() {
        // The private-host answer is the wrapped engine's, in both directions.
        let strict = PolicySecurityEngine::new(
            Arc::new(DefaultEngine::new()),
            policy(Principal::Human, None, &[]),
        );
        assert!(!strict.allow_private_hosts());
        let loopback = strict.preflight(&url("http://127.0.0.1/")).await;
        assert_eq!(loopback.decision, Decision::Block, "the wrapped engine still blocks a private host");
        assert!(!loopback.reasons[0].starts_with(BLOCK_PREFIX));

        let lenient = PolicySecurityEngine::new(
            Arc::new(DefaultEngine::new().with_allow_private_hosts(true)),
            policy(Principal::Human, None, &[]),
        );
        assert!(lenient.allow_private_hosts());
        assert_eq!(
            lenient.preflight(&url("http://127.0.0.1/")).await.decision,
            Decision::Allow
        );
    }

    #[test]
    fn the_final_url_recheck_is_off_until_asked_for() {
        let off = PolicySecurityEngine::new(
            Arc::new(Counting::default()),
            policy(Principal::Human, None, &[]),
        );
        assert!(!off.recheck_final_url());
        assert!(off.with_final_url_check(true).recheck_final_url());
    }

    #[tokio::test]
    async fn the_observer_hears_each_denial_and_nothing_else() {
        let heard = Arc::new(AtomicUsize::new(0));
        let counter = heard.clone();
        let engine = PolicySecurityEngine::new(
            Arc::new(Counting::default()),
            policy(Principal::Human, None, &["denied.example"]),
        )
        .with_observer(Arc::new(move |_denial| {
            counter.fetch_add(1, Ordering::SeqCst);
        }));
        engine.preflight(&url("https://denied.example/")).await;
        engine.preflight(&url("https://fine.example/")).await;
        assert_eq!(heard.load(Ordering::SeqCst), 1);
    }
}
