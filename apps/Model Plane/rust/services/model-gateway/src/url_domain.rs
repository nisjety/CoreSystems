//! Hostname extraction for web/deep-research citation grouping (chat-wave2
//! citations spec §2.2/§2.6).
//!
//! Ports the frontend's `hostname()` (`chat-media-markdown.tsx:530-536`,
//! `new URL(url).hostname.replace(/^www\./, '')`, falling back to the raw
//! string on a parse failure) by hand rather than adding the `url` crate:
//! this is the one place server and client must derive the SAME group key
//! for the SAME URL, so the Sources tab's domain grouping lines up on both
//! surfaces, but it is a small enough operation not to justify a new
//! dependency.
//!
//! Deliberately narrower than a real URL parser — it only has to agree with
//! `new URL(...).hostname` for the absolute `http(s)://host[:port][/...]`
//! shapes citations actually carry (search-engine and deep-research hit
//! URLs), and to fall back to the raw string exactly like the frontend's
//! `catch` does for anything else (a relative path, a bare domain with no
//! scheme, a malformed value).

/// The grouping key for a web/deep-research citation: the URL's lowercased
/// hostname with a leading `www.` stripped, or the raw `url` unchanged when
/// it does not look like an absolute URL (mirrors the frontend's `catch`
/// fallback so a citation the client cannot parse either still gets a
/// stable, non-empty group key).
#[must_use]
pub fn web_domain(url: &str) -> String {
    let Some(scheme_end) = url.find("://") else {
        return url.to_owned();
    };
    let after_scheme = &url[scheme_end + 3..];
    if after_scheme.is_empty() {
        return url.to_owned();
    }
    let authority_end = after_scheme
        .find(['/', '?', '#'])
        .unwrap_or(after_scheme.len());
    let authority = &after_scheme[..authority_end];
    // Strip `user:pass@` userinfo, if present.
    let host_and_port = authority.rsplit('@').next().unwrap_or(authority);
    if host_and_port.is_empty() {
        return url.to_owned();
    }
    // Strip a trailing `:port`, but not an IPv6 literal's own colons
    // (`[::1]:8080` keeps everything through the closing `]`).
    let host = if let Some(bracket_end) = host_and_port.rfind(']') {
        &host_and_port[..=bracket_end]
    } else {
        host_and_port.split(':').next().unwrap_or(host_and_port)
    };
    let stripped = host.strip_prefix("www.").unwrap_or(host);
    if stripped.is_empty() {
        url.to_owned()
    } else {
        stripped.to_lowercase()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_bare_hostname() {
        assert_eq!(web_domain("https://example.com/path"), "example.com");
    }

    #[test]
    fn strips_www_prefix() {
        assert_eq!(web_domain("https://www.example.com/path"), "example.com");
    }

    #[test]
    fn strips_port_and_query() {
        assert_eq!(
            web_domain("http://example.com:8080/path?q=1#frag"),
            "example.com"
        );
    }

    #[test]
    fn lowercases_the_host() {
        assert_eq!(web_domain("https://Example.COM/Path"), "example.com");
    }

    #[test]
    fn two_urls_on_the_same_host_share_a_group() {
        assert_eq!(
            web_domain("https://example.com/a"),
            web_domain("https://www.example.com/b?x=1"),
        );
    }

    #[test]
    fn distinct_hosts_produce_distinct_groups() {
        assert_ne!(
            web_domain("https://example.com/a"),
            web_domain("https://other.example/b"),
        );
    }

    #[test]
    fn falls_back_to_the_raw_string_on_a_non_absolute_url() {
        assert_eq!(web_domain("not-a-url"), "not-a-url");
    }
}
