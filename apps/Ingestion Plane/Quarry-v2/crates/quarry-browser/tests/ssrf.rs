//! SSRF protection tests for chromiumoxide driver.
//!
//! Tests that the browser driver blocks requests to internal/reserved ranges
//! (169.254.169.254, 127.0.0.1, localhost, etc.) and allows public URLs.
//!
//! Note: These tests are marked #[ignore] because they require:
//! 1. The chromiumoxide feature to be enabled
//! 2. A Chromium/Chrome binary available on the system
//! 3. Network access (for public URL tests)
//!
//! To run: `cargo test --test ssrf --features chromiumoxide -- --ignored`

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
        }
    }

    #[tokio::test]
    #[ignore] // Requires chromiumoxide + browser binary
    async fn test_ssrf_block_metadata_endpoint() {
        use quarry_browser::chromiumoxide::ChromiumoxideDriver;
        use quarry_browser::BrowserDriver;

        let driver = ChromiumoxideDriver::new();
        let lease = make_test_lease();

        // Acquire a session first
        let session = match driver.acquire(&lease).await {
            Ok(s) => s,
            Err(_) => {
                // If browser not available, just skip (marked ignore anyway)
                return;
            }
        };

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
    #[ignore] // Requires chromiumoxide + browser binary
    async fn test_ssrf_block_localhost() {
        use quarry_browser::chromiumoxide::ChromiumoxideDriver;
        use quarry_browser::BrowserDriver;

        let driver = ChromiumoxideDriver::new();
        let lease = make_test_lease();

        let session = match driver.acquire(&lease).await {
            Ok(s) => s,
            Err(_) => return,
        };

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
    #[ignore] // Requires chromiumoxide + browser binary
    async fn test_ssrf_block_127_0_0_1() {
        use quarry_browser::chromiumoxide::ChromiumoxideDriver;
        use quarry_browser::BrowserDriver;

        let driver = ChromiumoxideDriver::new();
        let lease = make_test_lease();

        let session = match driver.acquire(&lease).await {
            Ok(s) => s,
            Err(_) => return,
        };

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
    #[ignore] // Requires chromiumoxide + browser binary
    async fn test_ssrf_block_private_ranges() {
        use quarry_browser::chromiumoxide::ChromiumoxideDriver;
        use quarry_browser::BrowserDriver;

        let driver = ChromiumoxideDriver::new();
        let lease = make_test_lease();

        let session = match driver.acquire(&lease).await {
            Ok(s) => s,
            Err(_) => return,
        };

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
    #[ignore] // Requires chromiumoxide + browser binary
    async fn test_valid_public_url() {
        use quarry_browser::chromiumoxide::ChromiumoxideDriver;
        use quarry_browser::BrowserDriver;

        let driver = ChromiumoxideDriver::new();
        let lease = make_test_lease();

        let session = match driver.acquire(&lease).await {
            Ok(s) => s,
            Err(_) => return,
        };

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
