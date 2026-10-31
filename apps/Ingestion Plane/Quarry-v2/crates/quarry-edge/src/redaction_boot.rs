//! Boot policy for the secret-redaction mode (WS12.S2).
//!
//! `main` calls [`init_from_env`] once, before anything redacts, with whether
//! the environment is production. The mode is process-wide
//! (`quarry_core::redaction::init_mode`) and cannot change while the process
//! serves.
//!
//! - `QUARRY_EDGE__REDACTION_MODE` is `enforce` (the default), `shadow` or
//!   `off`, case-insensitive. Unset or blank is `enforce`.
//! - `shadow` counts what would be masked and returns the input unchanged. `off`
//!   masks nothing. Both are for development and staging, where `shadow` is how a
//!   rule change is tuned against real pages before `enforce`.
//! - Production accepts only `enforce`: `shadow` and `off` refuse to boot, so no
//!   configuration path turns masking off in production. Rolling back is
//!   redeploying the previous image.
//! - An unparsable value refuses to boot in every environment.
//!
//! WS12.S3 adds the receipt text policy, read by the same boot call:
//!
//! - `QUARRY_EDGE__RECEIPT_TEXT_MODE` is `sensitive` (the default) or `all`,
//!   case-insensitive. Unset or blank is `sensitive`.
//! - `sensitive` keeps text typed into an ordinary control (a search query) in a
//!   stored agent receipt, so the audit trail and a compiled procedure stay
//!   useful; text typed into a password, one-time-code, card, phone or labelled
//!   secret control is always masked. `all` masks every typed text, selected value
//!   and dialog prompt, and makes a compiled procedure unable to replay without its
//!   input. Both are accepted in production. A person's step under
//!   `QUARRY_EDGE__HUMAN_RECEIPT_MODE=metadata` is stricter than either and wins.
//! - An unparsable value refuses to boot in every environment. The policy is
//!   static: it cannot change while the process serves.

use std::fmt;

use quarry_core::redaction::action::{self as record, RecordPolicy};
use quarry_core::redaction::{self, Mode, RULE_VERSION};

/// The environment variable that carries the mode.
pub const ENV_VAR: &str = "QUARRY_EDGE__REDACTION_MODE";

/// The environment variable that carries the receipt text policy (WS12.S3).
pub const RECEIPT_TEXT_ENV_VAR: &str = "QUARRY_EDGE__RECEIPT_TEXT_MODE";

/// Why the edge must not start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedactionBootError {
    /// The value is not `enforce`, `shadow` or `off`.
    Unparsable,
    /// Production accepts only `enforce`.
    NotPermittedInProduction(Mode),
    /// A different mode was already pinned in this process.
    AlreadyPinned(Mode),
    /// `QUARRY_EDGE__RECEIPT_TEXT_MODE` is not `sensitive` or `all`.
    ReceiptTextUnparsable,
    /// A different receipt text policy was already pinned in this process.
    ReceiptTextAlreadyPinned(RecordPolicy),
}

impl fmt::Display for RedactionBootError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unparsable => write!(f, "{ENV_VAR} must be enforce, shadow or off"),
            Self::NotPermittedInProduction(mode) => write!(
                f,
                "{ENV_VAR}={} is refused in production: only enforce is accepted",
                mode_label(*mode)
            ),
            Self::AlreadyPinned(mode) => write!(
                f,
                "the redaction mode was already pinned to {} before boot set it",
                mode_label(*mode)
            ),
            Self::ReceiptTextUnparsable => {
                write!(f, "{RECEIPT_TEXT_ENV_VAR} must be sensitive or all")
            }
            Self::ReceiptTextAlreadyPinned(policy) => write!(
                f,
                "the receipt text policy was already pinned to {} before boot set it",
                policy.as_str()
            ),
        }
    }
}

impl std::error::Error for RedactionBootError {}

/// The mode as a static label (for logs and errors).
#[must_use]
pub const fn mode_label(mode: Mode) -> &'static str {
    match mode {
        Mode::Enforce => "enforce",
        Mode::Shadow => "shadow",
        Mode::Off => "off",
    }
}

/// Decide the mode from the raw value and the environment.
///
/// # Errors
/// [`RedactionBootError::Unparsable`] for an unknown value;
/// [`RedactionBootError::NotPermittedInProduction`] for `shadow` or `off` in
/// production.
pub fn decide(raw: Option<&str>, production: bool) -> Result<Mode, RedactionBootError> {
    let mode = match raw.map(str::trim).filter(|value| !value.is_empty()) {
        None => Mode::Enforce,
        Some(value) => Mode::parse(value).ok_or(RedactionBootError::Unparsable)?,
    };
    if production && !mode.permitted_in_production() {
        return Err(RedactionBootError::NotPermittedInProduction(mode));
    }
    Ok(mode)
}

/// Decide the mode and pin it for the process. Logs the rule version and the
/// mode once.
///
/// # Errors
/// What [`decide`] refuses, or [`RedactionBootError::AlreadyPinned`] when the
/// process already runs under a different mode.
pub fn init_with(raw: Option<&str>, production: bool) -> Result<Mode, RedactionBootError> {
    let mode = decide(raw, production)?;
    match redaction::init_mode(mode) {
        Ok(()) => {}
        Err(pinned) if pinned == mode => {}
        Err(pinned) => return Err(RedactionBootError::AlreadyPinned(pinned)),
    }
    if mode == Mode::Enforce {
        tracing::info!(
            rule_version = RULE_VERSION,
            mode = mode_label(mode),
            "secret redaction active"
        );
    } else {
        tracing::warn!(
            rule_version = RULE_VERSION,
            mode = mode_label(mode),
            "secret redaction is not enforcing: development or staging only"
        );
    }
    Ok(mode)
}

/// [`init_with`] on `QUARRY_EDGE__REDACTION_MODE`, then the receipt text policy
/// on `QUARRY_EDGE__RECEIPT_TEXT_MODE`.
///
/// # Errors
/// See [`init_with`] and [`init_record_policy_from_env`].
pub fn init_from_env(production: bool) -> Result<Mode, RedactionBootError> {
    let mode = init_with(std::env::var(ENV_VAR).ok().as_deref(), production)?;
    init_record_policy_from_env()?;
    Ok(mode)
}

/// Decide the receipt text policy from the raw value. Unset or blank is
/// `sensitive`.
///
/// # Errors
/// [`RedactionBootError::ReceiptTextUnparsable`] for an unknown value.
pub fn decide_record_policy(raw: Option<&str>) -> Result<RecordPolicy, RedactionBootError> {
    match raw.map(str::trim).filter(|value| !value.is_empty()) {
        None => Ok(RecordPolicy::SensitiveOnly),
        Some(value) => {
            RecordPolicy::parse(value).ok_or(RedactionBootError::ReceiptTextUnparsable)
        }
    }
}

/// Decide the receipt text policy and pin it for the process. Logs it once.
///
/// # Errors
/// What [`decide_record_policy`] refuses, or
/// [`RedactionBootError::ReceiptTextAlreadyPinned`] when the process already runs
/// under a different policy.
pub fn init_record_policy(raw: Option<&str>) -> Result<RecordPolicy, RedactionBootError> {
    let policy = decide_record_policy(raw)?;
    match record::init_record_policy(policy) {
        Ok(()) => {}
        Err(pinned) if pinned == policy => {}
        Err(pinned) => return Err(RedactionBootError::ReceiptTextAlreadyPinned(pinned)),
    }
    tracing::info!(receipt_text = policy.as_str(), "receipt text policy set");
    Ok(policy)
}

/// [`init_record_policy`] on `QUARRY_EDGE__RECEIPT_TEXT_MODE`.
///
/// # Errors
/// See [`init_record_policy`].
pub fn init_record_policy_from_env() -> Result<RecordPolicy, RedactionBootError> {
    init_record_policy(std::env::var(RECEIPT_TEXT_ENV_VAR).ok().as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unset_and_blank_enforce() {
        assert_eq!(decide(None, false), Ok(Mode::Enforce));
        assert_eq!(decide(Some("  "), false), Ok(Mode::Enforce));
        assert_eq!(decide(None, true), Ok(Mode::Enforce));
    }

    #[test]
    fn the_three_modes_parse_case_insensitively() {
        assert_eq!(decide(Some("ENFORCE"), false), Ok(Mode::Enforce));
        assert_eq!(decide(Some(" Shadow "), false), Ok(Mode::Shadow));
        assert_eq!(decide(Some("off"), false), Ok(Mode::Off));
    }

    #[test]
    fn production_accepts_only_enforce() {
        assert_eq!(decide(Some("enforce"), true), Ok(Mode::Enforce));
        assert_eq!(
            decide(Some("shadow"), true),
            Err(RedactionBootError::NotPermittedInProduction(Mode::Shadow))
        );
        assert_eq!(
            decide(Some("off"), true),
            Err(RedactionBootError::NotPermittedInProduction(Mode::Off))
        );
    }

    #[test]
    fn an_unknown_value_refuses_everywhere() {
        assert_eq!(
            decide(Some("log"), false),
            Err(RedactionBootError::Unparsable)
        );
        assert_eq!(decide(Some("0"), true), Err(RedactionBootError::Unparsable));
    }

    #[test]
    fn the_receipt_text_policy_defaults_to_sensitive_and_parses_both_words() {
        assert_eq!(decide_record_policy(None), Ok(RecordPolicy::SensitiveOnly));
        assert_eq!(
            decide_record_policy(Some("  ")),
            Ok(RecordPolicy::SensitiveOnly)
        );
        assert_eq!(
            decide_record_policy(Some("Sensitive")),
            Ok(RecordPolicy::SensitiveOnly)
        );
        assert_eq!(decide_record_policy(Some(" ALL ")), Ok(RecordPolicy::AllTyped));
    }

    #[test]
    fn an_unknown_receipt_text_policy_refuses_and_never_echoes_the_value() {
        assert_eq!(
            decide_record_policy(Some("metadata")),
            Err(RedactionBootError::ReceiptTextUnparsable)
        );
        let text = RedactionBootError::ReceiptTextUnparsable.to_string();
        assert!(text.contains(RECEIPT_TEXT_ENV_VAR));
        assert!(!text.contains("metadata"));
    }

    #[test]
    fn the_refusals_name_the_variable_and_never_echo_the_value() {
        let text = RedactionBootError::Unparsable.to_string();
        assert!(text.contains(ENV_VAR));
        let text = RedactionBootError::NotPermittedInProduction(Mode::Off).to_string();
        assert!(text.contains("production"));
    }
}
