//! Idempotency key derivation using blake3.
//!
//! Strategy: `blake3(producer | event_type | resource_ref | idempotency_key)`
//! This produces a stable 32-byte hash used for deduplication in NATS
//! consumers and the events table unique constraint.

/// Derive a 64-character hex idempotency hash from envelope fields.
#[must_use]
pub fn derive_idempotency_hash(
    producer: &str,
    event_type: &str,
    resource_ref: &str,
    idempotency_key: &str,
) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(producer.as_bytes());
    hasher.update(b"|");
    hasher.update(event_type.as_bytes());
    hasher.update(b"|");
    hasher.update(resource_ref.as_bytes());
    hasher.update(b"|");
    hasher.update(idempotency_key.as_bytes());
    hasher.finalize().to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_hash() {
        let h1 =
            derive_idempotency_hash("model-gateway", "INGRESS_ACCEPTED", "thread/abc", "req-1");
        let h2 =
            derive_idempotency_hash("model-gateway", "INGRESS_ACCEPTED", "thread/abc", "req-1");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn different_inputs_different_hash() {
        let h1 =
            derive_idempotency_hash("model-gateway", "INGRESS_ACCEPTED", "thread/abc", "req-1");
        let h2 =
            derive_idempotency_hash("model-gateway", "INGRESS_REJECTED", "thread/abc", "req-1");
        assert_ne!(h1, h2);
    }

    /// Golden vector — MUST match Go `envelope.DeriveIdempotencyHash` for cross-language parity.
    /// Input: "model-gateway|INGRESS_ACCEPTED|thread/abc|req-1"
    #[test]
    fn golden_idempotency_hash() {
        let h = derive_idempotency_hash("model-gateway", "INGRESS_ACCEPTED", "thread/abc", "req-1");
        assert_eq!(
            h,
            "fbc1d94e94d756ede12c527b3b59e2204f58a623e6bd5a3d679eb03d93f22637"
        );
    }

    /// Golden vector for orchestration tuple — MUST match Go
    /// `orchestration.idemPrefix("plan.transitioned", "thread/abc", "req-1")`
    /// which delegates to `envelope.DeriveIdempotencyHash("orchestrator-core", ...)`.
    /// Input: "orchestrator-core|plan.transitioned|thread/abc|req-1"
    #[test]
    fn golden_orchestration_digest() {
        let h = derive_idempotency_hash(
            "orchestrator-core",
            "plan.transitioned",
            "thread/abc",
            "req-1",
        );
        assert_eq!(
            h,
            "6170f7b4e75b6eb6f29b1eae866049136c042d4ec5570f5cd8644bbd60c36b38"
        );
    }
}
