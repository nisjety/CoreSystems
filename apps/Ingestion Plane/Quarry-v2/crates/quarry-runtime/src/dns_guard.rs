//! SSRF-guard: resolves the target host and rejects private/loopback addresses.
//!
//! Called before any outbound request leaves the runtime so that an attacker
//! cannot coerce the scraper into probing internal infrastructure.

use std::net::IpAddr;

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_security::heur::resolve_guard;
use url::Url;

/// Resolve every IP address the host maps to and return `Err` if any of them
/// falls inside a private, loopback, or link-local range.
///
/// # Errors
/// - `BadRequest`      — `url` has no host component.
/// - `Internal`        — DNS lookup failed (network or resolver error).
/// - `SecurityBlocked` — At least one resolved address is private/loopback.
pub async fn guard_url(url: &Url) -> QuarryResult<()> {
    let host = url
        .host_str()
        .ok_or_else(|| QuarryError::new(ErrorCode::BadRequest, "missing host in URL"))?;

    let port = url.port_or_known_default().unwrap_or(80);

    let addrs: Vec<IpAddr> = tokio::net::lookup_host(format!("{host}:{port}"))
        .await
        .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("dns lookup failed: {e}")))?
        .map(|s| s.ip())
        .collect();

    if let Some(reason) = resolve_guard(&addrs) {
        return Err(QuarryError::new(ErrorCode::SecurityBlocked, reason));
    }

    Ok(())
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
    async fn rejects_missing_host() {
        // "file:///etc/passwd" has no host
        let err = guard_url(&url("file:///etc/passwd")).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest);
    }
}
