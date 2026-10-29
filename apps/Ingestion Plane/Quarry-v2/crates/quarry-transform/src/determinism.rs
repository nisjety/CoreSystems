//! Determinism verification: run a transform twice; assert identical fingerprints.
//!
//! Used in CI/dev modes to catch hidden nondeterminism (HashMap iteration,
//! time-dependent fields, RNG, etc.) before artifacts ship.

use crate::fingerprint::{content_fingerprint, Fingerprint};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeterminismError {
    Mismatch {
        first: Fingerprint,
        second: Fingerprint,
    },
}

impl fmt::Display for DeterminismError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeterminismError::Mismatch { first, second } => write!(
                f,
                "determinism check failed: first={} second={}",
                first.as_str(),
                second.as_str()
            ),
        }
    }
}

impl std::error::Error for DeterminismError {}

/// Run `run` twice, fingerprint both outputs, return the fingerprint if equal.
///
/// Returns `Err(DeterminismError::Mismatch)` if the two runs disagree.
pub fn verify_deterministic<F>(run: F) -> Result<Fingerprint, DeterminismError>
where
    F: Fn() -> Vec<u8>,
{
    let first = content_fingerprint(&run());
    let second = content_fingerprint(&run());
    if first == second {
        Ok(first)
    } else {
        Err(DeterminismError::Mismatch { first, second })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn deterministic_run_returns_fingerprint() {
        let fp = verify_deterministic(|| b"stable bytes".to_vec()).expect("should be stable");
        assert!(fp.as_str().starts_with("blake3:"));
    }

    #[test]
    fn nondeterministic_run_returns_mismatch() {
        let counter = AtomicUsize::new(0);
        let result = verify_deterministic(|| {
            let n = counter.fetch_add(1, Ordering::SeqCst);
            format!("run-{n}").into_bytes()
        });
        match result {
            Err(DeterminismError::Mismatch { first, second }) => assert_ne!(first, second),
            other => panic!("expected mismatch, got {other:?}"),
        }
    }
}
