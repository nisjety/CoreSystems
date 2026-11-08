//! Block escalation policy (WS3.I, ADR-5).
//!
//! One question lives here: may a fetch change the identity it presents (the
//! egress proxy, the TLS fingerprint, the browser tier) because of what the
//! origin answered? Under `honest` the answer is no. A status, a block-class
//! error or a caller hint never changes identity; only a failure of the
//! transport itself (the connection did not carry the request) rotates a proxy.
//! Quarry then refuses with an existing error code and the caller decides.
//!
//! This module classifies and refuses. It does not spoof anything: no user
//! agent, TLS or header change is added anywhere, in either mode.
//!
//! `QUARRY_BLOCK_ESCALATION=legacy|honest` selects the mode once per process
//! (a change is a restart, which is also the rollback). `legacy` records the
//! behaviour of releases before WS3.I: a block status rotates the proxy, the
//! next driver in the chain is tried and a prior-block hint raises the driver
//! tier. Components take the mode as an explicit field, with [`current`] only as
//! the default, so tests never touch the process environment.

use std::sync::OnceLock;

use quarry_core::error::ErrorCode;

/// The environment variable that selects the mode.
pub const ESCALATION_ENV: &str = "QUARRY_BLOCK_ESCALATION";

/// The mode a process runs in when [`ESCALATION_ENV`] is unset (or blank).
/// The mechanism shipped with `Legacy` as the default (61906c93); this is the
/// flip to `Honest`, made on ADR-5's approved default in its own commit and
/// gated by `build_default_matches_the_documented_default`. The plan asks for a
/// before and after measurement of the pages a TLS or browser tier used to
/// rescue; none was possible on this branch, so the flip rests on the tests, not
/// on a measurement. `QUARRY_BLOCK_ESCALATION=legacy` is the rollback.
pub const BUILD_DEFAULT: EscalationMode = EscalationMode::Honest;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EscalationMode {
    /// Pre-WS3.I behaviour: a block status may rotate the proxy and the driver.
    Legacy,
    /// A status never changes identity; only a transport failure rotates.
    Honest,
}

impl Default for EscalationMode {
    fn default() -> Self {
        BUILD_DEFAULT
    }
}

impl EscalationMode {
    /// The pure half of [`current`](Self::current).
    ///
    /// Unset or blank is the build default. `legacy` and `honest` (any case,
    /// surrounding blanks ignored) are themselves. Any other value is `Honest`:
    /// an operator who set the variable meant a change, and `Honest` can only
    /// refuse more, never widen identity changes, so a typo fails closed.
    pub fn from_value(value: Option<&str>) -> Self {
        let Some(raw) = value else {
            return BUILD_DEFAULT;
        };
        match raw.trim().to_ascii_lowercase().as_str() {
            "" => BUILD_DEFAULT,
            "legacy" => Self::Legacy,
            _ => Self::Honest,
        }
    }

    /// The process-wide mode, read from the environment once. The first read
    /// logs the mode name (operator configuration, not caller data).
    pub fn current() -> Self {
        static MODE: OnceLock<EscalationMode> = OnceLock::new();
        *MODE.get_or_init(|| {
            let mode = Self::from_value(std::env::var(ESCALATION_ENV).ok().as_deref());
            tracing::info!(mode = mode.as_str(), "quarry.block_escalation.mode");
            mode
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Legacy => "legacy",
            Self::Honest => "honest",
        }
    }

    pub fn is_honest(self) -> bool {
        self == Self::Honest
    }
}

/// What prompted a request to change identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EscalationTrigger {
    /// The origin answered with a block-class HTTP status (0 when only the
    /// error class survived, see [`trigger_for_error`]).
    BlockStatus(u16),
    /// A strong challenge was classified on the page (`challenge.rs`).
    StrongChallenge,
    /// The caller passed prior-block hints (`DriverSignals::prior_block_signals`).
    CallerSignals(u8),
    /// The connection did not carry the request: connect, timeout, reset.
    TransportError,
}

/// May `trigger` change the identity presented to the origin under `mode`?
pub fn escalation_allowed(mode: EscalationMode, trigger: EscalationTrigger) -> bool {
    match mode {
        EscalationMode::Legacy => true,
        EscalationMode::Honest => matches!(trigger, EscalationTrigger::TransportError),
    }
}

/// The trigger a driver error stands for. `UpstreamBlocked` and `RateLimited`
/// are what a block status becomes once it is an error; every other code
/// (timeout, driver failure) is a transport-class failure.
///
/// A connect failure is also reported as `UpstreamBlocked` by the static driver
/// (its code mapping predates this module and is public API), so under `honest`
/// a connect failure does not fall through to the next driver either. That is
/// the fail-closed side of an ambiguity the error code cannot resolve.
pub fn trigger_for_error(code: ErrorCode) -> EscalationTrigger {
    match code {
        ErrorCode::UpstreamBlocked | ErrorCode::RateLimited => EscalationTrigger::BlockStatus(0),
        _ => EscalationTrigger::TransportError,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_TRIGGERS: [EscalationTrigger; 4] = [
        EscalationTrigger::BlockStatus(403),
        EscalationTrigger::StrongChallenge,
        EscalationTrigger::CallerSignals(2),
        EscalationTrigger::TransportError,
    ];

    #[test]
    fn matrix_legacy_allows_every_trigger() {
        for trigger in ALL_TRIGGERS {
            assert!(
                escalation_allowed(EscalationMode::Legacy, trigger),
                "legacy must keep allowing {trigger:?}"
            );
        }
    }

    #[test]
    fn matrix_honest_allows_only_transport_error() {
        for trigger in ALL_TRIGGERS {
            let allowed = escalation_allowed(EscalationMode::Honest, trigger);
            assert_eq!(
                allowed,
                trigger == EscalationTrigger::TransportError,
                "honest and {trigger:?}"
            );
        }
        for status in [401u16, 403, 407, 429, 503] {
            assert!(!escalation_allowed(
                EscalationMode::Honest,
                EscalationTrigger::BlockStatus(status)
            ));
        }
    }

    #[test]
    fn unset_is_build_default_and_unknown_is_honest() {
        assert_eq!(EscalationMode::from_value(None), BUILD_DEFAULT);
        assert_eq!(EscalationMode::from_value(Some("")), BUILD_DEFAULT);
        assert_eq!(EscalationMode::from_value(Some("   ")), BUILD_DEFAULT);
        assert_eq!(
            EscalationMode::from_value(Some("legacy")),
            EscalationMode::Legacy
        );
        assert_eq!(
            EscalationMode::from_value(Some(" LEGACY ")),
            EscalationMode::Legacy
        );
        assert_eq!(
            EscalationMode::from_value(Some("honest")),
            EscalationMode::Honest
        );
        assert_eq!(
            EscalationMode::from_value(Some("Honest")),
            EscalationMode::Honest
        );
        // A typo fails closed: it can only refuse more.
        for typo in ["hnoest", "off", "false", "stealth", "0"] {
            assert_eq!(
                EscalationMode::from_value(Some(typo)),
                EscalationMode::Honest,
                "{typo:?}"
            );
        }
    }

    #[test]
    fn block_class_errors_map_to_block_status() {
        for code in [ErrorCode::UpstreamBlocked, ErrorCode::RateLimited] {
            assert!(matches!(
                trigger_for_error(code),
                EscalationTrigger::BlockStatus(_)
            ));
        }
        for code in [
            ErrorCode::Timeout,
            ErrorCode::DriverFailed,
            ErrorCode::Internal,
        ] {
            assert_eq!(trigger_for_error(code), EscalationTrigger::TransportError);
        }
    }

    #[test]
    fn default_mode_is_the_build_default() {
        assert_eq!(EscalationMode::default(), BUILD_DEFAULT);
    }

    /// The gate for the default: changing `BUILD_DEFAULT` means changing this
    /// line and the "Block escalation" section of docs/RUNBOOKS.md in the same
    /// commit, on purpose. It checks the constant and, through
    /// [`resolved_in_child`], the mode a production constructor really gets
    /// from [`EscalationMode::current`] with the variable unset.
    #[test]
    fn build_default_matches_the_documented_default() {
        assert_eq!(BUILD_DEFAULT, EscalationMode::Honest);
        assert_eq!(resolved_in_child(None), "honest");
    }

    /// The documented rollback: `QUARRY_BLOCK_ESCALATION=legacy` selects the
    /// pre-WS3.I behaviour through the same `current()` every constructor reads.
    #[test]
    fn the_env_var_reaches_current_and_legacy_is_the_rollback() {
        assert_eq!(resolved_in_child(Some("legacy")), "legacy");
        assert_eq!(resolved_in_child(Some(" LEGACY ")), "legacy");
        assert_eq!(resolved_in_child(Some("honest")), "honest");
        assert_eq!(resolved_in_child(Some("")), "honest");
        // A typo fails closed.
        assert_eq!(resolved_in_child(Some("legcay")), "honest");
    }

    const CHILD_MARKER: &str = "QUARRY_ESCALATION_TEST_CHILD";

    /// Not a test of its own: it does nothing unless the parent re-runs this
    /// executable with [`CHILD_MARKER`] set, and then prints what the
    /// process-wide `current()` resolved to. `current()` caches in a
    /// `OnceLock` and reads the real environment, so each environment needs its
    /// own process.
    #[test]
    fn child_prints_the_resolved_mode() {
        if std::env::var_os(CHILD_MARKER).is_none() {
            return;
        }
        println!("RESOLVED_MODE={}", EscalationMode::current().as_str());
    }

    /// Runs this test executable again with `ESCALATION_ENV` set to `value`
    /// (or removed) and returns the mode name `current()` resolved to there.
    fn resolved_in_child(value: Option<&str>) -> String {
        let exe = std::env::current_exe().expect("test executable path");
        let mut command = std::process::Command::new(exe);
        command
            .args([
                "--exact",
                "escalation::tests::child_prints_the_resolved_mode",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD_MARKER, "1")
            .env_remove(ESCALATION_ENV);
        if let Some(value) = value {
            command.env(ESCALATION_ENV, value);
        }
        let output = command.output().expect("run the child test process");
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        assert!(
            output.status.success(),
            "child failed: {stdout}{}",
            String::from_utf8_lossy(&output.stderr)
        );
        // libtest prints "test <name> ... " on the same line before the child's
        // own output, so look for the marker anywhere in the line.
        const MARKER: &str = "RESOLVED_MODE=";
        stdout
            .lines()
            .find_map(|line| line.find(MARKER).map(|at| &line[at + MARKER.len()..]))
            .map(|mode| mode.trim().to_owned())
            .unwrap_or_else(|| {
                panic!("child printed no mode, so the filter matched no test: {stdout}")
            })
    }
}
