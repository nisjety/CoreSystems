//! Block-status classification.
//!
//! `is_block_status` says whether an HTTP status is an anti-bot block or soft
//! denial (as opposed to a genuine 404 or 400). The fallback driver uses it to
//! fall through to the next driver in the chain. There is no fingerprint or
//! session rotation here: the round-robin `FingerprintRotator` this module used
//! to hold had no caller and was removed (WS3.E).

/// True when an HTTP status signals an anti-bot block / soft denial worth
/// falling through to the next driver for (vs. a genuine 404/400 it won't fix).
pub fn is_block_status(status: u16) -> bool {
    matches!(status, 401 | 403 | 407 | 429 | 503)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_statuses_detected() {
        for s in [401, 403, 407, 429, 503] {
            assert!(is_block_status(s), "{s} should be a block");
        }
        for s in [200, 301, 400, 404, 500] {
            assert!(!is_block_status(s), "{s} should NOT be a block");
        }
    }
}
