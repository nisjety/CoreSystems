//! Readiness state for a service's GDPR erasure consumer.
//!
//! Lives in the shared crate because the bug it prevents was plane-wide, not
//! service-local. Every Rust service on `verevon.gdpr.erasure.requested`
//! supervises its consumer with a retry-forever loop that only logs; when the
//! shared broker rejected their credentials, all of them kept reporting
//! `healthy` while org erasure silently stopped being applied. Their Go
//! siblings on the same subject treat the identical failure as fatal, so the
//! plane had two opposite answers to one question.
//!
//! This module supplies the third answer: stay alive, but stop claiming to be
//! ready once the outage is no longer plausibly transient. Crashing (the Go
//! behaviour) trades a compliance outage for a serving outage; staying silent
//! (the old Rust behaviour) hides it entirely.
//!
//! # Why a process-level static
//!
//! There is exactly one erasure consumer per process. Threading a handle from
//! the supervisor task into every HTTP handler would mean touching each
//! service's `AppState` and all of its test fixtures, which is a lot of
//! plumbing for one bit and a timestamp. The statics are private; the only way
//! in is [`mark_enabled`] / [`mark_connected`], and the only way out is
//! [`readiness`].
//!
//! # Usage
//!
//! ```ignore
//! // in the supervisor, before the retry loop:
//! erasure_health::mark_enabled();
//! loop {
//!     match run_once(..).await {
//!         Ok(()) => {}
//!         Err(e) => { erasure_health::mark_connected(false); /* log */ }
//!     }
//! }
//! // and immediately after a successful bind:
//! erasure_health::mark_connected(true);
//!
//! // in /readyz:
//! let erasure = erasure_health::readiness();
//! let ok = pg_ok && erasure.is_ready();
//! ```

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How long the consumer may stay disconnected before readiness starts
/// failing. Long enough to absorb a broker restart or a rolling redeploy,
/// short enough that a credential mismatch or a wrong network posture cannot
/// sit unnoticed for a working day.
pub const READINESS_GRACE: Duration = Duration::from_secs(15 * 60);

static ENABLED: AtomicBool = AtomicBool::new(false);
static CONNECTED: AtomicBool = AtomicBool::new(false);
/// Unix seconds of the last successful bind. 0 = never connected.
static LAST_OK_EPOCH: AtomicU64 = AtomicU64::new(0);

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Record that the consumer is configured and will be supervised.
///
/// Called before the first connection attempt, not after a successful one, so
/// [`readiness`] can tell "switched off" apart from "configured but broken" —
/// the distinction that decides whether an unbound consumer is a deployment
/// choice or an incident.
pub fn mark_enabled() {
    ENABLED.store(true, Ordering::Relaxed);
}

/// Record a successful bind (`true`) or a lost/failed connection (`false`).
pub fn mark_connected(connected: bool) {
    CONNECTED.store(connected, Ordering::Relaxed);
    if connected {
        LAST_OK_EPOCH.store(now_epoch(), Ordering::Relaxed);
    }
}

/// Erasure-consumer state, as reported by `/readyz`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErasureReadiness {
    /// No GDPR broker configured — deliberately off, not a fault.
    Disabled,
    /// Bound to the shared broker and consuming.
    Connected,
    /// Disconnected, but inside [`READINESS_GRACE`]. Visible, still ready.
    Reconnecting { seconds_down: u64 },
    /// Disconnected past the grace window, or never connected at all.
    Stalled { seconds_down: u64 },
}

impl ErasureReadiness {
    /// Whether this state should keep the instance in the ready set.
    #[must_use]
    pub const fn is_ready(self) -> bool {
        !matches!(self, Self::Stalled { .. })
    }

    /// Stable string for the `/readyz` body.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Connected => "connected",
            Self::Reconnecting { .. } => "reconnecting",
            Self::Stalled { .. } => "stalled",
        }
    }
}

/// Current readiness of this process's erasure consumer.
#[must_use]
pub fn readiness() -> ErasureReadiness {
    if !ENABLED.load(Ordering::Relaxed) {
        return ErasureReadiness::Disabled;
    }
    if CONNECTED.load(Ordering::Relaxed) {
        return ErasureReadiness::Connected;
    }
    let last_ok = LAST_OK_EPOCH.load(Ordering::Relaxed);
    // Never connected is NOT given the grace window. A consumer that has never
    // bound is a misconfiguration, not a blip — and it is precisely the shape
    // of the credential mismatch this module exists to surface, where waiting
    // 15 minutes to admit the problem would just re-hide it.
    if last_ok == 0 {
        return ErasureReadiness::Stalled { seconds_down: 0 };
    }
    let seconds_down = now_epoch().saturating_sub(last_ok);
    if seconds_down >= READINESS_GRACE.as_secs() {
        ErasureReadiness::Stalled { seconds_down }
    } else {
        ErasureReadiness::Reconnecting { seconds_down }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// The statics are process-global, so the tests that mutate them must not
    /// interleave. Cargo runs tests in one process on multiple threads.
    static GUARD: Mutex<()> = Mutex::new(());

    fn reset(enabled: bool, connected: bool, last_ok: u64) {
        ENABLED.store(enabled, Ordering::Relaxed);
        CONNECTED.store(connected, Ordering::Relaxed);
        LAST_OK_EPOCH.store(last_ok, Ordering::Relaxed);
    }

    #[test]
    fn never_connected_is_stalled_not_reconnecting() {
        let _g = GUARD.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        reset(true, false, 0);
        let state = readiness();
        assert_eq!(state, ErasureReadiness::Stalled { seconds_down: 0 });
        assert!(
            !state.is_ready(),
            "a consumer that never bound must not read as ready — this is the \
             credential-mismatch case that stayed invisible"
        );
    }

    #[test]
    fn a_brief_reconnect_stays_ready() {
        let _g = GUARD.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        reset(true, false, now_epoch().saturating_sub(60));
        let state = readiness();
        assert!(matches!(state, ErasureReadiness::Reconnecting { .. }));
        assert!(state.is_ready(), "a broker blip must not evict the instance");
    }

    #[test]
    fn a_sustained_outage_fails_readiness() {
        let _g = GUARD.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        reset(
            true,
            false,
            now_epoch().saturating_sub(READINESS_GRACE.as_secs() + 1),
        );
        let state = readiness();
        assert!(matches!(state, ErasureReadiness::Stalled { .. }));
        assert!(!state.is_ready());
    }

    #[test]
    fn connected_is_ready() {
        let _g = GUARD.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        reset(true, true, now_epoch());
        assert_eq!(readiness(), ErasureReadiness::Connected);
        assert!(readiness().is_ready());
    }

    /// An unconfigured consumer is a deployment choice; a text-only or
    /// standalone deployment must not be held out of the ready set for it.
    #[test]
    fn disabled_is_ready() {
        let _g = GUARD.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        reset(false, false, 0);
        assert_eq!(readiness(), ErasureReadiness::Disabled);
        assert!(readiness().is_ready());
    }

    #[test]
    fn mark_connected_records_a_bind_timestamp() {
        let _g = GUARD.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        reset(true, false, 0);
        mark_connected(true);
        assert!(LAST_OK_EPOCH.load(Ordering::Relaxed) > 0);
        // Losing the connection must keep the timestamp, or the downtime clock
        // resets on every retry and `Stalled` is never reached.
        mark_connected(false);
        assert!(LAST_OK_EPOCH.load(Ordering::Relaxed) > 0);
        assert!(matches!(readiness(), ErasureReadiness::Reconnecting { .. }));
    }
}
