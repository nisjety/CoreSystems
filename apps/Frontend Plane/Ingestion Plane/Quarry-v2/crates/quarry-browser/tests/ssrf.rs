//! SSRF protection tests for chromiumoxide driver.
//!
//! Tests that the browser driver blocks requests to internal/reserved ranges
//! (169.254.169.254, 127.0.0.1, localhost, etc.) and allows public URLs.
//!
//! What these prove: `ChromiumoxideDriver::goto` refuses the URL in Rust
//! (`guard_navigation_target`) with `SecurityBlocked` before it sends anything
//! over CDP. The browser launched by `acquire` is never asked to navigate; the
//! egress boundary a running Chromium sits behind is proven in
//! `quarry-runtime/tests/browser_egress_boundary.rs`.
//!
//! The four blocking tests run in the live Chromium lane
//! (`CHROMIUMOXIDE_TEST=1`, `--features chromiumoxide`) through
//! `quarry_browser::live_gate`: without the flag they return early, with it they
//! FAIL when Chromium cannot be launched. `test_valid_public_url` stays
//! `#[ignore]`d on purpose: it needs DNS for a public host (offline, the guard
//! reports a resolution failure as `SecurityBlocked`), and this driver has no
//! pinned proxy here, so it can only assert a negative.
//!
//! To run: `CHROMIUMOXIDE_TEST=1 cargo test -p quarry-browser --features chromiumoxide --test ssrf`

#[cfg(all(feature = "chromiumoxide", test))]
mod ssrf_tests {
    use quarry_core::error::ErrorCode;
    use quarry_core::ids::kinds;
    use quarry_core::lease::{BrowserLease, Capability, ProxyAffinity};

    fn make_test_lease() -> BrowserLease {
        BrowserLease {
            lease_id: kinds::LeaseKind::new(),
            profile_id: kinds::ProfileKind::new(),
            session_affinity_key: "test-affinity-key".to_string(),
            proxy_affinity: ProxyAffinity {
                pool: "default".to_string(),
                sticky_key: None,
            },
            ttl_s: 3600,
            capabilities: vec![Capability::Screenshots],
            artifact_bucket: "test-bucket".to_string(),
            persist_profile: false,
            viewport: None,
            org_id: "test-org".to_string(),
        }
    }

    #[tokio::test]
    async fn test_ssrf_block_metadata_endpoint() {
        use quarry_browser::chromiumoxide::ChromiumoxideDriver;
        use quarry_browser::BrowserDriver;

        if !quarry_browser::live_gate::live_enabled() {
            return;
        }
        let driver = ChromiumoxideDriver::new();
        let lease = make_test_lease();

        // Acquire a session first. A browser that cannot start is a failure of
        // the live run, not a reason to pass.
        let session = driver
            .acquire(&lease)
            .await
            .expect("acquire session (this launches Chromium)");

        // Attempt to fetch metadata endpoint — should be blocked
        let result = driver.goto(&session, "http://169.254.169.254/").await;

        // Clean up session
        let _ = driver.release(session).await;

        assert!(result.is_err(), "SSRF: metadata endpoint should be blocked");

        if let Err(err) = result {
            assert_eq!(
                err.code,
                ErrorCode::SecurityBlocked,
                "Expected SecurityBlocked error code for metadata endpoint"
            );
        }
    }

    #[tokio::test]
    async fn test_ssrf_block_localhost() {
        use quarry_browser::chromiumoxide::ChromiumoxideDriver;
        use quarry_browser::BrowserDriver;

        if !quarry_browser::live_gate::live_enabled() {
            return;
        }
        let driver = ChromiumoxideDriver::new();
        let lease = make_test_lease();

        let session = driver
            .acquire(&lease)
            .await
            .expect("acquire session (this launches Chromium)");

        let result = driver.goto(&session, "http://localhost:3000/").await;
        let _ = driver.release(session).await;

        assert!(result.is_err(), "SSRF: localhost should be blocked");

        if let Err(err) = result {
            assert_eq!(
                err.code,
                ErrorCode::SecurityBlocked,
                "Expected SecurityBlocked error code for localhost"
            );
        }
    }

    #[tokio::test]
    async fn test_ssrf_block_127_0_0_1() {
        use quarry_browser::chromiumoxide::ChromiumoxideDriver;
        use quarry_browser::BrowserDriver;

        if !quarry_browser::live_gate::live_enabled() {
            return;
        }
        let driver = ChromiumoxideDriver::new();
        let lease = make_test_lease();

        let session = driver
            .acquire(&lease)
            .await
            .expect("acquire session (this launches Chromium)");

        let result = driver.goto(&session, "http://127.0.0.1:8080/").await;
        let _ = driver.release(session).await;

        assert!(result.is_err(), "SSRF: 127.0.0.1 should be blocked");

        if let Err(err) = result {
            assert_eq!(
                err.code,
                ErrorCode::SecurityBlocked,
                "Expected SecurityBlocked error code for 127.0.0.1"
            );
        }
    }

    #[tokio::test]
    async fn test_ssrf_block_private_ranges() {
        use quarry_browser::chromiumoxide::ChromiumoxideDriver;
        use quarry_browser::BrowserDriver;

        if !quarry_browser::live_gate::live_enabled() {
            return;
        }
        let driver = ChromiumoxideDriver::new();
        let lease = make_test_lease();

        let session = driver
            .acquire(&lease)
            .await
            .expect("acquire session (this launches Chromium)");

        let result = driver.goto(&session, "http://10.0.0.1/").await;
        let _ = driver.release(session).await;

        assert!(result.is_err(), "SSRF: 10.0.0.0/8 should be blocked");

        if let Err(err) = result {
            assert_eq!(
                err.code,
                ErrorCode::SecurityBlocked,
                "Expected SecurityBlocked error code for private range"
            );
        }
    }

    #[tokio::test]
    #[ignore = "needs DNS for a public host; see the file header"]
    async fn test_valid_public_url() {
        use quarry_browser::chromiumoxide::ChromiumoxideDriver;
        use quarry_browser::BrowserDriver;

        if !quarry_browser::live_gate::live_enabled() {
            return;
        }
        let driver = ChromiumoxideDriver::new();
        let lease = make_test_lease();

        let session = driver
            .acquire(&lease)
            .await
            .expect("acquire session (this launches Chromium)");

        let result = driver.goto(&session, "https://example.com/").await;
        let _ = driver.release(session).await;

        // If it fails, it should NOT be SecurityBlocked
        if let Err(e) = result {
            assert_ne!(
                e.code,
                ErrorCode::SecurityBlocked,
                "Public URL should not be blocked by SSRF guard"
            );
        }
    }
}
