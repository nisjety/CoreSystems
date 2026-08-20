//! TLS-profile driver backed by rquest upstream (`wreq`) and BoringSSL.
//!
//! The old implementation used `reqwest` + `rustls` with browser-like headers.
//! This adapter delegates to `quarry-tls`, which configures `wreq::Emulation`
//! for browser-family TLS and HTTP/2 fingerprints while preserving the runtime
//! `Driver` trait.

use std::time::Duration;

use async_trait::async_trait;
use quarry_core::output::DriverKind;
use quarry_core::QuarryResult;
pub use quarry_tls::TlsProfile;
use quarry_tls::{TlsClientConfig, TlsDnsPin, WreqTlsClient};
use url::Url;

use crate::driver::{Driver, FetchHints};
use crate::fetch::FetchResponse;

/// A [`Driver`] that fetches pages using browser-like TLS and HTTP/2 emulation.
pub struct TlsProfileDriver {
    client: WreqTlsClient,
}

impl TlsProfileDriver {
    /// Create a new driver.
    ///
    /// `timeout` is the end-to-end request timeout. `profile` selects the
    /// browser-family fingerprint to impersonate. `user_agent_override`
    /// replaces only the HTTP header; it does not downgrade the TLS emulation.
    pub fn new(
        profile: TlsProfile,
        timeout: Duration,
        user_agent_override: Option<&str>,
    ) -> QuarryResult<Self> {
        let base_config = TlsClientConfig::default()
            .with_profile(profile)
            .with_timeout(timeout);
        let config = match user_agent_override {
            Some(user_agent) => base_config.with_user_agent_override(user_agent),
            None => base_config,
        };
        let client = WreqTlsClient::new(config)?;
        Ok(Self { client })
    }

    pub fn profile(&self) -> TlsProfile {
        self.client.profile()
    }
}

#[async_trait]
impl Driver for TlsProfileDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::Tls
    }

    async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse> {
        Ok(into_fetch_response(self.client.fetch(url).await?))
    }

    async fn fetch_conditional(
        &self,
        url: &Url,
        hints: &FetchHints,
    ) -> QuarryResult<FetchResponse> {
        let response = match hints.resolved_target.as_ref() {
            Some(target) => {
                let pin = TlsDnsPin::new(target.host.clone(), target.addresses.clone())?;
                self.client.fetch_with_dns_pin(url, &pin).await?
            }
            None => self.client.fetch(url).await?,
        };
        Ok(into_fetch_response(response))
    }

    fn tls_profile(&self) -> Option<quarry_tls::TlsProfile> {
        Some(self.client.profile())
    }
}

fn into_fetch_response(response: quarry_tls::TlsFetchResponse) -> FetchResponse {
    FetchResponse {
        status: response.status,
        final_url: response.final_url,
        headers: response.headers,
        body: response.body,
        duration_ms: response.duration_ms,
        served_by: DriverKind::Tls,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarry_core::error::ErrorCode;
    use std::net::SocketAddr;

    use crate::dns_guard::ResolvedTarget;
    use crate::driver::FetchHints;

    #[test]
    fn profile_user_agents_are_distinct() {
        let agents: Vec<&str> = vec![
            TlsProfile::Chrome.user_agent(),
            TlsProfile::Firefox.user_agent(),
            TlsProfile::Safari.user_agent(),
        ];
        let unique: std::collections::HashSet<_> = agents.iter().copied().collect();
        assert_eq!(unique.len(), 3, "each profile must have a distinct UA");
    }

    #[test]
    fn legacy_profile_aliases_work() {
        assert_eq!(TlsProfile::Chrome120, TlsProfile::Chrome);
        assert_eq!(TlsProfile::Firefox121, TlsProfile::Firefox);
        assert_eq!(TlsProfile::Safari17, TlsProfile::Safari);
    }

    #[test]
    fn driver_reports_tls_kind() {
        let driver = TlsProfileDriver::new(TlsProfile::Chrome120, Duration::from_secs(30), None)
            .expect("tls driver builds");
        assert_eq!(driver.kind(), DriverKind::Tls);
        assert_eq!(driver.profile(), TlsProfile::Chrome);
    }

    #[test]
    fn ua_override_is_accepted() {
        let driver = TlsProfileDriver::new(
            TlsProfile::Firefox121,
            Duration::from_secs(30),
            Some("CustomBot/1.0"),
        )
        .expect("tls driver accepts user-agent override");
        assert_eq!(driver.kind(), DriverKind::Tls);
        assert_eq!(driver.profile(), TlsProfile::Firefox);
    }

    #[tokio::test]
    async fn rejects_a_preflight_pin_for_a_different_host_before_connecting() {
        let driver = TlsProfileDriver::new(TlsProfile::Chrome, Duration::from_millis(100), None)
            .expect("tls driver builds");
        let url: Url = "http://127.0.0.1:9/".parse().expect("valid URL");
        let hints = FetchHints {
            resolved_target: Some(ResolvedTarget {
                host: "different.example".to_string(),
                addresses: vec!["8.8.8.8:443".parse::<SocketAddr>().unwrap()],
            }),
            ..FetchHints::default()
        };

        let error = driver
            .fetch_conditional(&url, &hints)
            .await
            .expect_err("a mismatched pin must fail before any connection");

        assert_eq!(error.code, ErrorCode::BadRequest);
    }
}
