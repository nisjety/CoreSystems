//! Opaque, tenant/user-bound idempotency identities for managed starts.
//!
//! Session Core durably stores `StartManagedRunRequest.start_key`. Public
//! request IDs and idempotency keys must therefore be transformed before they
//! cross that boundary, especially for ZDR callers whose client values can
//! themselves contain sensitive text.

use anyhow::{Context, Result};

const ENV_SECRET: &str = "MODEL_GATEWAY_MANAGED_START_KEY_SECRET";
const DERIVATION_CONTEXT: &str = "velion/model-gateway/managed-start-key/v1";
const DOMAIN: &[u8] = b"managed-start-key\0";

/// Holds only a derived BLAKE3 key, never the configured secret material.
#[derive(Clone)]
pub(crate) struct ManagedStartKeyDeriver {
    key: [u8; 32],
}

impl ManagedStartKeyDeriver {
    /// Read the mandatory production secret. The service must not start with a
    /// predictable durable-idempotency transform.
    pub(crate) fn from_env() -> Result<Self> {
        let material = std::env::var(ENV_SECRET)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .context("MODEL_GATEWAY_MANAGED_START_KEY_SECRET is required")?;
        Ok(Self::from_material(&material))
    }

    /// Derive a fixed-size, opaque identity. Length-prefixing prevents an
    /// ambiguity such as `(org="ab", user="c")` matching `(org="a",
    /// user="bc")`; callers never receive the configured key or raw input.
    #[must_use]
    pub(crate) fn derive(
        &self,
        org_id: &str,
        user_id: &str,
        client_idempotency_key: Option<&str>,
        request_id: &str,
        managed_source: &str,
    ) -> String {
        let retry_identity = client_idempotency_key
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(request_id);
        let mut hasher = blake3::Hasher::new_keyed(&self.key);
        hasher.update(DOMAIN);
        update_segment(&mut hasher, org_id.as_bytes());
        update_segment(&mut hasher, user_id.as_bytes());
        update_segment(&mut hasher, retry_identity.as_bytes());
        // The configured source/route is part of the domain so an agentic
        // ExecutionAgent retry cannot collide with a GatewayDirect retry that
        // happened to reuse the same client idempotency key.
        update_segment(&mut hasher, managed_source.as_bytes());
        format!("msk-v1-{}", hasher.finalize().to_hex())
    }

    fn from_material(material: &str) -> Self {
        Self {
            key: blake3::derive_key(DERIVATION_CONTEXT, material.as_bytes()),
        }
    }

    /// `AppState::new` is an in-memory test/embedding constructor. Production
    /// uses `from_env`, which always replaces this isolated test key before a
    /// listener is started.
    #[must_use]
    pub(crate) fn test_only() -> Self {
        Self::from_material("model-gateway-in-memory-test-key-not-for-production")
    }
}

fn update_segment(hasher: &mut blake3::Hasher, value: &[u8]) {
    let length = u64::try_from(value.len()).expect("segment length fits u64");
    hasher.update(&length.to_be_bytes());
    hasher.update(value);
}

#[cfg(test)]
mod tests {
    use super::ManagedStartKeyDeriver;

    #[test]
    fn opaque_key_is_deterministic_scoped_and_does_not_echo_client_input() {
        let deriver = ManagedStartKeyDeriver::test_only();
        let raw = "private customer invoice 891";
        let first = deriver.derive("org-a", "user-a", Some(raw), "request-a", "gateway-direct");
        assert_eq!(
            first,
            deriver.derive("org-a", "user-a", Some(raw), "request-b", "gateway-direct",),
            "a client retry key must win over a trace/request id"
        );
        assert_ne!(
            first,
            deriver.derive("org-b", "user-a", Some(raw), "request-a", "gateway-direct")
        );
        assert_ne!(
            first,
            deriver.derive("org-a", "user-b", Some(raw), "request-a", "gateway-direct")
        );
        assert_ne!(
            first,
            deriver.derive("org-a", "user-a", Some(raw), "request-a", "execution-agent"),
            "same retry key must not collide across distinct managed producers"
        );
        assert!(first.starts_with("msk-v1-"));
        assert!(!first.contains(raw));
    }
}
