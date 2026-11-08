//! The org's domain rules as the Model Plane's web tools read them (Wave 3 EP-9 W3).
//!
//! org-core holds an organisation's domain rules (DEC-19, `browsersites`) and answers, per host, what the rules
//! allow, through `GET /internal/orgs/:orgId/browser-sites/resolve`. This module is the pure half both loops
//! share: the host canonicaliser that `browser-domain-canonical.json` pins, the reader that takes a URL to its
//! host, the rule that turns an answer into allow or refuse, and the switch that says how far the rules bind.
//! Each service keeps its own resolve client and cache (`site_policy_client.rs`), so the two loops stay separate
//! and share only what is decided here.
//!
//! Nothing here reads a clock, a socket or the environment. The only thing that leaves the Model Plane toward
//! org-core is a canonical host: no URL path, no query and no content.

use serde::Deserialize;

/// The environment switch of the org's domain rules (EP-9 W3). Each service reads its own copy once.
pub const WEB_DOMAIN_POLICY_MODE_ENV: &str = "WEB_DOMAIN_POLICY_MODE";

/// The most hosts one resolve call names. org-core refuses a longer list (`MaxResolveHosts`), so a client batches
/// to this size.
pub const MAX_RESOLVE_HOSTS: usize = 32;

/// How far the org's domain rules bind (`WEB_DOMAIN_POLICY_MODE`). Unset, empty and `off` read nothing and change
/// nothing. `shadow` reads and logs what the rules would refuse and refuses nothing. Any other value enforces,
/// because an authority switch treats an unknown value as enforce (K3-26).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebDomainPolicyMode {
    /// Nothing is read and nothing changes.
    Off,
    /// The rules are read and what they would refuse is logged; nothing is refused.
    Shadow,
    /// The rules bind, and a host whose answer cannot be read is refused.
    Enforce,
}

impl WebDomainPolicyMode {
    /// Parses the switch as the environment holds it.
    #[must_use]
    pub fn from_env_value(value: Option<&str>) -> Self {
        match value.map(str::trim) {
            None | Some("" | "off") => Self::Off,
            Some("shadow") => Self::Shadow,
            Some(_) => Self::Enforce,
        }
    }
}

/// The canonical form of one host, as `browser-domain-canonical.json` pins it: surrounding whitespace is trimmed,
/// letters are folded to lower case, and then only ASCII letters, digits and hyphens may appear, in dot-separated
/// labels. A host needs at least one dot, no label may be empty or start or end with a hyphen, a label may be at
/// most 63 octets and the host at most 253. A trailing dot is refused, so `example.com.` has no canonical form.
/// IP literals are refused, and so is a host whose last label is all digits: no top-level domain is numeric, and
/// that also refuses the shorthand IPv4 forms such as `127.1`.
#[must_use]
pub fn canonical_host(raw: &str) -> Option<String> {
    let host = raw.trim().to_lowercase();
    if host.is_empty() || host.len() > 253 {
        return None;
    }
    if !host
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'.')
    {
        return None;
    }
    if !host.contains('.') {
        return None;
    }
    let mut last_label = "";
    for label in host.split('.') {
        if label.is_empty() || label.len() > 63 || label.starts_with('-') || label.ends_with('-') {
            return None;
        }
        last_label = label;
    }
    if last_label.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(host)
}

/// The canonical host a URL names, read the way a fetch reads it: an absolute `http` or `https` URL whose authority
/// is a plain hostname with an optional port of digits. Userinfo, a bracketed or percent-encoded host, whitespace,
/// control characters or a backslash anywhere in the URL, another scheme and a missing scheme all have no host
/// here. The caller refuses a URL with no host under enforcement, because the rules cannot be applied to it.
#[must_use]
pub fn host_of_url(url: &str) -> Option<String> {
    let url = url.trim();
    if url
        .bytes()
        .any(|byte| byte.is_ascii_control() || byte == b' ' || byte == b'\\')
    {
        return None;
    }
    let rest = after_web_scheme(url)?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.contains('@') {
        return None;
    }
    let host = match authority.split_once(':') {
        Some((host, port)) => {
            if !port.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            host
        }
        None => authority,
    };
    canonical_host(host)
}

/// The text after an `https://` or `http://` scheme, matched without regard to case.
fn after_web_scheme(url: &str) -> Option<&str> {
    ["https://", "http://"].into_iter().find_map(|scheme| {
        let head = url.get(..scheme.len())?;
        head.eq_ignore_ascii_case(scheme)
            .then(|| &url[scheme.len()..])
    })
}

/// Whether an organisation id can name a resolve path: 1 to 128 letters, digits, hyphens or underscores. An id that
/// cannot is never sent to org-core, and its hosts are refused.
#[must_use]
pub fn routable_org_id(org_id: &str) -> bool {
    !org_id.is_empty()
        && org_id.len() <= 128
        && org_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

/// Whether a resolved effective answer lets a fetch through. A fetch is a read and this policy adds no prompt, so
/// `ask`, `read` and `act` allow it. `deny` refuses, and so does any answer this build does not know.
#[must_use]
pub fn fetch_allowed(effective: &str) -> bool {
    matches!(effective, "ask" | "read" | "act")
}

/// One host's answer in org-core's resolve response (`browsersites.Decision`). The cap and source fields are not
/// read here: a fetch needs only whether the answer lets it through.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SiteDecision {
    /// The canonical host the answer is for.
    pub host: String,
    /// `deny`, `ask`, `read` or `act`.
    pub effective: String,
}

/// org-core's answer to one resolve call: one decision per host asked, at one policy revision.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SiteResolution {
    /// The organisation the answer is for. Empty is tolerated; a different organisation is a defect and is refused
    /// by the client.
    #[serde(default)]
    pub organization_id: String,
    /// The policy revision the answer was read at.
    pub revision: i64,
    /// The answers, one per host asked.
    pub decisions: Vec<SiteDecision>,
}

/// What one host's answer means to a fetch: whether the rules let it through, and the revision it was read at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostVerdict {
    /// Whether the fetch may proceed.
    pub allowed: bool,
    /// The policy revision the answer was read at.
    pub revision: i64,
}

/// The verdict for `host` from one resolution. A resolution that answers for no such host refuses it, so an answer
/// that is partial can only narrow what may be fetched.
#[must_use]
pub fn verdict_for(host: &str, resolution: &SiteResolution) -> HostVerdict {
    let allowed = resolution
        .decisions
        .iter()
        .find(|decision| decision.host == host)
        .is_some_and(|decision| fetch_allowed(&decision.effective));
    HostVerdict {
        allowed,
        revision: resolution.revision,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The golden the browser broker's normaliser is pinned to. Both loops canonicalise a host by its rules.
    const GOLDEN: &str = include_str!("../../../../api/contracts/browser-domain-canonical.json");

    fn strings(value: &serde_json::Value) -> Vec<&str> {
        value
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
            .collect()
    }

    #[test]
    fn canonical_host_follows_the_golden_vectors() {
        let golden: serde_json::Value = serde_json::from_str(GOLDEN).expect("the golden parses");
        let vectors = golden["vectors"].as_array().expect("the golden has vectors");
        for vector in vectors {
            let id = vector["id"].as_str().unwrap_or_default();
            let inputs = strings(&vector["input"]);
            if vector["expect"].as_str() == Some("accept") {
                let mut canonical: Vec<String> = inputs
                    .iter()
                    .filter_map(|raw| canonical_host(raw))
                    .collect();
                assert_eq!(canonical.len(), inputs.len(), "{id}: every entry canonicalises");
                canonical.sort();
                canonical.dedup();
                let mut expected: Vec<&str> = strings(&vector["output"]);
                expected.sort_unstable();
                expected.dedup();
                assert_eq!(canonical, expected, "{id}");
            } else if inputs.len() == 1 {
                assert_eq!(canonical_host(inputs[0]), None, "{id}: a bad entry has no canonical host");
            } else if id == "one_bad_entry_rejects_the_whole_list" {
                assert!(
                    inputs.iter().any(|raw| canonical_host(raw).is_none()),
                    "{id}: one entry without a canonical host"
                );
            }
        }
    }

    #[test]
    fn canonical_host_refuses_a_numeric_top_level_label_and_the_empty_forms() {
        for raw in ["127.1", "example.123", "example.com.", "", "   "] {
            assert_eq!(canonical_host(raw), None, "{raw:?}");
        }
        assert_eq!(canonical_host("  Shop.Example.NO ").as_deref(), Some("shop.example.no"));
    }

    #[test]
    fn host_of_url_reads_an_absolute_web_url_as_a_fetch_reads_it() {
        assert_eq!(
            host_of_url("https://Example.com/path?q=1#frag").as_deref(),
            Some("example.com")
        );
        assert_eq!(host_of_url("HTTP://example.com:8080/a").as_deref(), Some("example.com"));
        assert_eq!(host_of_url("  https://example.com  ").as_deref(), Some("example.com"));
        assert_eq!(host_of_url("https://example.com:/").as_deref(), Some("example.com"));
        assert_eq!(host_of_url("https://example.com?x=1").as_deref(), Some("example.com"));
    }

    #[test]
    fn host_of_url_refuses_every_shape_it_cannot_read_with_certainty() {
        for url in [
            "example.com/path",
            "ftp://example.com/file",
            "https://user@example.com/",
            "https://evil.com\\@example.com/",
            "https://example.com\t.evil.com/",
            "https://exa%6Dple.com/",
            "https://[2001:db8::1]/",
            "https://192.168.0.1/",
            "https://example.com:port/",
            "https:///path",
            "https://example.com./",
            "https://localhost/",
            "https://bücher.example/",
        ] {
            assert_eq!(host_of_url(url), None, "{url}");
        }
    }

    #[test]
    fn fetch_is_allowed_for_ask_read_and_act_only() {
        for effective in ["ask", "read", "act"] {
            assert!(fetch_allowed(effective), "{effective}");
        }
        for effective in ["deny", "", "ASK", "none", "allow"] {
            assert!(!fetch_allowed(effective), "{effective:?}");
        }
    }

    #[test]
    fn the_switch_reads_off_shadow_and_enforce_and_an_unknown_value_enforces() {
        assert_eq!(WebDomainPolicyMode::from_env_value(None), WebDomainPolicyMode::Off);
        assert_eq!(WebDomainPolicyMode::from_env_value(Some("")), WebDomainPolicyMode::Off);
        assert_eq!(WebDomainPolicyMode::from_env_value(Some(" off ")), WebDomainPolicyMode::Off);
        assert_eq!(
            WebDomainPolicyMode::from_env_value(Some("shadow")),
            WebDomainPolicyMode::Shadow
        );
        assert_eq!(
            WebDomainPolicyMode::from_env_value(Some("enforce")),
            WebDomainPolicyMode::Enforce
        );
        assert_eq!(WebDomainPolicyMode::from_env_value(Some("on")), WebDomainPolicyMode::Enforce);
    }

    #[test]
    fn a_host_the_resolution_does_not_answer_for_is_refused() {
        let resolution: SiteResolution = serde_json::from_str(
            r#"{"organization_id":"org-a","revision":7,"decisions":[
                {"host":"example.com","effective":"ask","cap":"act","source":"default"},
                {"host":"blocked.example.com","effective":"deny","cap":"none","source":"org_block"},
                {"host":"odd.example.com","effective":"maybe","cap":"act","source":"default"}]}"#,
        )
        .expect("the answer parses");
        assert_eq!(
            verdict_for("example.com", &resolution),
            HostVerdict { allowed: true, revision: 7 }
        );
        assert_eq!(
            verdict_for("blocked.example.com", &resolution),
            HostVerdict { allowed: false, revision: 7 }
        );
        assert_eq!(
            verdict_for("odd.example.com", &resolution),
            HostVerdict { allowed: false, revision: 7 },
            "an answer this build does not know is refused"
        );
        assert_eq!(
            verdict_for("silent.example.com", &resolution),
            HostVerdict { allowed: false, revision: 7 },
            "a host the answer does not name is refused"
        );
    }

    #[test]
    fn an_organisation_id_must_be_a_plain_path_segment() {
        assert!(routable_org_id("org-a_1"));
        for org in ["", "org/a", "org a", "org%2Fa", "org.a"] {
            assert!(!routable_org_id(org), "{org:?}");
        }
    }
}
