//! Secret redaction at capture (WS12): one deterministic, versioned rule module.
//!
//! Everything Quarry returns, stores or emits goes through the same rules, so a
//! password, an OTP, a card number or a token that a page displayed does not
//! reach a caller, a receipt, an event, a log or a model prompt. See
//! `docs/SECRET_REDACTION.md` for the rule table, the version policy and the
//! limits.
//!
//! This module is a pure library: no I/O, no logging, no persistence, no
//! environment reads. Nothing in Quarry calls it yet (WS12.S1 to S6 wire it).
//!
//! * [`Profile::Live`] is applied to everything returned to any caller, ZDR
//!   included: field-semantic values and high-confidence token shapes.
//! * [`Profile::Durable`] is applied to everything kept or emitted beyond the
//!   caller (receipts, checkpoints, events, JSON artifacts, logs, error text).
//!   It is a superset of Live: it adds contextual rules (a labelled secret
//!   assignment, a numeric OTP beside an OTP label, a Luhn-valid card number)
//!   and URL query and fragment secrets.
//!
//! The marker that replaces a secret is `[redacted:<kind>]`. The rule set is
//! identified by [`RULE_VERSION`]; a change to a rule bumps it (see
//! [`rule_fingerprint`] for what the drift test can and cannot see).

pub mod field;
pub mod json;
pub mod luhn;
mod shapes;
pub mod stamp;
pub mod url;

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use field::{classify, classify_field, FieldFacts, FieldKind};
pub use stamp::{ProbeState, RedactionStamp, ScreenshotState};

/// Identifies the rule set. Any change to a pattern, a rule, a key list, a field
/// label or a threshold bumps this (the `redaction_rule_drift_requires_a_version_bump`
/// test fails otherwise, for those and for the behaviours in the fingerprint's
/// probe corpus), and the golden, its pin and every vendored copy are
/// regenerated in the same change.
pub const RULE_VERSION: &str = "qr-redact/3";

/// Every rule id that can appear in a [`Tally`] or a stamp.
pub const RULE_IDS: &[&str] = &[
    "field.password",
    "field.otp",
    "field.card",
    "field.phone",
    "field.label",
    "field.luhn",
    "shape.jwt",
    "shape.pem",
    "shape.aws",
    "shape.github",
    "shape.gitlab",
    "shape.npm",
    "shape.slack",
    "shape.google",
    "shape.openai",
    "shape.stripe",
    "shape.whsec",
    "shape.bearer",
    "shape.basic",
    "shape.userinfo",
    "ctx.labelled_secret",
    "ctx.otp_numeric",
    "ctx.pan",
    "url.query",
    "url.fragment",
    "json.key",
];

/// Which rules apply.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    /// Everything returned to any caller: field values and high-confidence shapes.
    Live,
    /// Everything kept or emitted beyond the caller. Superset of `Live`.
    #[default]
    Durable,
}

/// What the process does with a match. Set once at boot by [`init_mode`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Replace matches with markers.
    #[default]
    Enforce,
    /// Count what would be replaced, return the input unchanged. Staging only.
    Shadow,
    /// Do nothing.
    Off,
}

impl Mode {
    /// Parse `enforce`, `shadow` or `off` (case-insensitive). The edge reads
    /// its own configuration and calls this; the library never reads env.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "enforce" => Some(Self::Enforce),
            "shadow" => Some(Self::Shadow),
            "off" => Some(Self::Off),
            _ => None,
        }
    }

    /// `ENVIRONMENT=production` accepts only `Enforce`.
    #[must_use]
    pub const fn permitted_in_production(self) -> bool {
        matches!(self, Self::Enforce)
    }
}

static MODE: OnceLock<Mode> = OnceLock::new();

/// Set the process-wide mode. Callable once, before anything redacts. Returns
/// the mode already in force when it was set or read before: the first read
/// pins the default, so a process never changes mode while it is serving.
///
/// # Errors
/// The mode that is already in force.
pub fn init_mode(mode: Mode) -> Result<(), Mode> {
    MODE.set(mode).map_err(|_| self::mode())
}

/// The process-wide mode (default [`Mode::Enforce`]). The first call pins it.
#[must_use]
pub fn mode() -> Mode {
    *MODE.get_or_init(Mode::default)
}

/// The kind named in a marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MarkerKind {
    Password,
    Otp,
    Card,
    Phone,
    Secret,
    Token,
    Key,
    Credential,
}

impl MarkerKind {
    /// The word inside the marker.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Password => "password",
            Self::Otp => "otp",
            Self::Card => "card",
            Self::Phone => "phone",
            Self::Secret => "secret",
            Self::Token => "token",
            Self::Key => "key",
            Self::Credential => "credential",
        }
    }
}

/// The marker text for a kind: `[redacted:<kind>]`.
#[must_use]
pub const fn mask(kind: MarkerKind) -> &'static str {
    match kind {
        MarkerKind::Password => "[redacted:password]",
        MarkerKind::Otp => "[redacted:otp]",
        MarkerKind::Card => "[redacted:card]",
        MarkerKind::Phone => "[redacted:phone]",
        MarkerKind::Secret => "[redacted:secret]",
        MarkerKind::Token => "[redacted:token]",
        MarkerKind::Key => "[redacted:key]",
        MarkerKind::Credential => "[redacted:credential]",
    }
}

/// Prefix every marker starts with. Every rule leaves text that starts with it
/// alone, which is what makes redaction idempotent.
pub(crate) const MARKER_PREFIX: &str = "[redacted:";

/// Is this text exactly one marker?
#[must_use]
pub fn is_marker(s: &str) -> bool {
    s.strip_prefix(MARKER_PREFIX)
        .and_then(|rest| rest.strip_suffix(']'))
        .is_some_and(|kind| !kind.is_empty() && kind.bytes().all(|b| b.is_ascii_lowercase()))
}

/// What a pass masked: counts per rule id. It holds no content and no lengths.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tally {
    values_masked: u32,
    by_rule: BTreeMap<&'static str, u32>,
}

impl Tally {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one masked value under a rule id from [`RULE_IDS`].
    pub fn add(&mut self, rule: &'static str) {
        debug_assert!(RULE_IDS.contains(&rule), "unknown rule id {rule}");
        self.values_masked = self.values_masked.saturating_add(1);
        let n = self.by_rule.entry(rule).or_insert(0);
        *n = n.saturating_add(1);
    }

    /// How many values were masked.
    #[must_use]
    pub const fn values_masked(&self) -> u32 {
        self.values_masked
    }

    /// Masked values per rule id.
    #[must_use]
    pub const fn by_rule(&self) -> &BTreeMap<&'static str, u32> {
        &self.by_rule
    }

    /// Nothing was masked.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.values_masked == 0
    }

    /// Add another tally's counts to this one.
    pub fn merge(&mut self, other: &Self) {
        self.values_masked = self.values_masked.saturating_add(other.values_masked);
        for (rule, n) in &other.by_rule {
            let e = self.by_rule.entry(rule).or_insert(0);
            *e = e.saturating_add(*n);
        }
    }

    /// The stamp that records this pass.
    #[must_use]
    pub fn stamp(&self, profile: Profile, mode: Mode) -> RedactionStamp {
        RedactionStamp::from_tally(self, profile, mode)
    }
}

/// Redact free text under the process-wide [`mode`].
#[must_use]
pub fn redact_text<'a>(input: &'a str, profile: Profile, tally: &mut Tally) -> Cow<'a, str> {
    redact_text_with(mode(), input, profile, tally)
}

/// Redact free text under an explicit mode. `Shadow` counts and returns the
/// input unchanged; `Off` does nothing.
#[must_use]
pub fn redact_text_with<'a>(
    mode: Mode,
    input: &'a str,
    profile: Profile,
    tally: &mut Tally,
) -> Cow<'a, str> {
    match mode {
        Mode::Off => Cow::Borrowed(input),
        Mode::Shadow => {
            let _ = shapes::redact(input, profile, tally);
            Cow::Borrowed(input)
        }
        Mode::Enforce => shapes::redact(input, profile, tally),
    }
}

/// Redact every string leaf of a JSON value (and, under `Durable`, the value
/// of every object key that names a secret) under the process-wide [`mode`].
pub fn redact_json(value: &mut Value, profile: Profile, tally: &mut Tally) {
    redact_json_with(mode(), value, profile, tally);
}

/// [`redact_json`] under an explicit mode.
pub fn redact_json_with(mode: Mode, value: &mut Value, profile: Profile, tally: &mut Tally) {
    match mode {
        Mode::Off => {}
        Mode::Shadow => {
            let mut scratch = value.clone();
            json::redact_value(&mut scratch, profile, tally);
        }
        Mode::Enforce => json::redact_value(value, profile, tally),
    }
}

/// Redact a URL under the process-wide [`mode`]: it keeps the scheme, host,
/// path and key names and masks userinfo passwords and (under `Durable`)
/// secret query and fragment values. Input that is not an absolute URL goes
/// through [`redact_text`].
#[must_use]
pub fn redact_url<'a>(input: &'a str, profile: Profile, tally: &mut Tally) -> Cow<'a, str> {
    redact_url_with(mode(), input, profile, tally)
}

/// [`redact_url`] under an explicit mode.
#[must_use]
pub fn redact_url_with<'a>(
    mode: Mode,
    input: &'a str,
    profile: Profile,
    tally: &mut Tally,
) -> Cow<'a, str> {
    match mode {
        Mode::Off => Cow::Borrowed(input),
        Mode::Shadow => {
            let _ = url::redact_url_enforce(input, profile, tally);
            Cow::Borrowed(input)
        }
        Mode::Enforce => url::redact_url_enforce(input, profile, tally),
    }
}

/// A digest of every rule: ids, kinds, profiles, patterns, key lists, field
/// labels, thresholds, the card-number table, and what the rules do to a fixed
/// probe corpus (the verify functions, the card group-size rule, the JSON header
/// pairs). The drift test pins it per [`RULE_VERSION`], so a rule cannot change
/// without a version bump. A behaviour change that no probe reaches is caught
/// by the golden cases, not by the fingerprint.
#[must_use]
pub fn rule_fingerprint() -> String {
    let mut h = blake3::Hasher::new();
    let mut feed = |part: &str| {
        h.update(&(part.len() as u64).to_le_bytes());
        h.update(part.as_bytes());
    };
    feed(MARKER_PREFIX);
    for id in RULE_IDS {
        feed(id);
    }
    for part in shapes::fingerprint_parts()
        .into_iter()
        .chain(field::fingerprint_parts())
        .chain(json::fingerprint_parts())
        .chain(url::fingerprint_parts())
        .chain(std::iter::once(luhn_behaviour_digest()))
        .chain(std::iter::once(luhn::group_behaviour_probe()))
    {
        feed(&part);
    }
    h.finalize().to_hex().to_string()
}

/// How `is_pan` answers for every issuer prefix and length, as a string of bits.
/// Part of the fingerprint so an edit to the card table counts as a rule change.
fn luhn_behaviour_digest() -> String {
    let mut out = String::new();
    for prefix in 0..100u32 {
        for len in 13..=19usize {
            let mut body = format!("{prefix:02}");
            while body.len() < len - 1 {
                body.push('0');
            }
            // Pick the check digit that makes the Luhn sum pass.
            let digit = (0..10u8)
                .find(|d| luhn::luhn_ok(&format!("{body}{d}")))
                .unwrap_or(0);
            out.push(if luhn::is_pan(&format!("{body}{digit}")) {
                '1'
            } else {
                '0'
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_have_the_documented_shape() {
        for kind in [
            MarkerKind::Password,
            MarkerKind::Otp,
            MarkerKind::Card,
            MarkerKind::Phone,
            MarkerKind::Secret,
            MarkerKind::Token,
            MarkerKind::Key,
            MarkerKind::Credential,
        ] {
            assert_eq!(mask(kind), format!("[redacted:{}]", kind.as_str()));
            assert!(is_marker(mask(kind)));
        }
        assert!(!is_marker("[redacted:]"));
        assert!(!is_marker("[redacted:x] tail"));
        assert!(!is_marker("plain"));
    }

    #[test]
    fn mode_parses_and_production_accepts_only_enforce() {
        assert_eq!(Mode::parse(" Enforce "), Some(Mode::Enforce));
        assert_eq!(Mode::parse("SHADOW"), Some(Mode::Shadow));
        assert_eq!(Mode::parse("off"), Some(Mode::Off));
        assert_eq!(Mode::parse("on"), None);
        assert!(Mode::Enforce.permitted_in_production());
        assert!(!Mode::Shadow.permitted_in_production());
        assert!(!Mode::Off.permitted_in_production());
    }

    /// The only test in this binary that touches the process-wide mode; every
    /// other test passes its mode explicitly. (The default-and-pin half is
    /// tested in `tests/redaction_golden.rs`, which is a separate process.)
    #[test]
    fn init_mode_sets_once() {
        assert_eq!(init_mode(Mode::Shadow), Ok(()));
        assert_eq!(mode(), Mode::Shadow);
        assert_eq!(
            init_mode(Mode::Enforce),
            Err(Mode::Shadow),
            "a second init is refused"
        );
        assert_eq!(mode(), Mode::Shadow);
    }

    #[test]
    fn tally_counts_by_rule_and_merges() {
        let mut a = Tally::new();
        assert!(a.is_empty());
        a.add("shape.jwt");
        a.add("shape.jwt");
        a.add("ctx.pan");
        let mut b = Tally::new();
        b.add("ctx.pan");
        a.merge(&b);
        assert_eq!(a.values_masked(), 4);
        assert_eq!(a.by_rule().get("shape.jwt"), Some(&2));
        assert_eq!(a.by_rule().get("ctx.pan"), Some(&2));
    }

    #[test]
    fn fingerprint_is_stable_within_a_process() {
        assert_eq!(rule_fingerprint(), rule_fingerprint());
        assert_eq!(rule_fingerprint().len(), 64);
    }
}
