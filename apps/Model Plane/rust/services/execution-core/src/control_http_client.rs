//! Shared hardening for outbound HTTP clients that call another plane's HTTP
//! API with a static service-token credential.
//!
//! Extracted from `ticket_tools.rs`'s originally-private copies once
//! `capability_client.rs` needed the exact same validation — this repo's own
//! stated policy (Model Plane's `CLAUDE.md`) is to build a shared seam once a
//! second real consumer lands, not before. Behavior is unchanged from the
//! ticket-action original; only the generic "ticket action" wording in error
//! messages became a caller-supplied `name`.

use std::net::IpAddr;

use reqwest::Url;

/// Validates and normalizes a configured base URL: a bare scheme+host, no
/// credentials/query/fragment/path, and HTTPS unless the caller both allows
/// an insecure loopback endpoint AND the host actually resolves as loopback.
/// `name` should describe the specific setting (e.g. "Control Plane sandbox
/// capability") — it prefixes every error this returns.
pub fn service_base_url(value: &str, name: &str, allow_insecure_loopback: bool) -> Result<Url, String> {
    let url = Url::parse(value.trim()).map_err(|_| format!("{name} URL is invalid"))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/")
    {
        return Err(format!("{name} URL is invalid"));
    }
    if url.scheme() == "http" && (!allow_insecure_loopback || !is_loopback_url(&url)) {
        return Err(format!(
            "{name} URL must use HTTPS; plaintext is permitted only for an explicitly enabled IP-loopback development endpoint"
        ));
    }
    Ok(url)
}

/// Builds the request URL for one call: `base` with its path replaced (never
/// appended — a base URL is validated to have no path of its own) and any
/// query/fragment cleared.
pub fn service_endpoint(base: &Url, path: &str) -> Result<Url, String> {
    let mut endpoint = base.clone();
    endpoint.set_path(path);
    endpoint.set_query(None);
    endpoint.set_fragment(None);
    Ok(endpoint)
}

fn is_loopback_url(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    host.strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(host)
        .parse::<IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false)
}

/// Validates a static service-token credential's length is in a sane range
/// (long enough to not be a placeholder, short enough to not be a mis-pasted
/// blob). `name` should describe the specific credential.
pub fn bounded_secret(value: &str, name: &str) -> Result<String, String> {
    let value = value.trim();
    if value.len() < 32 || value.len() > 4_096 {
        return Err(format!("{name} is invalid"));
    }
    Ok(value.to_owned())
}

pub fn env_value(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

pub fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_base_url_requires_https_unless_loopback_and_opted_in() {
        assert!(service_base_url("https://control.example", "Test", false).is_ok());
        assert!(service_base_url("http://control.example", "Test", false).is_err());
        assert!(service_base_url("http://control.example", "Test", true).is_err());
        assert!(service_base_url("http://127.0.0.1:8080", "Test", false).is_err());
        assert!(service_base_url("http://127.0.0.1:8080", "Test", true).is_ok());
    }

    #[test]
    fn service_base_url_rejects_credentials_query_fragment_and_path() {
        assert!(service_base_url("https://user:pass@control.example", "Test", false).is_err());
        assert!(service_base_url("https://control.example/some/path", "Test", false).is_err());
        assert!(service_base_url("https://control.example?x=1", "Test", false).is_err());
        assert!(service_base_url("https://control.example#frag", "Test", false).is_err());
    }

    #[test]
    fn bounded_secret_rejects_short_and_oversized_values() {
        assert!(bounded_secret(&"a".repeat(31), "Test").is_err());
        assert!(bounded_secret(&"a".repeat(32), "Test").is_ok());
        assert!(bounded_secret(&"a".repeat(4_096), "Test").is_ok());
        assert!(bounded_secret(&"a".repeat(4_097), "Test").is_err());
    }

    #[test]
    fn env_helpers_treat_blank_as_absent() {
        std::env::set_var("CONTROL_HTTP_CLIENT_TEST_VALUE", "  ");
        std::env::set_var("CONTROL_HTTP_CLIENT_TEST_FLAG", "TRUE");
        assert_eq!(env_value("CONTROL_HTTP_CLIENT_TEST_VALUE"), None);
        assert!(env_flag("CONTROL_HTTP_CLIENT_TEST_FLAG"));
        std::env::remove_var("CONTROL_HTTP_CLIENT_TEST_VALUE");
        std::env::remove_var("CONTROL_HTTP_CLIENT_TEST_FLAG");
    }
}
