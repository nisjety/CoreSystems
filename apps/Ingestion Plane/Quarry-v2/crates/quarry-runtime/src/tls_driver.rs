//! TLS-profile driver backed by rquest upstream (`wreq`) and BoringSSL.
//!
//! The old implementation used `reqwest` + `rustls` with browser-like headers.
//! This adapter delegates to `quarry-tls`, which configures `wreq::Emulation`
//! for browser-family TLS and HTTP/2 fingerprints while preserving the runtime
//! `Driver` trait.

use std::time::Duration;

use async_trait::async_trait;
use quarry_core::identity;
use quarry_core::output::DriverKind;
use quarry_core::QuarryResult;
pub use quarry_tls::TlsProfile;
use quarry_tls::{TlsClientConfig, TlsDnsPin, WreqTlsClient};
use url::Url;

use crate::driver::{Driver, FetchHints};
use crate::fallback_driver::fetch_sub_codes_enabled;
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
    /// Under `QUARRY_EDGE__UNIFIED_UA` the identity is made Chromium-only and
    /// carries the product token (see `resolve_identity`).
    pub fn new(
        profile: TlsProfile,
        timeout: Duration,
        user_agent_override: Option<&str>,
    ) -> QuarryResult<Self> {
        let (profile, user_agent_override) =
            resolve_identity(profile, user_agent_override, identity::unified_enabled());
        let base_config = TlsClientConfig::default()
            .with_profile(profile)
            .with_timeout(timeout);
        let config = match user_agent_override {
            Some(user_agent) => base_config.with_user_agent_override(user_agent),
            None => base_config,
        };
        // E3.8.1: the client names a failed send's transport sub-code when the flag is on.
        let client = WreqTlsClient::new(config)?.with_fetch_sub_codes(fetch_sub_codes_enabled());
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
        captures: None,
    }
}

/// The identity a TLS driver presents, from the caller's profile and user agent override.
///
/// With `unified` off the caller's choice is returned as it is. With it on (ADR 5, D-W5-2:
/// never present a non-Chromium identity) a Firefox or Safari profile is driven as Chrome,
/// so the TLS emulation matches the user agent; a caller override is kept and gets the
/// product token unless it already names `QuarryBot`; and with no override the user agent is
/// the Chrome user agent with the token. The quarry-tls profile presets are not changed here.
fn resolve_identity(
    profile: TlsProfile,
    user_agent_override: Option<&str>,
    unified: bool,
) -> (TlsProfile, Option<String>) {
    if !unified {
        return (profile, user_agent_override.map(str::to_owned));
    }
    let profile = match profile {
        TlsProfile::Chrome => profile,
        other => {
            tracing::warn!(
                profile = other.as_str(),
                "tls profile is not chromium; the driver uses the chrome profile instead"
            );
            TlsProfile::Chrome
        }
    };
    let user_agent = match user_agent_override {
        Some(user_agent) if user_agent.contains(identity::PRODUCT_TOKEN) => user_agent.to_owned(),
        Some(user_agent) => identity::append_product_token(user_agent),
        None => identity::tls_user_agent(TlsProfile::Chrome.user_agent()),
    };
    (profile, Some(user_agent))
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

    /// The switch value goes through the production parser, so no test touches the
    /// process environment.
    fn switch(raw: Option<&str>) -> bool {
        identity::parse_unified(raw)
    }

    #[test]
    fn unified_off_keeps_the_caller_identity() {
        let off = switch(None);
        for profile in [TlsProfile::Chrome, TlsProfile::Firefox, TlsProfile::Safari] {
            assert_eq!(resolve_identity(profile, None, off), (profile, None));
        }
        assert_eq!(
            resolve_identity(TlsProfile::Firefox, Some("CustomBot/1.0"), off),
            (TlsProfile::Firefox, Some("CustomBot/1.0".to_owned()))
        );
    }

    #[test]
    fn unified_on_never_drives_firefox_or_safari() {
        let on = switch(Some("on"));
        for profile in [TlsProfile::Firefox, TlsProfile::Safari] {
            let (resolved, _) = resolve_identity(profile, None, on);
            assert_eq!(resolved, TlsProfile::Chrome);
            let (resolved, _) = resolve_identity(profile, Some("CustomBot/1.0"), on);
            assert_eq!(resolved, TlsProfile::Chrome);
        }
        let (resolved, _) = resolve_identity(TlsProfile::Chrome, None, on);
        assert_eq!(resolved, TlsProfile::Chrome);
    }

    #[test]
    fn unified_on_defaults_to_the_chrome_user_agent_with_the_token() {
        let on = switch(Some("1"));
        let chrome = TlsProfile::Chrome.user_agent();
        let (_, user_agent) = resolve_identity(TlsProfile::Firefox, None, on);
        assert_eq!(
            user_agent,
            Some(format!("{chrome} {}", identity::HONEST_USER_AGENT))
        );
    }

    #[test]
    fn unified_on_appends_the_token_to_a_caller_override_without_it() {
        let on = switch(Some("yes"));
        let (profile, user_agent) = resolve_identity(TlsProfile::Chrome, Some("CustomBot/1.0"), on);
        assert_eq!(profile, TlsProfile::Chrome);
        assert_eq!(
            user_agent.as_deref(),
            Some("CustomBot/1.0 QuarryBot/2.0 (+https://verevon.io/bot)")
        );
        let (_, blank) = resolve_identity(TlsProfile::Chrome, Some("   "), on);
        assert_eq!(blank.as_deref(), Some(identity::HONEST_USER_AGENT));
    }

    #[test]
    fn unified_on_keeps_a_caller_override_that_already_names_the_token() {
        let on = switch(Some("true"));
        let named = "Probe QuarryBot/1.0";
        let (_, user_agent) = resolve_identity(TlsProfile::Chrome, Some(named), on);
        assert_eq!(user_agent.as_deref(), Some(named));
    }
}
