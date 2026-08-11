//! Browser-like TLS transport built on the rquest upstream `wreq` crate.
//!
//! `wreq` gives Quarry a BoringSSL-backed client with explicit TLS and HTTP/2
//! emulation controls. This crate keeps that dependency behind a narrow adapter
//! so runtime code can keep using Quarry-native request/response types.

use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;
use std::time::{Duration, Instant};

// Not currently wired into this crate's own client (see `.redirect(redirect::Policy::none())`
// below, which disables auto-follow and forces callers to re-validate each hop as a fresh
// request -- already the safer pattern). Declared here purely so the module is compiled and
// its tests run rather than sitting as an unreachable, unverified orphan; it was previously
// not declared in any `mod` at all.
mod redirect_policy;
pub use redirect_policy::QuarryRedirectPolicy;

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_security::heur::resolve_guard;
use serde::{Deserialize, Serialize};
use url::Url;
use wreq::{
    header::{self, HeaderMap, HeaderValue, OrigHeaderMap},
    http2::{Http2Options, PseudoId, PseudoOrder},
    redirect,
    tls::{AlpnProtocol, TlsOptions, TlsVersion},
    Client, Emulation,
};

/// Semantic browser-family TLS profile.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TlsProfile {
    #[default]
    Chrome,
    Firefox,
    Safari,
}

impl TlsProfile {
    /// Compatibility alias for the legacy runtime profile name.
    #[allow(non_upper_case_globals)]
    pub const Chrome120: Self = TlsProfile::Chrome;
    /// Compatibility alias for the legacy runtime profile name.
    #[allow(non_upper_case_globals)]
    pub const Firefox121: Self = TlsProfile::Firefox;
    /// Compatibility alias for the legacy runtime profile name.
    #[allow(non_upper_case_globals)]
    pub const Safari17: Self = TlsProfile::Safari;

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            TlsProfile::Chrome => "chrome",
            TlsProfile::Firefox => "firefox",
            TlsProfile::Safari => "safari",
        }
    }

    #[must_use]
    pub fn user_agent(self) -> &'static str {
        match self {
            TlsProfile::Chrome => {
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36"
            }
            TlsProfile::Firefox => {
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:132.0) Gecko/20100101 Firefox/132.0"
            }
            TlsProfile::Safari => {
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_7) AppleWebKit/605.1.15 \
                 (KHTML, like Gecko) Version/17.6 Safari/605.1.15"
            }
        }
    }

    #[must_use]
    pub fn accept_language(self) -> &'static str {
        match self {
            TlsProfile::Firefox => "en-US,en;q=0.5",
            TlsProfile::Chrome | TlsProfile::Safari => "en-US,en;q=0.9",
        }
    }
}

impl FromStr for TlsProfile {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" | "chrome" | "chrome_latest" | "chrome131" | "chrome_131" => {
                Ok(TlsProfile::Chrome)
            }
            "firefox" | "firefox_latest" | "firefox132" | "firefox_132" => Ok(TlsProfile::Firefox),
            "safari" | "safari_latest" | "safari17" | "safari_17" => Ok(TlsProfile::Safari),
            other => Err(format!("unsupported tls profile: {other}")),
        }
    }
}

/// Configuration for a `wreq` TLS impersonation client.
#[derive(Debug, Clone)]
pub struct TlsClientConfig {
    /// Browser-family TLS/HTTP2 profile to emulate.
    pub profile: TlsProfile,
    /// End-to-end request timeout. Values below 100ms are rejected because a
    /// DNS/TCP/TLS handshake cannot complete reliably under that floor.
    pub timeout: Duration,
    /// Optional HTTP `User-Agent` override. This does not change the TLS
    /// fingerprint and is therefore warned about at client construction time.
    pub user_agent_override: Option<String>,
    /// Optional `Accept-Language` override.
    pub accept_language_override: Option<String>,
    /// Prefer HTTP/3 (QUIC) when the upstream advertises it via Alt-Svc.
    ///
    /// Off by default. Currently a **no-op** at the wire level because
    /// `wreq` 6.0.0-rc.28 does not expose an `http3` feature in a
    /// stable form — we keep the toggle here so callers can opt in
    /// today and the upgrade to wreq stable wires this through without
    /// a public-API change. When set, [`WreqTlsClient::new`] logs a
    /// `tracing::info!` so operators can see when the request would
    /// have used QUIC if the runtime supported it.
    pub prefer_http3: bool,
}

impl Default for TlsClientConfig {
    fn default() -> Self {
        Self {
            profile: TlsProfile::default(),
            timeout: Duration::from_secs(30),
            user_agent_override: None,
            accept_language_override: None,
            prefer_http3: false,
        }
    }
}

impl TlsClientConfig {
    #[must_use]
    pub fn with_profile(self, profile: TlsProfile) -> Self {
        Self { profile, ..self }
    }

    #[must_use]
    pub fn with_timeout(self, timeout: Duration) -> Self {
        Self { timeout, ..self }
    }

    /// Override the HTTP `User-Agent` header.
    ///
    /// This can make HTTP headers inconsistent with the selected TLS profile.
    /// Use only for compatibility with a known target, not as a stealth knob.
    #[must_use]
    pub fn with_user_agent_override(self, value: impl Into<String>) -> Self {
        Self {
            user_agent_override: Some(value.into()),
            ..self
        }
    }

    #[must_use]
    pub fn with_accept_language_override(self, value: impl Into<String>) -> Self {
        Self {
            accept_language_override: Some(value.into()),
            ..self
        }
    }

    /// Opt in to HTTP/3 (QUIC) when the upstream advertises it.
    ///
    /// See [`TlsClientConfig::prefer_http3`] for the current runtime
    /// limitation. Setting this today is forward-compatible: when
    /// `wreq` exposes an HTTP/3 feature in stable form, this flag will
    /// flip the wire-level transport without an API change.
    #[must_use]
    pub fn with_prefer_http3(self, prefer: bool) -> Self {
        Self {
            prefer_http3: prefer,
            ..self
        }
    }

    fn user_agent(&self) -> &str {
        self.user_agent_override
            .as_deref()
            .unwrap_or_else(|| self.profile.user_agent())
    }

    fn accept_language(&self) -> &str {
        self.accept_language_override
            .as_deref()
            .unwrap_or_else(|| self.profile.accept_language())
    }
}

/// Response from a TLS-impersonated fetch.
#[derive(Debug, Clone)]
pub struct TlsFetchResponse {
    pub status: u16,
    pub final_url: Url,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub duration_ms: u64,
}

/// Vetted target addresses for one TLS-profile request.
///
/// The caller keeps using the origin hostname in the request URL, so wreq
/// still emits the expected Host header and TLS SNI. This pin changes only the
/// socket destination and deliberately admits no fallback DNS resolution.
#[derive(Debug, Clone)]
pub struct TlsDnsPin {
    host: String,
    addresses: Vec<SocketAddr>,
}

impl TlsDnsPin {
    pub fn new(host: impl Into<String>, addresses: Vec<SocketAddr>) -> QuarryResult<Self> {
        let host = host.into().trim_end_matches('.').to_ascii_lowercase();
        if host.is_empty() {
            return Err(QuarryError::new(
                ErrorCode::BadRequest,
                "TLS DNS pin is missing a host",
            ));
        }
        if addresses.is_empty() {
            return Err(QuarryError::new(
                ErrorCode::BadRequest,
                "TLS DNS pin has no addresses",
            ));
        }
        let ips: Vec<IpAddr> = addresses.iter().map(SocketAddr::ip).collect();
        if let Some(reason) = resolve_guard(&ips) {
            return Err(QuarryError::new(ErrorCode::SecurityBlocked, reason));
        }
        Ok(Self { host, addresses })
    }

    fn applies_to(&self, url: &Url) -> bool {
        url.host_str()
            .is_some_and(|host| host.trim_end_matches('.').eq_ignore_ascii_case(&self.host))
    }
}

/// BoringSSL-backed HTTP client with browser-family TLS and HTTP/2 emulation.
///
/// The adapter does not follow redirects. Quarry must validate each redirect
/// target through SSRF/DNS preflight before redirect-following can be enabled.
pub struct WreqTlsClient {
    profile: TlsProfile,
    config: TlsClientConfig,
    client: Client,
}

impl WreqTlsClient {
    /// Build a TLS-emulating client.
    ///
    /// Redirects are disabled deliberately; callers that need redirect support
    /// should route through a future DriverPlan redirect policy with per-hop
    /// security validation.
    pub fn new(config: TlsClientConfig) -> QuarryResult<Self> {
        validate_timeout(config.timeout)?;
        warn_on_user_agent_override(&config);
        if config.prefer_http3 {
            // wreq 6.0.0-rc.28 has no `http3` feature; emit an
            // operator-visible log so the gap is auditable instead of
            // silently dropped. The toggle stays so a future wreq
            // release can wire it through without touching the public
            // API.
            tracing::info!(
                profile = ?config.profile,
                "prefer_http3 requested but wreq 6.0.0-rc.x has no http3 feature; falling back to HTTP/2"
            );
        }
        let client = build_client(&config, None)?;

        Ok(Self {
            profile: config.profile,
            config,
            client,
        })
    }

    #[must_use]
    pub fn profile(&self) -> TlsProfile {
        self.profile
    }

    pub async fn fetch(&self, url: &Url) -> QuarryResult<TlsFetchResponse> {
        fetch_with_client(&self.client, url).await
    }

    /// Fetch with a connection-level DNS pin while preserving the request URL's
    /// hostname for TLS SNI and HTTP authority.
    pub async fn fetch_with_dns_pin(
        &self,
        url: &Url,
        pin: &TlsDnsPin,
    ) -> QuarryResult<TlsFetchResponse> {
        if !pin.applies_to(url) {
            return Err(QuarryError::new(
                ErrorCode::BadRequest,
                "TLS DNS pin does not match the requested host",
            ));
        }
        let client = build_client(&self.config, Some(pin))?;
        fetch_with_client(&client, url).await
    }
}

fn build_client(config: &TlsClientConfig, pin: Option<&TlsDnsPin>) -> QuarryResult<Client> {
    let emulation = browser_emulation(config)?;
    let mut builder = Client::builder()
        .timeout(config.timeout)
        .connect_timeout(config.timeout)
        .redirect(redirect::Policy::none())
        // A TLS-profile request is direct egress. Accepting a process/system
        // proxy here would delegate target DNS to a different authority and
        // nullify a caller-supplied connection pin.
        .no_proxy()
        .emulation(emulation);
    if let Some(pin) = pin {
        builder = builder.resolve_to_addrs(pin.host.clone(), pin.addresses.clone());
    }
    builder
        .build()
        .map_err(|e| QuarryError::new(ErrorCode::Internal, format!("wreq client: {e}")))
}

async fn fetch_with_client(client: &Client, url: &Url) -> QuarryResult<TlsFetchResponse> {
    let started = Instant::now();
    let resp = client
        .get(url.as_str())
        .send()
        .await
        .map_err(|e| map_wreq_error("tls fetch", e))?;

    let status = resp.status().as_u16();
    let final_url = match Url::parse(&resp.uri().to_string()) {
        Ok(parsed) => parsed,
        Err(err) => {
            tracing::warn!(error = %err, "failed to parse wreq response URI; falling back to requested URL");
            url.clone()
        }
    };
    let headers = resp
        .headers()
        .iter()
        .filter_map(|(key, value)| {
            value
                .to_str()
                .ok()
                .map(|text| (key.as_str().to_string(), text.to_string()))
        })
        .collect();
    let body = resp
        .bytes()
        .await
        .map_err(|e| map_wreq_error("tls body", e))?
        .to_vec();

    Ok(TlsFetchResponse {
        status,
        final_url,
        headers,
        body,
        duration_ms: started.elapsed().as_millis() as u64,
    })
}

fn browser_emulation(config: &TlsClientConfig) -> QuarryResult<Emulation> {
    Ok(Emulation::builder()
        .tls_options(tls_options(config.profile))
        .http2_options(http2_options(config.profile))
        .headers(profile_headers(config)?)
        .orig_headers(original_header_order())
        .build())
}

fn profile_headers(config: &TlsClientConfig) -> QuarryResult<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::USER_AGENT,
        header_value("user-agent", config.user_agent())?,
    );
    headers.insert(
        header::ACCEPT_LANGUAGE,
        header_value("accept-language", config.accept_language())?,
    );
    headers.insert(
        header::ACCEPT_ENCODING,
        HeaderValue::from_static("gzip, deflate, br, zstd"),
    );
    headers.insert(
        header::ACCEPT,
        HeaderValue::from_static(
            "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8",
        ),
    );
    Ok(headers)
}

fn header_value(name: &str, value: &str) -> QuarryResult<HeaderValue> {
    if !value.is_ascii()
        || value.contains('\r')
        || value.contains('\n')
        || value.contains('\u{2028}')
        || value.contains('\u{2029}')
    {
        return Err(QuarryError::new(
            ErrorCode::BadRequest,
            format!("invalid {name} header value: control or non-ascii character"),
        ));
    }
    HeaderValue::from_str(value).map_err(|e| {
        QuarryError::new(
            ErrorCode::BadRequest,
            format!("invalid {name} header value: {e}"),
        )
    })
}

fn validate_timeout(timeout: Duration) -> QuarryResult<()> {
    if timeout < Duration::from_millis(100) {
        return Err(QuarryError::new(
            ErrorCode::BadRequest,
            format!("tls timeout must be at least 100ms; got {timeout:?}"),
        ));
    }
    Ok(())
}

fn warn_on_user_agent_override(config: &TlsClientConfig) {
    if let Some(user_agent) = config.user_agent_override.as_deref() {
        tracing::warn!(
            profile = config.profile.as_str(),
            user_agent = user_agent,
            "user-agent override may not match the configured TLS fingerprint"
        );
    }
}

/// HTTP/1.1 header order for browser-like requests.
///
/// Real browsers emit request headers in stable orders. HTTP/2 pseudo-header
/// order is configured separately in `http2_options`.
fn original_header_order() -> OrigHeaderMap {
    let mut headers = OrigHeaderMap::new();
    headers.insert("Host");
    headers.insert("Connection");
    headers.insert("User-Agent");
    headers.insert("Accept");
    headers.insert("Accept-Language");
    headers.insert("Accept-Encoding");
    headers
}

/// TLS options that approximate browser-family ClientHello fingerprints.
///
/// These first-cut presets target the Chrome 131, Firefox 132, and Safari 17.6
/// browser families. They intentionally live in Quarry instead of `wreq-util`
/// until the profile-catalog dependency clears license review. Refresh them by
/// comparing real browser handshakes against fingerprint endpoints such as
/// tls.peet.ws and tls.browserleaks.com before production GA, then at least
/// monthly while `wreq` is exact-pinned to an RC.
fn tls_options(profile: TlsProfile) -> TlsOptions {
    let (curves, ciphers, sigalgs) = match profile {
        TlsProfile::Chrome => (
            "X25519:P-256:P-384",
            "TLS_AES_128_GCM_SHA256:TLS_AES_256_GCM_SHA384:TLS_CHACHA20_POLY1305_SHA256:TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256:TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256:TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384:TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384:TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256:TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256",
            "ecdsa_secp256r1_sha256:rsa_pss_rsae_sha256:rsa_pkcs1_sha256:ecdsa_secp384r1_sha384:rsa_pss_rsae_sha384:rsa_pkcs1_sha384:rsa_pss_rsae_sha512:rsa_pkcs1_sha512:rsa_pkcs1_sha1",
        ),
        TlsProfile::Firefox => (
            "X25519:P-256:P-384:P-521",
            "TLS_AES_128_GCM_SHA256:TLS_CHACHA20_POLY1305_SHA256:TLS_AES_256_GCM_SHA384:TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256:TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256:TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256:TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256:TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384:TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384",
            "ecdsa_secp256r1_sha256:ecdsa_secp384r1_sha384:ecdsa_secp521r1_sha512:rsa_pss_rsae_sha256:rsa_pss_rsae_sha384:rsa_pss_rsae_sha512:rsa_pkcs1_sha256:rsa_pkcs1_sha384:rsa_pkcs1_sha512",
        ),
        TlsProfile::Safari => (
            "X25519:P-256:P-384:P-521",
            "TLS_AES_128_GCM_SHA256:TLS_AES_256_GCM_SHA384:TLS_CHACHA20_POLY1305_SHA256:TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384:TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256:TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384:TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256:TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256:TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256",
            "ecdsa_secp256r1_sha256:rsa_pss_rsae_sha256:rsa_pkcs1_sha256:ecdsa_secp384r1_sha384:rsa_pss_rsae_sha384:rsa_pkcs1_sha384:rsa_pss_rsae_sha512:rsa_pkcs1_sha512",
        ),
    };

    TlsOptions::builder()
        .enable_ocsp_stapling(true)
        .enable_signed_cert_timestamps(true)
        .curves_list(curves)
        .cipher_list(ciphers)
        .sigalgs_list(sigalgs)
        .alpn_protocols([AlpnProtocol::HTTP2, AlpnProtocol::HTTP1])
        .min_tls_version(TlsVersion::TLS_1_2)
        .max_tls_version(TlsVersion::TLS_1_3)
        .build()
}

fn http2_options(profile: TlsProfile) -> Http2Options {
    let pseudo_order = match profile {
        TlsProfile::Firefox => PseudoOrder::builder()
            .extend([
                PseudoId::Method,
                PseudoId::Path,
                PseudoId::Authority,
                PseudoId::Scheme,
            ])
            .build(),
        TlsProfile::Chrome | TlsProfile::Safari => PseudoOrder::builder()
            .extend([
                PseudoId::Method,
                PseudoId::Authority,
                PseudoId::Scheme,
                PseudoId::Path,
            ])
            .build(),
    };

    Http2Options::builder()
        .initial_stream_id(3)
        .initial_window_size(16_777_216)
        .initial_connection_window_size(16_711_681 + 65_535)
        .headers_pseudo_order(pseudo_order)
        .build()
}

fn map_wreq_error(context: &str, err: wreq::Error) -> QuarryError {
    let (code, kind) = if err.is_timeout() {
        (ErrorCode::Timeout, "timeout")
    } else if err.is_redirect() || err.is_connect() {
        (ErrorCode::UpstreamBlocked, "upstream_blocked")
    } else {
        (ErrorCode::DriverFailed, "driver_failed")
    };
    tracing::debug!(context, error_kind = kind, "wreq request failed");
    QuarryError::new(code, format!("{context}: {kind}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, SocketAddr};

    #[test]
    fn profiles_have_distinct_user_agents() {
        let agents = [
            TlsProfile::Chrome.user_agent(),
            TlsProfile::Firefox.user_agent(),
            TlsProfile::Safari.user_agent(),
        ];
        let unique = agents.into_iter().collect::<std::collections::HashSet<_>>();
        assert_eq!(unique.len(), 3);
    }

    #[test]
    fn parses_profile_aliases() {
        assert_eq!("auto".parse::<TlsProfile>(), Ok(TlsProfile::Chrome));
        assert_eq!(
            "firefox_latest".parse::<TlsProfile>(),
            Ok(TlsProfile::Firefox)
        );
        assert_eq!("safari_17".parse::<TlsProfile>(), Ok(TlsProfile::Safari));
        assert!("lynx".parse::<TlsProfile>().is_err());
    }

    #[test]
    fn profile_strings_round_trip() {
        for profile in [TlsProfile::Chrome, TlsProfile::Firefox, TlsProfile::Safari] {
            assert_eq!(profile.as_str().parse::<TlsProfile>(), Ok(profile));
        }
    }

    #[test]
    fn accepts_user_agent_override() {
        let config = TlsClientConfig::default().with_user_agent_override("CustomBrowser/1.0");
        let headers = profile_headers(&config).expect("profile headers");
        assert_eq!(headers[header::USER_AGENT], "CustomBrowser/1.0");
    }

    #[test]
    fn dns_pin_rejects_private_or_empty_targets() {
        let private = TlsDnsPin::new(
            "customer.example",
            vec![SocketAddr::from((Ipv4Addr::LOCALHOST, 443))],
        )
        .expect_err("private pins must not be accepted");
        assert_eq!(private.code, ErrorCode::SecurityBlocked);

        let empty = TlsDnsPin::new("customer.example", Vec::new())
            .expect_err("a pin without addresses must fail closed");
        assert_eq!(empty.code, ErrorCode::BadRequest);
    }

    #[test]
    fn rejects_header_injection_override() {
        for value in [
            "Custom\r\nX-Evil: 1",
            "Custom\rX-Evil: 1",
            "Custom\nX-Evil: 1",
            "Custom\u{2028}X-Evil: 1",
        ] {
            let config = TlsClientConfig::default().with_user_agent_override(value);
            let err = profile_headers(&config).expect_err("header injection should be rejected");
            assert_eq!(err.code, ErrorCode::BadRequest);
        }
    }

    #[test]
    fn rejects_sub_100ms_timeout() {
        let result =
            WreqTlsClient::new(TlsClientConfig::default().with_timeout(Duration::from_millis(1)));
        assert!(matches!(result, Err(err) if err.code == ErrorCode::BadRequest));
    }

    #[test]
    fn builds_client_without_network() {
        let client = WreqTlsClient::new(
            TlsClientConfig::default()
                .with_profile(TlsProfile::Firefox)
                .with_timeout(Duration::from_secs(5)),
        )
        .expect("wreq client builds");
        assert_eq!(client.profile(), TlsProfile::Firefox);
    }

    #[tokio::test]
    async fn gated_peet_probe_smoke() {
        if std::env::var("QUARRY_NET_TESTS").ok().as_deref() != Some("1") {
            return;
        }
        let client = WreqTlsClient::new(
            TlsClientConfig::default()
                .with_profile(TlsProfile::Chrome)
                .with_timeout(Duration::from_secs(15)),
        )
        .expect("wreq client builds");
        let url = Url::parse("https://tls.peet.ws/api/all").expect("probe url");
        let resp = client.fetch(&url).await.expect("tls probe response");
        assert_eq!(resp.status, 200);
        let body = String::from_utf8_lossy(&resp.body);
        assert!(body.contains("tls") || body.contains("ja3") || body.contains("ja4"));
    }
}
