//! Client contract between the gateway and the Verevon SPA (WS07-S3).
//!
//! The SPA is a static bundle that an open tab keeps running across a deploy, so the
//! gateway publishes which contract it speaks and which oldest client contract it
//! still accepts (`GET /api/v1/client-config`). The SPA compares the pair with its
//! own compiled-in constants and shows a banner when it is too old.
//!
//! Parity: `CLIENT_CONTRACT` and `CLIENT_MIN_CONTRACT` are mirrored by
//! `src/shared/build/client-contract.ts` and by `clientContract` in `vite.config.ts`
//! (written to `version.json`). `apps/gateway/scripts/client-contract-parity.test.mjs`
//! reads all three and fails when they disagree. Raise a number here and in both
//! mirrors in the same change.
//!
//! Nothing here is an authorization input: the policy only decides what the browser
//! shows or does about a stale tab. The gateway stores nothing about it.

use std::env;

/// The contract revision this gateway build speaks.
pub(crate) const CLIENT_CONTRACT: u32 = 1;

/// The oldest client contract this gateway build still serves. `CLIENT_MIN_CONTRACT` in
/// the environment may raise it (to force stale tabs onto the banner) but never lower it.
pub(crate) const CLIENT_MIN_CONTRACT: u32 = 1;

/// `CLIENT_VERSION_POLICY`: what a stale client does.
///
/// - `off`: counters only; the SPA shows nothing.
/// - `banner` (default): the SPA shows a non-blocking banner.
/// - `enforce`: the SPA may reload itself, but only when it holds no unsaved work; a tab
///   with no registered unsaved-work probe behaves as `banner` (decided in the SPA).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ClientVersionPolicy {
    Off,
    #[default]
    Banner,
    Enforce,
}

impl ClientVersionPolicy {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Banner => "banner",
            Self::Enforce => "enforce",
        }
    }

    /// Unset or unknown text is `banner`: a typo must neither silence the notice nor
    /// start reloading tabs.
    pub(crate) fn parse(value: Option<&str>) -> Self {
        match value.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
            Some("off") => Self::Off,
            Some("enforce") => Self::Enforce,
            _ => Self::Banner,
        }
    }
}

/// The client-contract settings, read once in `build_state` (never in a handler).
#[derive(Clone, Debug)]
pub(crate) struct ClientPolicy {
    pub(crate) policy: ClientVersionPolicy,
    pub(crate) min_contract: u32,
    /// `SOURCE_REVISION` baked into the image, `unverified` when unset.
    pub(crate) revision: String,
    /// `BUILD_DATE` baked into the image, `unknown` when unset.
    pub(crate) build_date: String,
}

impl Default for ClientPolicy {
    fn default() -> Self {
        Self {
            policy: ClientVersionPolicy::default(),
            min_contract: CLIENT_MIN_CONTRACT,
            revision: "unverified".to_owned(),
            build_date: "unknown".to_owned(),
        }
    }
}

impl ClientPolicy {
    pub(crate) fn from_env() -> Self {
        let text = |key: &str, fallback: &str| {
            env::var(key)
                .ok()
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| fallback.to_owned())
        };
        Self {
            policy: ClientVersionPolicy::parse(env::var("CLIENT_VERSION_POLICY").ok().as_deref()),
            min_contract: min_contract_from(env::var("CLIENT_MIN_CONTRACT").ok().as_deref()),
            revision: text("SOURCE_REVISION", "unverified"),
            build_date: text("BUILD_DATE", "unknown"),
        }
    }
}

/// The raised minimum from `CLIENT_MIN_CONTRACT`, never below the compiled-in floor and
/// never an unparsable value.
pub(crate) fn min_contract_from(value: Option<&str>) -> u32 {
    value
        .and_then(|v| v.trim().parse::<u32>().ok())
        .map_or(CLIENT_MIN_CONTRACT, |v| v.max(CLIENT_MIN_CONTRACT))
}
