//! Application-level encryption for retained approval continuation descriptors.
//!
//! The descriptor is action input, not authority. It still must not be stored
//! as plaintext JSONB because database snapshots and read-only approval views
//! are outside the service's transport boundary. The key is supplied by the
//! deployment secret manager through `SESSION_CORE_CONTINUATION_DESCRIPTOR_KEY`
//! as base64-encoded 32-byte AES-256-GCM material. Missing or malformed key
//! material fails closed whenever a descriptor is written or read.
//!
//! The same cipher seals the durable loop's fold (`loop_checkpoint.rs`). Each
//! caller binds its ciphertext to its own AAD scope, so a ciphertext cannot be
//! moved to another row, org, user or segment and still open.

use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use ring::{aead, rand};

const KEY_ENV: &str = "SESSION_CORE_CONTINUATION_DESCRIPTOR_KEY";
const PREFIX: &str = "v1:";
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;

/// Encrypt/decrypt one descriptor with an AAD scope bound to its owning row.
pub(crate) struct DescriptorCipher {
    key: aead::LessSafeKey,
    random: rand::SystemRandom,
}

impl DescriptorCipher {
    /// Load the deployment key. Callers must not fall back to a process-local
    /// default: a missing key means the descriptor boundary is unavailable.
    pub(crate) fn from_env() -> Result<Self> {
        let encoded = std::env::var(KEY_ENV)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .with_context(|| format!("{KEY_ENV} is required for continuation descriptors"))?;
        let decoded = BASE64
            .decode(encoded.trim())
            .context("continuation descriptor key is not valid base64")?;
        Self::from_key_bytes(&decoded)
    }

    #[cfg(test)]
    pub(crate) fn from_key_bytes_for_test(key: &[u8]) -> Result<Self> {
        Self::from_key_bytes(key)
    }

    fn from_key_bytes(key: &[u8]) -> Result<Self> {
        if key.len() != KEY_LEN {
            bail!("continuation descriptor key must be exactly {KEY_LEN} bytes");
        }
        let unbound = aead::UnboundKey::new(&aead::AES_256_GCM, key)
            .map_err(|_| anyhow::anyhow!("continuation descriptor key is invalid"))?;
        Ok(Self {
            key: aead::LessSafeKey::new(unbound),
            random: rand::SystemRandom::new(),
        })
    }

    /// The AAD scope of an approval descriptor.
    fn approval_aad(approval_id: &str, org_id: &str, user_id: &str) -> String {
        format!("approval:{approval_id}:org:{org_id}:user:{user_id}")
    }

    pub(crate) fn encrypt(
        &self,
        approval_id: &str,
        org_id: &str,
        user_id: &str,
        plaintext: &str,
    ) -> Result<String> {
        self.seal(&Self::approval_aad(approval_id, org_id, user_id), plaintext)
    }

    pub(crate) fn decrypt(
        &self,
        approval_id: &str,
        org_id: &str,
        user_id: &str,
        ciphertext: &str,
    ) -> Result<String> {
        self.open(
            &Self::approval_aad(approval_id, org_id, user_id),
            ciphertext,
        )
    }

    /// Seal `plaintext` under a caller-built AAD scope. The scope must name the
    /// row the ciphertext belongs to, so opening it anywhere else fails.
    pub(crate) fn seal(&self, aad: &str, plaintext: &str) -> Result<String> {
        let mut nonce_bytes = [0_u8; NONCE_LEN];
        rand::SecureRandom::fill(&self.random, &mut nonce_bytes)
            .map_err(|_| anyhow::anyhow!("could not generate continuation nonce"))?;
        let nonce = aead::Nonce::assume_unique_for_key(nonce_bytes);
        let mut payload = plaintext.as_bytes().to_vec();
        self.key
            .seal_in_place_append_tag(
                nonce,
                aead::Aad::from(aad.as_bytes().to_vec()),
                &mut payload,
            )
            .map_err(|_| anyhow::anyhow!("could not encrypt continuation payload"))?;
        let mut encoded = nonce_bytes.to_vec();
        encoded.extend_from_slice(&payload);
        Ok(format!("{PREFIX}{}", BASE64.encode(encoded)))
    }

    /// Open a ciphertext sealed under the same AAD scope.
    pub(crate) fn open(&self, aad: &str, ciphertext: &str) -> Result<String> {
        let encoded = ciphertext
            .strip_prefix(PREFIX)
            .context("unsupported continuation ciphertext version")?;
        let payload = BASE64
            .decode(encoded)
            .context("continuation ciphertext is not valid base64")?;
        if payload.len() <= NONCE_LEN + aead::MAX_TAG_LEN {
            bail!("continuation ciphertext is too short");
        }
        let mut nonce_bytes = [0_u8; NONCE_LEN];
        nonce_bytes.copy_from_slice(&payload[..NONCE_LEN]);
        let nonce = aead::Nonce::assume_unique_for_key(nonce_bytes);
        let mut sealed = payload[NONCE_LEN..].to_vec();
        let plaintext = self
            .key
            .open_in_place(nonce, aead::Aad::from(aad.as_bytes().to_vec()), &mut sealed)
            .map_err(|_| anyhow::anyhow!("continuation authentication failed"))?;
        String::from_utf8(plaintext.to_vec()).context("continuation plaintext is not valid UTF-8")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypts_and_decrypts_with_row_bound_aad() {
        let cipher = DescriptorCipher::from_key_bytes_for_test(&[7_u8; KEY_LEN]).unwrap();
        let encrypted = cipher
            .encrypt("appr-1", "org-1", "user-1", r#"{"tool":"send"}"#)
            .unwrap();
        assert!(encrypted.starts_with(PREFIX));
        assert_ne!(encrypted, r#"{"tool":"send"}"#);
        assert_eq!(
            cipher
                .decrypt("appr-1", "org-1", "user-1", &encrypted)
                .unwrap(),
            r#"{"tool":"send"}"#
        );
        assert!(cipher
            .decrypt("appr-1", "org-2", "user-1", &encrypted)
            .is_err());
    }

    #[test]
    fn a_sealed_scope_opens_only_under_the_same_scope() {
        let cipher = DescriptorCipher::from_key_bytes_for_test(&[7_u8; KEY_LEN]).unwrap();
        let sealed = cipher
            .seal("loopfold:run-1:org:org-1:user:user-1:seq:4", "fold")
            .unwrap();
        assert_eq!(
            cipher
                .open("loopfold:run-1:org:org-1:user:user-1:seq:4", &sealed)
                .unwrap(),
            "fold"
        );
        // Moving the fold to another seq, run or owner must not open it.
        assert!(cipher
            .open("loopfold:run-1:org:org-1:user:user-1:seq:5", &sealed)
            .is_err());
        assert!(cipher
            .open("loopfold:run-2:org:org-1:user:user-1:seq:4", &sealed)
            .is_err());
    }

    #[test]
    fn rejects_missing_or_wrong_sized_keys() {
        assert!(DescriptorCipher::from_key_bytes_for_test(&[]).is_err());
        assert!(DescriptorCipher::from_key_bytes_for_test(&[0_u8; 31]).is_err());
        assert!(DescriptorCipher::from_key_bytes_for_test(&[0_u8; 33]).is_err());
    }
}
