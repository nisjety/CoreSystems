//! Envelope encryption for profile snapshots (WS17.E1).
//!
//! One data-encryption key (DEK) per profile, 256-bit AES-GCM (`ring`), one
//! 96-bit random nonce per seal. The DEK is wrapped by a key provider
//! (`profile_keys.rs`) and stored in the envelope header; the envelope binds
//! the ciphertext to its tenant, profile and snapshot id through the AAD. The
//! AAD is rebuilt on open, never stored, so a row copied to another org or
//! profile does not decrypt.
//!
//! Plaintext is gzip-compressed above 4 KiB. Plaintext above 16 MiB is refused
//! before sealing, and decompression is capped at the same size.
//!
//! Nothing here logs. Errors carry a class, never payload bytes.

use std::io::{Read, Write};

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM, NONCE_LEN};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};

/// Envelope format version written by this build.
pub const ENC_V: u16 = 1;

/// Algorithm name carried in the envelope header (informational; the AAD and
/// the cipher are fixed by `ENC_V`).
pub const ALG: &str = "A256GCM";

/// Plaintext larger than this is refused before sealing and before opening.
pub const PLAINTEXT_CAP_BYTES: usize = 16 * 1024 * 1024;

/// Plaintext above this size is gzip-compressed before sealing.
pub const GZIP_THRESHOLD_BYTES: usize = 4096;

/// Domain prefix of the envelope AAD. The version tag inside it is the
/// `ENC_V` constant, so a future format cannot be opened as v1.
const AAD_DOMAIN: &[u8] = b"quarry-profile\0v1\0";

/// Error classes for the envelope layer. Never carries payload or key bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CryptoError {
    /// Plaintext over `PLAINTEXT_CAP_BYTES`, or a decompression bomb.
    TooLarge,
    /// The envelope header does not parse or has the wrong shape.
    Malformed,
    /// Unknown `enc_v`, `alg` or compression tag.
    Unsupported,
    /// The AEAD tag did not verify (tampering, wrong key, wrong org or profile).
    Auth,
    /// The RNG or the compressor failed.
    Codec,
}

impl CryptoError {
    /// Short class for logs.
    pub fn class(self) -> &'static str {
        match self {
            Self::TooLarge => "too_large",
            Self::Malformed => "malformed",
            Self::Unsupported => "unsupported",
            Self::Auth => "auth",
            Self::Codec => "codec",
        }
    }
}

/// The stored envelope (the JSON bytes of this struct are the `envelope`
/// column). Field names are part of the on-disk format; do not rename.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvelopeV1 {
    pub enc_v: u16,
    pub alg: String,
    /// Key that wrapped `wrapped_dek`, for example `env:v1`. Outside the AAD so
    /// a KEK rotation can rewrap the header without re-sealing the payload.
    pub key_id: String,
    /// Base64 of the wrapped DEK (see `profile_keys.rs`).
    pub wrapped_dek: String,
    /// `snap_<ulid>`, one per seal. Part of the AAD.
    pub snapshot_id: String,
    /// `gzip` or `none`.
    pub z: String,
    /// Base64 of the 12-byte nonce.
    pub nonce: String,
    /// Base64 of the ciphertext with the 16-byte tag appended.
    pub ct: String,
}

impl EnvelopeV1 {
    /// The raw wrapped DEK bytes.
    pub fn wrapped_dek_bytes(&self) -> Result<Vec<u8>, CryptoError> {
        B64.decode(self.wrapped_dek.as_bytes())
            .map_err(|_| CryptoError::Malformed)
    }
}

/// Append `bytes` to `out` with a 4-byte big-endian length prefix. Length
/// prefixes make the AAD unambiguous: org `a|b` with profile `c` and org `a`
/// with profile `b|c` produce different bytes.
pub(crate) fn push_len_prefixed(out: &mut Vec<u8>, bytes: &[u8]) {
    let len = u32::try_from(bytes.len()).unwrap_or(u32::MAX);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(bytes);
}

/// AAD of a sealed snapshot:
/// `"quarry-profile\0v1\0" || len32(org) || org || len32(profile) || profile ||
/// len32(snapshot_id) || snapshot_id || enc_v (2 bytes, big-endian)`.
pub fn build_aad(org: &str, profile: &str, snapshot_id: &str, enc_v: u16) -> Vec<u8> {
    let mut out = Vec::with_capacity(
        AAD_DOMAIN.len() + org.len() + profile.len() + snapshot_id.len() + 16,
    );
    out.extend_from_slice(AAD_DOMAIN);
    push_len_prefixed(&mut out, org.as_bytes());
    push_len_prefixed(&mut out, profile.as_bytes());
    push_len_prefixed(&mut out, snapshot_id.as_bytes());
    out.extend_from_slice(&enc_v.to_be_bytes());
    out
}

fn gzip(data: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let mut enc = GzEncoder::new(Vec::new(), Compression::default());
    enc.write_all(data).map_err(|_| CryptoError::Codec)?;
    enc.finish().map_err(|_| CryptoError::Codec)
}

fn gunzip_capped(data: &[u8], cap: usize) -> Result<Vec<u8>, CryptoError> {
    let mut out = Vec::new();
    GzDecoder::new(data)
        .take(cap as u64 + 1)
        .read_to_end(&mut out)
        .map_err(|_| CryptoError::Malformed)?;
    if out.len() > cap {
        return Err(CryptoError::TooLarge);
    }
    Ok(out)
}

fn random_nonce() -> Result<[u8; NONCE_LEN], CryptoError> {
    let mut nonce = [0u8; NONCE_LEN];
    SystemRandom::new()
        .fill(&mut nonce)
        .map_err(|_| CryptoError::Codec)?;
    Ok(nonce)
}

fn random_snapshot_id() -> String {
    format!("snap_{}", ulid::Ulid::new())
}

/// Seal `plaintext` under `dek` for one (org, profile). Generates a fresh
/// nonce and snapshot id.
pub fn seal(
    dek: &[u8; 32],
    org: &str,
    profile: &str,
    plaintext: &[u8],
    key_id: &str,
    wrapped_dek: &[u8],
) -> Result<EnvelopeV1, CryptoError> {
    let nonce = random_nonce()?;
    seal_with(
        dek,
        org,
        profile,
        plaintext,
        key_id,
        wrapped_dek,
        nonce,
        random_snapshot_id(),
    )
}

/// Seal with a caller-chosen nonce and snapshot id. Used by `seal` and by the
/// known-answer test. Never call it with a reused (dek, nonce) pair.
#[allow(clippy::too_many_arguments)]
pub(crate) fn seal_with(
    dek: &[u8; 32],
    org: &str,
    profile: &str,
    plaintext: &[u8],
    key_id: &str,
    wrapped_dek: &[u8],
    nonce: [u8; NONCE_LEN],
    snapshot_id: String,
) -> Result<EnvelopeV1, CryptoError> {
    if plaintext.len() > PLAINTEXT_CAP_BYTES {
        return Err(CryptoError::TooLarge);
    }
    let (body, z) = if plaintext.len() > GZIP_THRESHOLD_BYTES {
        (gzip(plaintext)?, "gzip")
    } else {
        (plaintext.to_vec(), "none")
    };
    let unbound = UnboundKey::new(&AES_256_GCM, dek).map_err(|_| CryptoError::Codec)?;
    let key = LessSafeKey::new(unbound);
    let aad = build_aad(org, profile, &snapshot_id, ENC_V);
    let mut in_out = body;
    key.seal_in_place_append_tag(
        Nonce::assume_unique_for_key(nonce),
        Aad::from(aad),
        &mut in_out,
    )
    .map_err(|_| CryptoError::Codec)?;
    Ok(EnvelopeV1 {
        enc_v: ENC_V,
        alg: ALG.to_string(),
        key_id: key_id.to_string(),
        wrapped_dek: B64.encode(wrapped_dek),
        snapshot_id,
        z: z.to_string(),
        nonce: B64.encode(nonce),
        ct: B64.encode(&in_out),
    })
}

/// Open an envelope under `dek` for (org, profile). Returns the plaintext JSON
/// bytes. Any header or tag mismatch is an error; nothing is partially returned.
pub fn open(
    dek: &[u8; 32],
    org: &str,
    profile: &str,
    env: &EnvelopeV1,
) -> Result<Vec<u8>, CryptoError> {
    if env.enc_v != ENC_V || env.alg != ALG {
        return Err(CryptoError::Unsupported);
    }
    let nonce_vec = B64
        .decode(env.nonce.as_bytes())
        .map_err(|_| CryptoError::Malformed)?;
    let nonce: [u8; NONCE_LEN] = nonce_vec
        .as_slice()
        .try_into()
        .map_err(|_| CryptoError::Malformed)?;
    let mut buf = B64
        .decode(env.ct.as_bytes())
        .map_err(|_| CryptoError::Malformed)?;
    if buf.len() < AES_256_GCM.tag_len() {
        return Err(CryptoError::Malformed);
    }
    let unbound = UnboundKey::new(&AES_256_GCM, dek).map_err(|_| CryptoError::Codec)?;
    let key = LessSafeKey::new(unbound);
    let aad = build_aad(org, profile, &env.snapshot_id, env.enc_v);
    let plain_len = key
        .open_in_place(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(aad),
            &mut buf,
        )
        .map_err(|_| CryptoError::Auth)?
        .len();
    buf.truncate(plain_len);
    match env.z.as_str() {
        "none" => {
            if buf.len() > PLAINTEXT_CAP_BYTES {
                return Err(CryptoError::TooLarge);
            }
            Ok(buf)
        }
        "gzip" => gunzip_capped(&buf, PLAINTEXT_CAP_BYTES),
        _ => Err(CryptoError::Unsupported),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    const DEK: [u8; 32] = [7u8; 32];
    const WRAPPED: &[u8] = b"wrapped-dek-test-bytes";

    fn seal_ok(org: &str, profile: &str, plain: &[u8]) -> EnvelopeV1 {
        seal(&DEK, org, profile, plain, "env:v1", WRAPPED).expect("seal")
    }

    #[test]
    fn seal_open_roundtrip() {
        let plain = br#"{"cookies":[{"name":"quarry_e2e","value":"CANARY"}]}"#;
        let env = seal_ok("org_a", "prof_1", plain);
        assert_eq!(env.enc_v, ENC_V);
        assert_eq!(env.z, "none");
        assert_eq!(open(&DEK, "org_a", "prof_1", &env).expect("open"), plain);
        // The ciphertext must not contain the canary in clear.
        let ct = B64.decode(env.ct.as_bytes()).unwrap();
        assert!(!ct.windows(6).any(|w| w == b"CANARY"));
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let mut env = seal_ok("org_a", "prof_1", b"{\"a\":1}");
        let mut ct = B64.decode(env.ct.as_bytes()).unwrap();
        ct[0] ^= 0x01;
        env.ct = B64.encode(&ct);
        assert_eq!(open(&DEK, "org_a", "prof_1", &env), Err(CryptoError::Auth));
    }

    #[test]
    fn aad_binds_org_profile_and_snapshot() {
        let env = seal_ok("org_a", "prof_1", b"{\"a\":1}");
        assert_eq!(open(&DEK, "org_b", "prof_1", &env), Err(CryptoError::Auth));
        assert_eq!(open(&DEK, "org_a", "prof_2", &env), Err(CryptoError::Auth));
        let mut moved = env.clone();
        moved.snapshot_id = "snap_other".to_string();
        assert_eq!(open(&DEK, "org_a", "prof_1", &moved), Err(CryptoError::Auth));
        let wrong_key = [9u8; 32];
        assert_eq!(open(&wrong_key, "org_a", "prof_1", &env), Err(CryptoError::Auth));
    }

    #[test]
    fn aad_is_length_prefixed_not_separator_ambiguous() {
        // org "a|b" + profile "c" must not collide with org "a" + profile "b|c".
        let x = build_aad("a|b", "c", "snap_1", ENC_V);
        let y = build_aad("a", "b|c", "snap_1", ENC_V);
        assert_ne!(x, y);
    }

    #[test]
    fn nonces_are_unique_across_10000_seals() {
        let mut seen = HashSet::new();
        for _ in 0..10_000 {
            let env = seal_ok("org_a", "prof_1", b"{}");
            assert!(seen.insert(env.nonce), "nonce repeated");
        }
    }

    #[test]
    fn empty_and_max_size_snapshots() {
        let empty = seal_ok("org_a", "prof_1", b"");
        assert_eq!(open(&DEK, "org_a", "prof_1", &empty).unwrap(), b"");
        let max = vec![b'x'; PLAINTEXT_CAP_BYTES];
        let env = seal_ok("org_a", "prof_1", &max);
        assert_eq!(env.z, "gzip");
        assert_eq!(open(&DEK, "org_a", "prof_1", &env).unwrap(), max);
        let over = vec![b'x'; PLAINTEXT_CAP_BYTES + 1];
        assert_eq!(
            seal(&DEK, "org_a", "prof_1", &over, "env:v1", WRAPPED),
            Err(CryptoError::TooLarge)
        );
    }

    #[test]
    fn gzip_threshold() {
        let small = vec![b'a'; GZIP_THRESHOLD_BYTES];
        assert_eq!(seal_ok("o", "p", &small).z, "none");
        let big = vec![b'a'; GZIP_THRESHOLD_BYTES + 1];
        assert_eq!(seal_ok("o", "p", &big).z, "gzip");
    }

    #[test]
    fn known_answer_vector() {
        // Fixed key, nonce and snapshot id: sealing is deterministic, and the
        // result opens back. The pinned ciphertext value is owed (see the
        // W2-U36 depth notes); this test pins the shape and the round trip.
        let nonce = [1u8; NONCE_LEN];
        let a = seal_with(
            &DEK,
            "org_a",
            "prof_1",
            b"{\"k\":1}",
            "env:v1",
            WRAPPED,
            nonce,
            "snap_fixed".to_string(),
        )
        .unwrap();
        let b = seal_with(
            &DEK,
            "org_a",
            "prof_1",
            b"{\"k\":1}",
            "env:v1",
            WRAPPED,
            nonce,
            "snap_fixed".to_string(),
        )
        .unwrap();
        assert_eq!(a, b);
        assert_eq!(open(&DEK, "org_a", "prof_1", &a).unwrap(), b"{\"k\":1}");
    }

    #[test]
    fn unknown_version_and_compression_are_refused() {
        let mut env = seal_ok("o", "p", b"{}");
        env.enc_v = 2;
        assert_eq!(open(&DEK, "o", "p", &env), Err(CryptoError::Unsupported));
        let mut env = seal_ok("o", "p", b"{}");
        env.z = "zstd".to_string();
        assert_eq!(open(&DEK, "o", "p", &env), Err(CryptoError::Unsupported));
    }
}
