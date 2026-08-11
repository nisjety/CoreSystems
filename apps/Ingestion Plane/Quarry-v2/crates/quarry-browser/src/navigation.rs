//! Shared SSRF navigation guard for all browser drivers.
//!
//! Every [`crate::BrowserDriver`] must call [`guard_navigation_target`] before
//! sending a caller-provided URL to a browser session (session-create or
//! `goto`). It runs the same URL heuristics and DNS-resolution checks as the
//! static fetch path (`quarry_security::heur`), so a private/internal target
//! can't be reached just because the request goes through a browser instead
//! of the static `wreq` fetcher — including DNS-rebinding, where a hostname
//! passes the string-level heuristic but resolves to an internal address.
//!
//! [`guard_page_request_target`] additionally covers requests a page issues
//! after navigation (redirects, `fetch`/XHR, iframes, images, stylesheets).
//! Only the chromiumoxide driver calls it today (`install_network_guard`),
//! because it holds a live CDP `Page` and can attach a `Fetch` domain
//! listener to it. The Browserbase, Browserless, and Kernel drivers talk to
//! their provider over a REST helper API and hold no local CDP session, so
//! `guard_navigation_target` is the only defence they have: it blocks the
//! entry point, but a redirect or subresource load the remote page issues on
//! its own after that is not re-checked. See `docs/GAP.md` for the
//! per-provider capability audit of that gap.

use std::net::IpAddr;

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_security::{heur, Decision};
use url::Url;

/// Validate a caller-provided URL before letting any browser driver navigate
/// to it (session-create or `goto`). Runs the same scheme/private-host
/// heuristics as the static fetch path, then resolves the host and rejects if
/// any resolved address is private/loopback/link-local.
pub async fn guard_navigation_target(raw: &str) -> QuarryResult<()> {
    if raw == "about:blank" {
        return Ok(());
    }

    let url = Url::parse(raw)
        .map_err(|_| QuarryError::new(ErrorCode::BadRequest, "invalid browser navigation URL"))?;
    let verdict = heur::check(&url);
    if verdict.decision != Decision::Allow {
        return Err(QuarryError::new(
            ErrorCode::SecurityBlocked,
            "browser navigation target is blocked by security policy",
        ));
    }

    let host = url.host_str().ok_or_else(|| {
        QuarryError::new(
            ErrorCode::SecurityBlocked,
            "browser navigation target is missing a host",
        )
    })?;
    let port = url.port_or_known_default().unwrap_or(80);
    let addrs = tokio::net::lookup_host(format!("{host}:{port}"))
        .await
        .map_err(|_| {
            QuarryError::new(
                ErrorCode::SecurityBlocked,
                "browser target DNS lookup failed",
            )
        })?
        .map(|address| address.ip())
        .collect::<Vec<_>>();

    ensure_public_addresses(&addrs)
}

/// Admit a URL requested from within an already-loaded page.
///
/// Data and blob resources carry no new network destination, so they are safe
/// to continue. All other requests use the same HTTP(S) and DNS policy as an
/// explicit browser navigation.
pub async fn guard_page_request_target(raw: &str) -> QuarryResult<()> {
    let url = Url::parse(raw)
        .map_err(|_| QuarryError::new(ErrorCode::BadRequest, "invalid browser request URL"))?;
    if matches!(url.scheme(), "data" | "blob") {
        return Ok(());
    }
    guard_navigation_target(raw).await
}

fn ensure_public_addresses(addrs: &[IpAddr]) -> QuarryResult<()> {
    if addrs.is_empty() || heur::resolve_guard(addrs).is_some() {
        return Err(QuarryError::new(
            ErrorCode::SecurityBlocked,
            "browser navigation target resolves to a blocked address",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn allows_about_blank() {
        guard_navigation_target("about:blank").await.unwrap();
    }

    #[tokio::test]
    async fn rejects_invalid_url() {
        let err = guard_navigation_target("not a url").await.unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest);
    }

    #[tokio::test]
    async fn rejects_disallowed_scheme() {
        let err = guard_navigation_target("file:///etc/passwd")
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::SecurityBlocked);
    }

    #[tokio::test]
    async fn rejects_literal_loopback_ip() {
        let err = guard_navigation_target("http://127.0.0.1:8080/admin")
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::SecurityBlocked);
    }

    #[tokio::test]
    async fn rejects_literal_private_ip() {
        let err = guard_navigation_target("http://10.0.0.5/")
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::SecurityBlocked);
    }

    #[tokio::test]
    async fn rejects_cloud_metadata_endpoint() {
        let err = guard_navigation_target("http://169.254.169.254/latest/meta-data/")
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::SecurityBlocked);
    }

    #[tokio::test]
    async fn rejects_localhost_hostname() {
        let err = guard_navigation_target("http://localhost:9999/")
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::SecurityBlocked);
    }

    #[tokio::test]
    async fn page_request_allows_data_scheme() {
        guard_page_request_target("data:text/plain;base64,aGk=")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn page_request_allows_blob_scheme() {
        guard_page_request_target("blob:https://example.com/uuid")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn page_request_rejects_private_target_same_as_navigation() {
        let err = guard_page_request_target("http://192.168.1.1/")
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::SecurityBlocked);
    }

    #[test]
    fn ensure_public_addresses_rejects_empty_resolution() {
        let err = ensure_public_addresses(&[]).unwrap_err();
        assert_eq!(err.code, ErrorCode::SecurityBlocked);
    }
}
