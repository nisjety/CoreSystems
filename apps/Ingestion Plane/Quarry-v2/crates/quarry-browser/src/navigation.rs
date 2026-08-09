//! Browser navigation admission control.
//!
//! Browser providers ultimately receive an untrusted navigation target. Keep
//! the security decision at this boundary so direct driver calls cannot bypass
//! the runtime's crawl preflight.

use std::net::IpAddr;

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_security::{heur, Decision};
use url::Url;

/// Admit a target before it is handed to a local or remote browser provider.
///
/// `about:blank` is the only non-network target the browser API supports.
/// Every network target must use HTTP(S), pass the shared URL heuristic, and
/// resolve entirely to public addresses. This closes direct driver entry
/// points; Chromium request interception remains a separate defence needed
/// for redirects and page subresources.
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
    use super::{ensure_public_addresses, guard_navigation_target, guard_page_request_target};
    use quarry_core::error::ErrorCode;

    #[tokio::test]
    async fn blocks_private_targets_before_a_browser_is_started() {
        let err = guard_navigation_target("http://127.0.0.1:8080/private")
            .await
            .expect_err("loopback navigation must be blocked");

        assert_eq!(err.code, ErrorCode::SecurityBlocked);
    }

    #[tokio::test]
    async fn blocks_non_web_navigation_schemes() {
        let err = guard_navigation_target("file:///etc/passwd")
            .await
            .expect_err("file navigation must be blocked");

        assert_eq!(err.code, ErrorCode::SecurityBlocked);
    }

    #[tokio::test]
    async fn allows_a_blank_tab_without_triggering_dns() {
        guard_navigation_target("about:blank")
            .await
            .expect("a blank tab is safe and does not fetch a target");
    }

    #[tokio::test]
    async fn allows_non_network_page_resources_without_triggering_dns() {
        guard_page_request_target("data:image/png;base64,iVBORw0KGgo=")
            .await
            .expect("data resources have no network destination");
    }

    #[test]
    fn rejects_private_dns_results() {
        let err = ensure_public_addresses(&["169.254.169.254".parse().unwrap()])
            .expect_err("metadata addresses must be blocked");

        assert_eq!(err.code, ErrorCode::SecurityBlocked);
    }

    #[tokio::test]
    async fn rejects_invalid_url() {
        // Unparseable input is a caller mistake, not an attack: it must be
        // BadRequest, so a typo is never reported as a security block.
        let err = guard_navigation_target("not a url")
            .await
            .expect_err("an unparseable target must be rejected");

        assert_eq!(err.code, ErrorCode::BadRequest);
    }

    #[tokio::test]
    async fn rejects_literal_private_ip() {
        let err = guard_navigation_target("http://10.0.0.5/")
            .await
            .expect_err("RFC1918 targets must be blocked");

        assert_eq!(err.code, ErrorCode::SecurityBlocked);
    }

    #[tokio::test]
    async fn rejects_cloud_metadata_endpoint() {
        // The link-local metadata service is the highest-value SSRF target in
        // any cloud environment, so it gets its own guard test.
        let err = guard_navigation_target("http://169.254.169.254/latest/meta-data/")
            .await
            .expect_err("the cloud metadata endpoint must be blocked");

        assert_eq!(err.code, ErrorCode::SecurityBlocked);
    }

    #[test]
    fn rejects_an_empty_resolution() {
        // A name that resolves to nothing must fail closed. Without this the
        // `is_empty` arm reads as unreachable and could be dropped as dead.
        let err = ensure_public_addresses(&[]).expect_err("an empty resolution must be blocked");

        assert_eq!(err.code, ErrorCode::SecurityBlocked);
    }
}
