//! Security engine tests: heuristic SSRF blocks + discovered-URL scoping.

use quarry_security::{preflight::DefaultEngine, Decision, SecurityEngine};
use url::Url;

fn u(s: &str) -> Url {
    Url::parse(s).unwrap()
}

#[tokio::test]
async fn preflight_allows_plain_https() {
    let e = DefaultEngine::new();
    let v = e.preflight(&u("https://example.com/")).await;
    assert_eq!(v.decision, Decision::Allow);
}

#[tokio::test]
async fn preflight_blocks_non_http_scheme() {
    let e = DefaultEngine::new();
    let v = e.preflight(&u("ftp://example.com/")).await;
    assert_eq!(v.decision, Decision::Block);
}

#[tokio::test]
async fn preflight_blocks_loopback_ipv4() {
    let e = DefaultEngine::new();
    let v = e.preflight(&u("http://127.0.0.1/admin")).await;
    assert_eq!(v.decision, Decision::Block);
}

#[tokio::test]
async fn preflight_blocks_private_ipv4() {
    let e = DefaultEngine::new();
    let v = e.preflight(&u("http://10.0.0.5/")).await;
    assert_eq!(v.decision, Decision::Block);
}

#[tokio::test]
async fn preflight_blocks_internal_hostname() {
    let e = DefaultEngine::new();
    let v = e.preflight(&u("http://localhost/")).await;
    assert_eq!(v.decision, Decision::Block);
    let v2 = e.preflight(&u("http://foo.internal/")).await;
    assert_eq!(v2.decision, Decision::Block);
}

#[tokio::test]
async fn preflight_blocks_userinfo() {
    let e = DefaultEngine::new();
    let v = e.preflight(&u("https://user:pass@example.com/")).await;
    assert_eq!(v.decision, Decision::Block);
}

#[tokio::test]
async fn blocklist_blocks_listed_host() {
    let e = DefaultEngine::new();
    e.block_host("bad.com").await;
    let v = e.preflight(&u("https://bad.com/")).await;
    assert_eq!(v.decision, Decision::Block);
}

#[tokio::test]
async fn allowlist_escalates_unlisted_host() {
    let e = DefaultEngine::new();
    e.allow_host("ok.com").await;
    let v = e.preflight(&u("https://ok.com/")).await;
    assert_eq!(v.decision, Decision::Allow);
    let v2 = e.preflight(&u("https://other.com/")).await;
    assert_eq!(v2.decision, Decision::Escalate);
}

#[tokio::test]
async fn discovered_same_origin_allowed() {
    let e = DefaultEngine::new();
    let parent = u("https://example.com/a");
    let child = u("https://example.com/b");
    let v = e.check_discovered(&parent, &child).await;
    assert_eq!(v.decision, Decision::Allow);
}

#[tokio::test]
async fn discovered_cross_origin_escalates() {
    let e = DefaultEngine::new();
    let parent = u("https://example.com/a");
    let child = u("https://third.com/b");
    let v = e.check_discovered(&parent, &child).await;
    assert_eq!(v.decision, Decision::Escalate);
}

#[tokio::test]
async fn discovered_blocked_child_overrides_scope() {
    let e = DefaultEngine::new();
    let parent = u("https://example.com/a");
    let child = u("http://127.0.0.1/internal");
    let v = e.check_discovered(&parent, &child).await;
    assert_eq!(v.decision, Decision::Block);
}
