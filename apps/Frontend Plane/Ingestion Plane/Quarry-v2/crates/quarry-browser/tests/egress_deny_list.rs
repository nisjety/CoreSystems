//! WS7.G: the web policy's deny list at the browser egress boundary. A host on the list
//! is refused in every mode, for every http(s) request the page makes, and an empty list
//! changes nothing. Built, NOT run (wave 2 rule). The live-Chromium counterpart is
//! `egress_deny_blocks_a_subresource_on_an_allowed_page` in `chromiumoxide.rs`.

use quarry_browser::BrowserEgressPolicy;

fn hosts(list: &[&str]) -> Vec<String> {
    list.iter().map(|host| (*host).to_owned()).collect()
}

#[test]
fn an_empty_deny_list_leaves_every_policy_byte_identical() {
    let none: Vec<String> = Vec::new();
    assert_eq!(
        BrowserEgressPolicy::from_allowed_domains(&hosts(&["example.com"])).with_denied_domains(&none),
        BrowserEgressPolicy::from_allowed_domains(&hosts(&["example.com"]))
    );
    assert_eq!(
        BrowserEgressPolicy::from_allowed_domains(&[]).with_denied_domains(&none),
        BrowserEgressPolicy::from_allowed_domains(&[])
    );
    assert_eq!(
        BrowserEgressPolicy::deny_all().with_denied_domains(&none),
        BrowserEgressPolicy::deny_all()
    );
    assert_eq!(
        BrowserEgressPolicy::open_web(&hosts(&["bank.example"])).with_denied_domains(&none),
        BrowserEgressPolicy::open_web(&hosts(&["bank.example"]))
    );
}

#[test]
fn an_unrestricted_run_still_refuses_a_denied_host_and_its_subdomains() {
    let policy =
        BrowserEgressPolicy::from_allowed_domains(&[]).with_denied_domains(&hosts(&["ads.example"]));
    assert!(policy.allows_url("https://news.example.org/"));
    assert!(!policy.allows_url("https://ads.example/pixel.png"));
    assert!(!policy.allows_url("http://cdn.ads.example/pixel.png"));
    // A host that merely ends in the same text is not under it.
    assert!(policy.allows_url("https://notads.example/"));
    assert!(policy.allows_url("https://ads.example.org/"));
}

#[test]
fn a_denied_subdomain_of_an_allowed_parent_is_refused() {
    let policy = BrowserEgressPolicy::from_allowed_domains(&hosts(&["example.com"]))
        .with_denied_domains(&hosts(&["ads.example.com"]));
    assert!(policy.allows_url("https://www.example.com/"));
    assert!(!policy.allows_url("https://ads.example.com/"));
    assert!(!policy.allows_url("https://x.ads.example.com/"));
}

#[test]
fn the_deny_list_is_normalised_and_deduplicated() {
    let policy = BrowserEgressPolicy::from_allowed_domains(&[]).with_denied_domains(&hosts(&[
        " Ads.Example. ",
        "ads.example",
        "",
        "  ",
    ]));
    assert_eq!(policy.denied_domains(), &["ads.example".to_owned()]);
    assert!(policy.denies_url("https://ADS.example/"));
    assert!(!policy.denies_url("data:text/plain,hello"));
    assert!(!policy.denies_url("not a url"));
}

#[test]
fn data_and_blob_urls_keep_working_under_a_deny_list() {
    let policy =
        BrowserEgressPolicy::from_allowed_domains(&[]).with_denied_domains(&hosts(&["ads.example"]));
    assert!(policy.allows_url("data:image/png;base64,AAAA"));
    assert!(policy.allows_url("blob:https://ads.example/uuid"));
}

#[test]
fn a_deny_list_never_widens_a_deny_all_policy() {
    let policy = BrowserEgressPolicy::deny_all().with_denied_domains(&hosts(&["ads.example"]));
    assert!(!policy.allows_url("https://example.com/"));
    assert!(!policy.allows_url("https://ads.example/"));
}

#[test]
fn open_web_merges_the_policy_list_with_the_grants() {
    let policy = BrowserEgressPolicy::open_web(&hosts(&["bank.example"]))
        .with_denied_domains(&hosts(&["ads.example"]));
    assert!(!policy.allows_url("https://bank.example/"));
    assert!(!policy.allows_url("https://ads.example/"));
    assert!(policy.allows_url("https://example.com/"));
    // Private hosts stay refused whatever the lists say.
    assert!(!policy.allows_url("http://127.0.0.1/"));
}

#[test]
fn only_the_unrestricted_allow_list_is_unconstrained() {
    assert!(BrowserEgressPolicy::from_allowed_domains(&[]).is_unconstrained());
    assert!(BrowserEgressPolicy::from_allowed_domains(&[])
        .with_denied_domains(&hosts(&["ads.example"]))
        .is_unconstrained());
    assert!(!BrowserEgressPolicy::from_allowed_domains(&hosts(&["example.com"])).is_unconstrained());
    assert!(!BrowserEgressPolicy::deny_all().is_unconstrained());
    assert!(!BrowserEgressPolicy::open_web(&[]).is_unconstrained());
}
