//! Open-web egress policy (WS8.B, plan D8-2): a human-browse grant admits any
//! public http(s) host except the broker's denied list, and never a private,
//! loopback, link-local or cloud-metadata host. Built, NOT run (wave 2 rule).

use quarry_browser::{BrokerNavPolicy, BrowserEgressPolicy};

fn denied(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn open_web_admits_public_hosts_only() {
    let policy = BrowserEgressPolicy::open_web(&denied(&["bank.example"]));
    assert!(policy.allows_url("https://example.com/"));
    assert!(policy.allows_url("https://www.iana.org/domains/example"));
    assert!(policy.allows_url("http://news.example.org/path?q=1"));
    // Non-web schemes never gain authority from an open-web grant.
    assert!(!policy.allows_url("file:///etc/passwd"));
    assert!(!policy.allows_url("javascript:alert(1)"));
    assert!(!policy.allows_url("ftp://example.com/"));
    // data: and blob: do not open a connection and keep working.
    assert!(policy.allows_url("data:text/plain,hello"));
    assert!(policy.allows_url("blob:https://example.com/uuid"));
}

#[test]
fn allows_url_open_web_blocks_local_and_metadata_hosts() {
    let policy = BrowserEgressPolicy::open_web(&[]);
    for url in [
        "http://localhost/",
        "http://intranet/",
        "http://127.0.0.1/",
        "http://10.0.0.5/",
        "http://192.168.1.1/",
        "http://172.16.4.4/",
        "http://169.254.169.254/latest/meta-data",
        "http://100.64.0.1/",
        "http://0.0.0.0/",
        "http://[::1]/",
        "http://[fe80::1]/",
        "http://[fd00::1]/",
        "http://[::ffff:10.0.0.1]/",
        "http://metadata.google.internal/",
        "http://metadata.goog/",
        "http://printer.lan/",
        "http://nas.home.arpa/",
        "http://dev.local/",
        "http://app.localhost/",
    ] {
        assert!(!policy.allows_url(url), "open web must refuse {url}");
    }
}

#[test]
fn open_web_policy_still_blocks_private_and_metadata_hosts() {
    let policy = BrowserEgressPolicy::open_web(&[]);
    assert!(!policy.allows_url("http://metadata.google.internal/computeMetadata/v1/"));
    assert!(!policy.allows_url("http://169.254.169.254/"));
    assert!(!policy.allows_url("http://127.0.0.1:9090/"));
}

#[test]
fn denied_entries_match_host_and_subdomains_only() {
    let policy = BrowserEgressPolicy::open_web(&denied(&["bank.example"]));
    assert!(!policy.allows_url("https://bank.example/"));
    assert!(!policy.allows_url("https://login.bank.example/"));
    // A different host that merely ends with the same text is not under it.
    assert!(policy.allows_url("https://notbank.example/"));
    assert!(policy.allows_url("https://bank.example.org/"));
}

#[test]
fn canonical_denied_domains_rejects_noncanonical_input() {
    assert_eq!(
        BrowserEgressPolicy::canonical_denied_domains(&[]).unwrap(),
        Vec::<String>::new()
    );
    assert_eq!(
        BrowserEgressPolicy::canonical_denied_domains(&denied(&["a.example", "b.example"])).unwrap(),
        denied(&["a.example", "b.example"])
    );
    // Unsorted, upper case, wildcard, IP literal, malformed suffix and empty: all refused.
    assert!(BrowserEgressPolicy::canonical_denied_domains(&denied(&["b.example", "a.example"])).is_err());
    assert!(BrowserEgressPolicy::canonical_denied_domains(&denied(&["Bank.example"])).is_err());
    assert!(BrowserEgressPolicy::canonical_denied_domains(&denied(&["*.example"])).is_err());
    assert!(BrowserEgressPolicy::canonical_denied_domains(&denied(&["10.0.0.1"])).is_err());
    assert!(BrowserEgressPolicy::canonical_denied_domains(&denied(&["..localhost"])).is_err());
    assert!(BrowserEgressPolicy::canonical_denied_domains(&denied(&[""])).is_err());
    // Bounded at 64 entries.
    let too_many: Vec<String> = (0..65).map(|i| format!("d{i:02}.example")).collect();
    assert!(BrowserEgressPolicy::canonical_denied_domains(&too_many).is_err());
}

#[test]
fn broker_suffixes_validate_and_block_matching_public_names() {
    let deny = denied(&[".blocked.example", ".home.arpa", ".internal", ".lan", ".local", ".localdomain", ".localhost", ".svc", "metadata.goog", "metadata.google.internal"]);
    let policy = BrokerNavPolicy::from_validation("open_web", &[], &deny).unwrap().egress_policy();
    assert!(policy.denies_url("https://sub.blocked.example/"));
    assert!(!policy.allows_url("https://sub.blocked.example/"));
    assert!(!policy.allows_url("https://service.svc/"));
    assert!(policy.allows_url("https://blocked.example/"));
    assert!(policy.allows_url("https://example.com/"));
}

#[test]
fn broker_nav_policy_fails_closed_on_unknown_mode() {
    assert!(BrokerNavPolicy::from_validation("bogus", &[], &[]).is_err());
    // An allow-list carrying a deny list is refused, not silently widened.
    assert!(BrokerNavPolicy::from_validation("allowlist", &denied(&["a.example"]), &denied(&["x.example"])).is_err());
    // An open-web grant naming allowed hosts is refused.
    assert!(BrokerNavPolicy::from_validation("open_web", &denied(&["a.example"]), &[]).is_err());
    let open = BrokerNavPolicy::from_validation("open_web", &[], &denied(&["bank.example"])).unwrap();
    assert_eq!(open.mode_name(), "open_web");
    assert!(!open.egress_policy().allows_url("https://bank.example/"));
    let allow = BrokerNavPolicy::from_validation("allowlist", &denied(&["example.com"]), &[]).unwrap();
    assert_eq!(allow.mode_name(), "allowlist");
    assert!(allow.egress_policy().allows_url("https://example.com/"));
    assert!(!allow.egress_policy().allows_url("https://other.test/"));
}

#[test]
fn allowlist_default_is_unchanged_by_open_web_fields() {
    // deny_all and from_allowed_domains keep their established meaning.
    assert!(!BrowserEgressPolicy::deny_all().allows_url("https://example.com/"));
    assert!(BrowserEgressPolicy::from_allowed_domains(&[]).allows_url("https://example.com/"));
    assert!(BrowserEgressPolicy::from_allowed_domains(&denied(&["example.com"])).allows_url("https://a.example.com/"));
}
