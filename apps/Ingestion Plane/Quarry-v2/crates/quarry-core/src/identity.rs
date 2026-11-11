//! The one product identity Quarry presents (ADR 5, D-W5-1).
//!
//! Every user agent, robots token and bot-information URL Quarry sends is defined here and
//! nowhere else. The honest identity is [`HONEST_USER_AGENT`]; the legacy strings are kept
//! verbatim so that with `QUARRY_EDGE__UNIFIED_UA` off every caller sends exactly what it sent
//! before.
//!
//! Browser tiers never present a non-Chromium identity: they keep the browser's own native
//! user agent and append [`HONEST_USER_AGENT`] to it ([`append_product_token`]).

/// The product token robots.txt groups and log readers match on.
pub const PRODUCT_TOKEN: &str = "QuarryBot";
/// The product version carried in the user agent.
pub const PRODUCT_VERSION: &str = "2.0";
/// Where a site operator reads what the crawler is and how to stop it.
pub const BOT_INFO_URL: &str = "https://verevon.io/bot";
/// The single honest user agent (D-W5-1).
pub const HONEST_USER_AGENT: &str = "QuarryBot/2.0 (+https://verevon.io/bot)";
/// The robots.txt tokens Quarry obeys; the most restrictive answer wins.
pub const ROBOTS_TOKENS: [&str; 2] = ["quarrybot", "quarry"];
/// The token the discovery fetcher names itself with.
pub const DISCOVERY_TOKEN: &str = PRODUCT_TOKEN;

/// Legacy edge user agent, kept byte-identical for the switch-off path.
pub const LEGACY_EDGE_USER_AGENT: &str = "Quarry/2.0 (+https://triodelab.com/quarry)";
/// Legacy fetch-policy user agent, kept byte-identical for the switch-off path.
pub const LEGACY_POLICY_USER_AGENT: &str = "Quarry/2.0";
/// Legacy HTTP/3 driver user agent, kept byte-identical for the switch-off path.
pub const LEGACY_HTTP3_USER_AGENT: &str =
    "Mozilla/5.0 (compatible; QuarryBot/1.0; +https://verevon.io/bot)";

/// The switch that moves the edge, policy and HTTP/3 user agents to [`HONEST_USER_AGENT`].
pub const UNIFIED_UA_ENV: &str = "QUARRY_EDGE__UNIFIED_UA";

/// Whether a raw switch value turns the unified user agent on: `1`, `true`, `yes` or `on`,
/// trimmed, any case. Unset or anything else is off.
pub fn parse_unified(raw: Option<&str>) -> bool {
    matches!(
        raw.map(|value| value.trim().to_ascii_lowercase())
            .as_deref(),
        Some("1" | "true" | "yes" | "on")
    )
}

/// Whether `QUARRY_EDGE__UNIFIED_UA` is on. Read once per process (re-read on every call under
/// `cfg(test)`).
pub fn unified_enabled() -> bool {
    #[cfg(test)]
    {
        parse_unified(std::env::var(UNIFIED_UA_ENV).ok().as_deref())
    }
    #[cfg(not(test))]
    {
        static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *ENABLED.get_or_init(|| parse_unified(std::env::var(UNIFIED_UA_ENV).ok().as_deref()))
    }
}

/// The edge's user agent: honest when the switch is on, the legacy string otherwise.
pub fn edge_user_agent() -> &'static str {
    if unified_enabled() {
        HONEST_USER_AGENT
    } else {
        LEGACY_EDGE_USER_AGENT
    }
}

/// The fetch policy's user agent: honest when the switch is on, the legacy string otherwise.
pub fn policy_user_agent() -> &'static str {
    if unified_enabled() {
        HONEST_USER_AGENT
    } else {
        LEGACY_POLICY_USER_AGENT
    }
}

/// The HTTP/3 driver's user agent: honest when the switch is on, the legacy string otherwise.
pub fn http3_user_agent() -> &'static str {
    if unified_enabled() {
        HONEST_USER_AGENT
    } else {
        LEGACY_HTTP3_USER_AGENT
    }
}

/// The TLS tier's user agent: the Chrome user agent with the product token appended.
pub fn tls_user_agent(chrome_ua: &str) -> String {
    append_product_token(chrome_ua)
}

/// `native` with ` QuarryBot/2.0 (+https://verevon.io/bot)` appended, unless it already ends
/// with the token. The native string is otherwise kept as it is (D-W5-2: never a non-Chromium
/// identity, never a different browser).
pub fn append_product_token(native: &str) -> String {
    let trimmed = native.trim_end();
    if trimmed.ends_with(HONEST_USER_AGENT) {
        return trimmed.to_owned();
    }
    if trimmed.is_empty() {
        return HONEST_USER_AGENT.to_owned();
    }
    format!("{trimmed} {HONEST_USER_AGENT}")
}

/// `ua` without a trailing product token, so a user agent seen with the token and one seen
/// without it compare equal.
pub fn strip_product_token(ua: &str) -> &str {
    let trimmed = ua.trim_end();
    match trimmed.strip_suffix(HONEST_USER_AGENT) {
        Some(rest) => rest.trim_end(),
        None => trimmed,
    }
}
