//! SSRF-guard: resolves the target host and rejects private/loopback addresses.
//!
//! Called before any outbound request leaves the runtime so that an attacker
//! cannot coerce the scraper into probing internal infrastructure.

use std::net::{IpAddr, SocketAddr};

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_security::heur::resolve_guard;
use url::Url;

/// The exact addresses [`resolve_public_url`] validated for a host, so a
/// driver can pin its connection to them instead of letting the HTTP client
/// re-resolve DNS independently. Without pinning, a hostname could resolve
/// to a public address at guard-check time and to a private one moments
/// later when the connection is actually opened (DNS rebinding); pinning
/// closes that gap by using the exact address set that was already checked.
#[derive(Debug, Clone)]
pub struct ResolvedTarget {
    pub host: String,
    pub addrs: Vec<SocketAddr>,
}

/// Resolve every IP address the host maps to and return `Err` if any of them
/// falls inside a private, loopback, or link-local range.
///
/// # Errors
/// - `BadRequest`      — `url` has no host component.
/// - `Internal`        — DNS lookup failed (network or resolver error).
/// - `SecurityBlocked` — At least one resolved address is private/loopback.
pub async fn guard_url(url: &Url) -> QuarryResult<()> {
    resolve_public_url(url).await.map(|_| ())
}

/// Like [`guard_url`], but returns the validated address set for connection
/// pinning instead of discarding it.
///
/// # Errors
/// Same as [`guard_url`].
pub async fn resolve_public_url(url: &Url) -> QuarryResult<ResolvedTarget> {
    let host = url
        .host_str()
        .ok_or_else(|| QuarryError::new(ErrorCode::BadRequest, "missing host in URL"))?
        .to_string();

    let port = url.port_or_known_default().unwrap_or(80);

    let addrs: Vec<SocketAddr> = tokio::net::lookup_host(format!("{host}:{port}"))
        .await
        .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("dns lookup failed: {e}")))?
        .collect();

    let ips: Vec<IpAddr> = addrs.iter().map(SocketAddr::ip).collect();
    if let Some(reason) = resolve_guard(&ips) {
        return Err(QuarryError::new(ErrorCode::SecurityBlocked, reason));
    }

    Ok(ResolvedTarget { host, addrs })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[tokio::test]
    async fn blocks_loopback() {
        let err = guard_url(&url("http://127.0.0.1/foo")).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::SecurityBlocked);
    }

    #[tokio::test]
    async fn blocks_localhost() {
        let err = guard_url(&url("http://localhost/")).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::SecurityBlocked);
    }

    #[tokio::test]
    async fn resolve_public_url_returns_resolved_addrs_for_public_host() {
        let target = resolve_public_url(&url("https://example.com/"))
            .await
            .unwrap();
        assert_eq!(target.host, "example.com");
        assert!(!target.addrs.is_empty());
        assert!(target.addrs.iter().all(|a| !a.ip().is_loopback()));
    }

    #[tokio::test]
    async fn resolve_public_url_blocks_loopback_same_as_guard_url() {
        let err = resolve_public_url(&url("http://127.0.0.1/foo"))
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::SecurityBlocked);
    }

    #[tokio::test]
    async fn rejects_missing_host() {
        // "file:///etc/passwd" has no host
        let err = guard_url(&url("file:///etc/passwd")).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest);
    }
}
