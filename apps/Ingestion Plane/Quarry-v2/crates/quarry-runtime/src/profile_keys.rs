//! Key provider for profile envelopes (WS17.E1).
//!
//! The trait wraps and unwraps the per-profile DEK. The interim provider,
//! `EnvKeyProvider`, holds master keys from the edge environment
//! (`QUARRY_PROFILE_MASTER_KEYS`, a JSON object of `{"<id>": "<base64 32 bytes>"}`,
//! with `QUARRY_PROFILE_MASTER_KEY_ACTIVE` naming the master used for new wraps).
//! The Control Plane key provider (`org_core`, WS17.E2) is not built here.
//!
//! Wrapped DEK layout: `nonce(12) || AES-256-GCM(master, dek) || tag(16)`, with
//! AAD `"quarry.profile.wrap\0v1\0" || len32(org) || org || len32(profile) ||
//! profile`. The `key_id` (`env:<master_id>`) lives in the envelope header, not
//! in the wrap. Unwrapped DEKs are cached for 300 s (at most 4096 entries,
//! evictable by org) and are never logged.

use std::collections::HashMap;
use std::time::Duration;

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM, NONCE_LEN};
use ring::rand::{SecureRandom, SystemRandom};
use zeroize::Zeroizing;

use crate::profile_crypto::push_len_prefixed;

/// Prefix of `key_id` values minted by the env provider.
pub const ENV_KEY_ID_PREFIX: &str = "env:";

/// TTL of an unwrapped DEK in the cache.
pub const KEY_CACHE_TTL_S: u64 = 300;

/// Maximum number of cached unwrapped DEKs.
pub const KEY_CACHE_MAX_ENTRIES: u64 = 4096;

/// Why a key operation failed. Carries a class only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyError {
    /// The provider cannot answer right now (for example the Control Plane is
    /// down). The caller must not treat the profile as absent.
    Unavailable,
    /// The key that wrapped the DEK is no longer configured.
    Destroyed,
    /// The wrap does not verify for this org or profile.
    Denied,
    /// Malformed wrap or key material.
    Invalid,
}

impl KeyError {
    pub fn class(self) -> &'static str {
        match self {
            Self::Unavailable => "unavailable",
            Self::Destroyed => "destroyed",
            Self::Denied => "denied",
            Self::Invalid => "invalid",
        }
    }
}

/// A new DEK with its wrap and the key that produced the wrap.
pub struct GeneratedDek {
    pub dek: Zeroizing<[u8; 32]>,
    pub wrapped: Vec<u8>,
    pub key_id: String,
}

/// Wraps and unwraps per-profile DEKs.
pub trait ProfileKeyProvider: Send + Sync {
    /// A fresh random DEK, wrapped under the current key.
    fn generate(&self, org: &str, profile: &str) -> Result<GeneratedDek, KeyError> {
        let mut dek = Zeroizing::new([0u8; 32]);
        SystemRandom::new()
            .fill(&mut dek[..])
            .map_err(|_| KeyError::Unavailable)?;
        let (wrapped, key_id) = self.wrap(org, profile, &dek)?;
        Ok(GeneratedDek {
            dek,
            wrapped,
            key_id,
        })
    }

    /// Wrap `dek` under the current key. Returns the wrap and its `key_id`.
    fn wrap(&self, org: &str, profile: &str, dek: &[u8; 32]) -> Result<(Vec<u8>, String), KeyError>;

    /// Unwrap a DEK that was wrapped under `key_id`.
    fn unwrap(
        &self,
        org: &str,
        profile: &str,
        key_id: &str,
        wrapped: &[u8],
    ) -> Result<Zeroizing<[u8; 32]>, KeyError>;

    /// The `key_id` that new wraps use.
    fn current_key_id(&self) -> String;

    /// Unwrap, then wrap under the current key (used after a KEK rotation).
    fn rewrap(
        &self,
        org: &str,
        profile: &str,
        key_id: &str,
        wrapped: &[u8],
    ) -> Result<(Vec<u8>, String), KeyError> {
        let dek = self.unwrap(org, profile, key_id, wrapped)?;
        self.wrap(org, profile, &dek)
    }
}

/// Why the env master-key configuration was refused at boot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyConfigError {
    /// The JSON object is missing, empty, or does not parse.
    Malformed,
    /// A master key is not 32 bytes of base64.
    BadKeyLength,
    /// `QUARRY_PROFILE_MASTER_KEY_ACTIVE` names no configured key.
    ActiveNotConfigured,
}

impl KeyConfigError {
    pub fn class(self) -> &'static str {
        match self {
            Self::Malformed => "malformed",
            Self::BadKeyLength => "bad_key_length",
            Self::ActiveNotConfigured => "active_not_configured",
        }
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct UnwrapCacheKey {
    org: String,
    profile: String,
    key_id: String,
    wrapped_digest: [u8; 32],
}

/// Interim `env` provider: master keys from the edge environment.
pub struct EnvKeyProvider {
    masters: HashMap<String, Zeroizing<[u8; 32]>>,
    active: String,
    cache: moka::sync::Cache<UnwrapCacheKey, Zeroizing<[u8; 32]>>,
}

impl EnvKeyProvider {
    /// Build from the two environment values. Refuses anything weak, so the
    /// boot check can name the problem instead of falling back silently.
    pub fn from_config(master_keys_json: &str, active: &str) -> Result<Self, KeyConfigError> {
        let raw: HashMap<String, String> =
            serde_json::from_str(master_keys_json).map_err(|_| KeyConfigError::Malformed)?;
        if raw.is_empty() {
            return Err(KeyConfigError::Malformed);
        }
        let mut masters = HashMap::with_capacity(raw.len());
        for (id, b64) in raw {
            let bytes = B64
                .decode(b64.as_bytes())
                .map_err(|_| KeyConfigError::BadKeyLength)?;
            let arr: [u8; 32] = bytes
                .as_slice()
                .try_into()
                .map_err(|_| KeyConfigError::BadKeyLength)?;
            masters.insert(id, Zeroizing::new(arr));
        }
        if !masters.contains_key(active) {
            return Err(KeyConfigError::ActiveNotConfigured);
        }
        Ok(Self {
            masters,
            active: active.to_string(),
            cache: moka::sync::Cache::builder()
                .max_capacity(KEY_CACHE_MAX_ENTRIES)
                .time_to_live(Duration::from_secs(KEY_CACHE_TTL_S))
                .support_invalidation_closures()
                .build(),
        })
    }

    /// Drop every cached DEK of one org (called on profile delete and org purge).
    pub fn evict_org(&self, org: &str) {
        let org = org.to_string();
        let _ = self
            .cache
            .invalidate_entries_if(move |k, _| k.org == org);
    }

    /// Entries currently cached (after pending maintenance). Tests and metrics.
    pub fn cached_entries(&self) -> u64 {
        self.cache.run_pending_tasks();
        self.cache.entry_count()
    }
}

fn wrap_aad(org: &str, profile: &str) -> Vec<u8> {
    let mut out = b"quarry.profile.wrap\0v1\0".to_vec();
    push_len_prefixed(&mut out, org.as_bytes());
    push_len_prefixed(&mut out, profile.as_bytes());
    out
}

fn wrap_with(
    master: &[u8; 32],
    org: &str,
    profile: &str,
    dek: &[u8; 32],
) -> Result<Vec<u8>, KeyError> {
    let mut nonce = [0u8; NONCE_LEN];
    SystemRandom::new()
        .fill(&mut nonce)
        .map_err(|_| KeyError::Unavailable)?;
    let key = LessSafeKey::new(
        UnboundKey::new(&AES_256_GCM, master).map_err(|_| KeyError::Invalid)?,
    );
    let mut in_out = dek.to_vec();
    key.seal_in_place_append_tag(
        Nonce::assume_unique_for_key(nonce),
        Aad::from(wrap_aad(org, profile)),
        &mut in_out,
    )
    .map_err(|_| KeyError::Invalid)?;
    let mut out = Vec::with_capacity(NONCE_LEN + in_out.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&in_out);
    Ok(out)
}

fn unwrap_with(
    master: &[u8; 32],
    org: &str,
    profile: &str,
    wrapped: &[u8],
) -> Result<Zeroizing<[u8; 32]>, KeyError> {
    // nonce + 32-byte DEK + 16-byte tag.
    if wrapped.len() != NONCE_LEN + 32 + AES_256_GCM.tag_len() {
        return Err(KeyError::Invalid);
    }
    let nonce: [u8; NONCE_LEN] = wrapped[..NONCE_LEN]
        .try_into()
        .map_err(|_| KeyError::Invalid)?;
    let key = LessSafeKey::new(
        UnboundKey::new(&AES_256_GCM, master).map_err(|_| KeyError::Invalid)?,
    );
    let mut buf = wrapped[NONCE_LEN..].to_vec();
    let plain = key
        .open_in_place(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(wrap_aad(org, profile)),
            &mut buf,
        )
        .map_err(|_| KeyError::Denied)?;
    let arr: [u8; 32] = plain.try_into().map_err(|_| KeyError::Invalid)?;
    Ok(Zeroizing::new(arr))
}

impl ProfileKeyProvider for EnvKeyProvider {
    fn wrap(&self, org: &str, profile: &str, dek: &[u8; 32]) -> Result<(Vec<u8>, String), KeyError> {
        let master = self.masters.get(&self.active).ok_or(KeyError::Destroyed)?;
        let wrapped = wrap_with(master, org, profile, dek)?;
        Ok((wrapped, self.current_key_id()))
    }

    fn unwrap(
        &self,
        org: &str,
        profile: &str,
        key_id: &str,
        wrapped: &[u8],
    ) -> Result<Zeroizing<[u8; 32]>, KeyError> {
        let cache_key = UnwrapCacheKey {
            org: org.to_string(),
            profile: profile.to_string(),
            key_id: key_id.to_string(),
            wrapped_digest: *blake3::hash(wrapped).as_bytes(),
        };
        if let Some(hit) = self.cache.get(&cache_key) {
            return Ok(hit);
        }
        let master_id = key_id
            .strip_prefix(ENV_KEY_ID_PREFIX)
            .ok_or(KeyError::Invalid)?;
        let master = self.masters.get(master_id).ok_or(KeyError::Destroyed)?;
        let dek = unwrap_with(master, org, profile, wrapped)?;
        self.cache.insert(cache_key, dek.clone());
        Ok(dek)
    }

    fn current_key_id(&self) -> String {
        format!("{ENV_KEY_ID_PREFIX}{}", self.active)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b64_key(byte: u8) -> String {
        B64.encode([byte; 32])
    }

    fn provider() -> EnvKeyProvider {
        let json = format!(r#"{{"v1":"{}","v2":"{}"}}"#, b64_key(1), b64_key(2));
        EnvKeyProvider::from_config(&json, "v1").expect("config")
    }

    #[test]
    fn env_provider_wrap_unwrap() {
        let p = provider();
        let g = p.generate("org_a", "prof_1").expect("generate");
        assert_eq!(g.key_id, "env:v1");
        let back = p
            .unwrap("org_a", "prof_1", &g.key_id, &g.wrapped)
            .expect("unwrap");
        assert_eq!(*back, *g.dek);
    }

    #[test]
    fn unwrap_with_wrong_org_fails() {
        let p = provider();
        let g = p.generate("org_a", "prof_1").expect("generate");
        assert_eq!(
            p.unwrap("org_b", "prof_1", &g.key_id, &g.wrapped).err(),
            Some(KeyError::Denied)
        );
        assert_eq!(
            p.unwrap("org_a", "prof_2", &g.key_id, &g.wrapped).err(),
            Some(KeyError::Denied)
        );
    }

    #[test]
    fn unknown_master_is_destroyed_and_rewrap_moves_to_current() {
        let p = provider();
        let other = EnvKeyProvider::from_config(
            &format!(r#"{{"v2":"{}"}}"#, b64_key(2)),
            "v2",
        )
        .unwrap();
        let g = p.generate("org_a", "prof_1").unwrap();
        assert_eq!(
            other.unwrap("org_a", "prof_1", "env:v1", &g.wrapped).err(),
            Some(KeyError::Destroyed)
        );
        let (rewrapped, key_id) = p.rewrap("org_a", "prof_1", "env:v1", &g.wrapped).unwrap();
        assert_eq!(key_id, "env:v1");
        assert!(p.unwrap("org_a", "prof_1", &key_id, &rewrapped).is_ok());
    }

    #[test]
    fn key_cache_ttl_and_org_eviction() {
        let p = provider();
        let g = p.generate("org_a", "prof_1").unwrap();
        let h = p.generate("org_b", "prof_1").unwrap();
        p.unwrap("org_a", "prof_1", &g.key_id, &g.wrapped).unwrap();
        p.unwrap("org_b", "prof_1", &h.key_id, &h.wrapped).unwrap();
        assert_eq!(p.cached_entries(), 2);
        p.evict_org("org_a");
        assert_eq!(p.cached_entries(), 1);
    }

    #[test]
    fn config_refuses_weak_or_missing_material() {
        assert_eq!(
            EnvKeyProvider::from_config("{}", "v1").err(),
            Some(KeyConfigError::Malformed)
        );
        assert_eq!(
            EnvKeyProvider::from_config("not json", "v1").err(),
            Some(KeyConfigError::Malformed)
        );
        let short = format!(r#"{{"v1":"{}"}}"#, B64.encode([1u8; 16]));
        assert_eq!(
            EnvKeyProvider::from_config(&short, "v1").err(),
            Some(KeyConfigError::BadKeyLength)
        );
        let ok = format!(r#"{{"v1":"{}"}}"#, b64_key(1));
        assert_eq!(
            EnvKeyProvider::from_config(&ok, "v9").err(),
            Some(KeyConfigError::ActiveNotConfigured)
        );
    }
}
