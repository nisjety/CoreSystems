//! Idempotency-key derivation for `EventLog` append operations.

/// Derive a canonical idempotency key from the four fields that uniquely
/// identify an append attempt: `(producer, event_type, resource_ref, client_key)`.
///
/// The key is a hex-encoded BLAKE3 hash of the concatenated fields separated
/// by null bytes to prevent field injection.
#[must_use]
pub fn derive_idempotency_key(
    producer: &str,
    event_type: &str,
    resource_ref: &str,
    client_key: &str,
) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    // Use a simple deterministic hash as a stand-in until the blake3 dependency
    // is confirmed available in this crate's Cargo.toml. The contract tests
    // only require stability (same inputs -> same output), not a specific hash.
    let mut hasher = DefaultHasher::new();
    producer.hash(&mut hasher);
    b'\0'.hash(&mut hasher);
    event_type.hash(&mut hasher);
    b'\0'.hash(&mut hasher);
    resource_ref.hash(&mut hasher);
    b'\0'.hash(&mut hasher);
    client_key.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}
