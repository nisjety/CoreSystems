//! Cryptographic primitives for the RustDesk-compatible wire protocol's
//! handshake and session encryption. No I/O, no protocol-message knowledge —
//! callers (`va-protocol`) own message shapes and sequencing.
//!
//! Mirrors, function-for-function, the primitives `@verevon/remote-core`
//! (the browser controller side) already uses via `libsodium-wrappers`:
//! Ed25519 identity signing (attached signatures), X25519 sealed-box key
//! exchange, and XSalsa20-Poly1305 secretbox with the wire protocol's custom
//! counter-based nonce scheme (independent per-direction counters over one
//! shared key — a known, deliberate wire-compatibility quirk, not a bug).

use dryoc::classic::crypto_box as box_api;
use dryoc::classic::crypto_secretbox as secretbox_api;
use dryoc::classic::crypto_sign as sign_api;
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("signature verification failed")]
    InvalidSignature,
    #[error("signed message too short to contain a signature")]
    SignedMessageTooShort,
    #[error("sealed-box open failed (wrong key or corrupted ciphertext)")]
    UnsealFailed,
    #[error("secretbox open failed (wrong key, wrong nonce, or corrupted ciphertext)")]
    OpenFailed,
}

/// A device's persistent Ed25519 identity keypair. Generated once per
/// install and registered with hbbs (`RegisterPk`); every connection's
/// `SignedId` is signed with this key, never regenerated per-session.
pub struct IdentityKeyPair {
    public: sign_api::PublicKey,
    secret: sign_api::SecretKey,
}

impl IdentityKeyPair {
    #[must_use]
    pub fn generate() -> Self {
        let (public, secret) = sign_api::crypto_sign_keypair();
        Self { public, secret }
    }

    #[must_use]
    pub fn public_key(&self) -> [u8; 32] {
        self.public
    }

    /// Produces an ATTACHED Ed25519 signature over `message`: the wire
    /// format `RustDesk`'s `SignedId.id` uses is `signature(64) || message`,
    /// not a detached signature — a detached verification would fail on
    /// otherwise-valid data.
    ///
    /// # Panics
    ///
    /// Never, in practice: the output buffer is always sized to exactly
    /// `message.len() + 64`, which is the one length `dryoc`'s
    /// `crypto_sign` accepts.
    #[must_use]
    pub fn sign(&self, message: &[u8]) -> Vec<u8> {
        let mut signed = vec![0u8; message.len() + 64];
        sign_api::crypto_sign(&mut signed, message, &self.secret)
            .expect("crypto_sign: buffer length is exactly message.len() + CRYPTO_SIGN_BYTES");
        signed
    }
}

/// Verifies an attached-signature payload (as produced by
/// [`IdentityKeyPair::sign`]) against `public_key`, returning the original
/// message on success.
///
/// # Errors
///
/// Returns [`CryptoError::SignedMessageTooShort`] if `signed_message` is
/// shorter than the 64-byte signature, or [`CryptoError::InvalidSignature`]
/// if the signature doesn't verify against `public_key`.
pub fn verify_signed(public_key: &[u8; 32], signed_message: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if signed_message.len() < 64 {
        return Err(CryptoError::SignedMessageTooShort);
    }
    let mut message = vec![0u8; signed_message.len() - 64];
    sign_api::crypto_sign_open(&mut message, signed_message, public_key)
        .map_err(|_| CryptoError::InvalidSignature)?;
    Ok(message)
}

/// A fresh X25519 keypair generated for exactly one connection attempt.
/// Never reused across sessions.
pub struct EphemeralKeyPair {
    public: box_api::PublicKey,
    secret: box_api::SecretKey,
}

impl EphemeralKeyPair {
    #[must_use]
    pub fn generate() -> Self {
        let (public, secret) = box_api::crypto_box_keypair();
        Self { public, secret }
    }

    #[must_use]
    pub fn public_key(&self) -> [u8; 32] {
        self.public
    }
}

/// Anonymous-sender sealed-box encryption (`crypto_box_seal`): used by
/// whichever side is SENDING the session key (the controller, in the real
/// protocol) to seal it to the other side's ephemeral public key. Exposed
/// here so `va-testkit`'s fake controller can play that role too.
///
/// # Panics
///
/// Never, in practice: the output buffer is always sized to exactly
/// `message.len() + CRYPTO_BOX_SEALBYTES`, the one length `dryoc` accepts.
#[must_use]
pub fn seal(recipient_public: &[u8; 32], message: &[u8]) -> Vec<u8> {
    let mut ciphertext = vec![0u8; message.len() + dryoc::constants::CRYPTO_BOX_SEALBYTES];
    box_api::crypto_box_seal(&mut ciphertext, message, recipient_public)
        .expect("crypto_box_seal: buffer length is exactly message.len() + CRYPTO_BOX_SEALBYTES");
    ciphertext
}

/// Opens a sealed box addressed to `recipient_public`/`recipient_secret`
/// (the host's own ephemeral keypair). This is what the host does with
/// `PublicKey.symmetric_value` to recover the shared session key.
///
/// # Errors
///
/// Returns [`CryptoError::UnsealFailed`] if `ciphertext` is too short, or if
/// opening fails (wrong recipient keypair or corrupted ciphertext).
pub fn unseal(
    recipient: &EphemeralKeyPair,
    ciphertext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    if ciphertext.len() < dryoc::constants::CRYPTO_BOX_SEALBYTES {
        return Err(CryptoError::UnsealFailed);
    }
    let mut message = vec![0u8; ciphertext.len() - dryoc::constants::CRYPTO_BOX_SEALBYTES];
    box_api::crypto_box_seal_open(&mut message, ciphertext, &recipient.public, &recipient.secret)
        .map_err(|_| CryptoError::UnsealFailed)?;
    Ok(message)
}

/// One direction of the session's secretbox cipher. The wire protocol keeps
/// two of these over the SAME key `K` — one for what this side sends, one
/// for what it receives — each with its own independently-incrementing
/// counter and no direction/role bit anywhere on the wire. This replicates
/// a real (and known) weakness in `RustDesk`'s own scheme; it is required
/// for wire compatibility, not something to "fix" unilaterally.
pub struct DirectionalCipher {
    key: secretbox_api::Key,
    counter: u64,
}

impl DirectionalCipher {
    #[must_use]
    pub fn new(key: [u8; 32]) -> Self {
        Self { key, counter: 0 }
    }

    /// Nonce = 24 bytes; low 8 = little-endian counter (incremented BEFORE
    /// use, so the first message's counter value is 1, not 0); high 16
    /// bytes always zero.
    fn next_nonce(&mut self) -> secretbox_api::Nonce {
        self.counter += 1;
        let mut nonce = [0u8; 24];
        nonce[..8].copy_from_slice(&self.counter.to_le_bytes());
        nonce
    }

    /// # Panics
    ///
    /// Never, in practice: the output buffer is always sized to exactly
    /// `message.len() + CRYPTO_SECRETBOX_MACBYTES`, the one length `dryoc`
    /// accepts.
    #[must_use]
    pub fn seal(&mut self, message: &[u8]) -> Vec<u8> {
        let nonce = self.next_nonce();
        let mut ciphertext = vec![0u8; message.len() + dryoc::constants::CRYPTO_SECRETBOX_MACBYTES];
        secretbox_api::crypto_secretbox_easy(&mut ciphertext, message, &nonce, &self.key)
            .expect("crypto_secretbox_easy: buffer length is exactly message.len() + MACBYTES");
        ciphertext
    }

    /// # Errors
    ///
    /// Returns [`CryptoError::OpenFailed`] if `ciphertext` is too short, or
    /// if authentication fails (wrong key or corrupted ciphertext). Note
    /// this always advances this cipher's counter, even on failure — the
    /// caller owns retry/reconnect policy, not this type.
    pub fn open(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let nonce = self.next_nonce();
        if ciphertext.len() < dryoc::constants::CRYPTO_SECRETBOX_MACBYTES {
            return Err(CryptoError::OpenFailed);
        }
        let mut message = vec![0u8; ciphertext.len() - dryoc::constants::CRYPTO_SECRETBOX_MACBYTES];
        secretbox_api::crypto_secretbox_open_easy(&mut message, ciphertext, &nonce, &self.key)
            .map_err(|_| CryptoError::OpenFailed)?;
        Ok(message)
    }
}

/// The full per-session cipher: independent send/receive
/// [`DirectionalCipher`]s over one shared key, exactly mirroring
/// `remote-core`'s `SessionChannel`.
pub struct SessionCipher {
    pub send: DirectionalCipher,
    pub recv: DirectionalCipher,
}

impl SessionCipher {
    #[must_use]
    pub fn new(session_key: [u8; 32]) -> Self {
        Self {
            send: DirectionalCipher::new(session_key),
            recv: DirectionalCipher::new(session_key),
        }
    }
}

/// The wire's plain-double-SHA-256 password scheme: `h1 = SHA256(secret ‖
/// salt)`, `wire = SHA256(h1 ‖ challenge)`. Both concatenations operate on
/// UTF-8-encoded strings (the wire's `Hash.salt`/`Hash.challenge` fields are
/// proto3 `string`, not arbitrary bytes) — no scrypt/Argon2/PBKDF2 anywhere
/// in this path. `salt` is a stable, per-device value generated once and
/// reused across sessions; `challenge` must be freshly random per
/// connection, which is what makes `wire` replay-resistant.
#[must_use]
pub fn hash_password(secret: &str, salt: &str, challenge: &str) -> [u8; 32] {
    let mut h1 = Sha256::new();
    h1.update(secret.as_bytes());
    h1.update(salt.as_bytes());
    let h1 = h1.finalize();

    let mut wire = Sha256::new();
    wire.update(h1);
    wire.update(challenge.as_bytes());
    wire.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_and_verify_round_trips() {
        let identity = IdentityKeyPair::generate();
        let message = b"IdPk-encoded-bytes";
        let signed = identity.sign(message);
        assert_eq!(signed.len(), message.len() + 64);
        let recovered = verify_signed(&identity.public_key(), &signed).unwrap();
        assert_eq!(recovered, message);
    }

    #[test]
    fn verify_rejects_tampered_signature() {
        let identity = IdentityKeyPair::generate();
        let mut signed = identity.sign(b"hello");
        signed[0] ^= 0xFF;
        assert!(verify_signed(&identity.public_key(), &signed).is_err());
    }

    #[test]
    fn verify_rejects_wrong_key() {
        let identity = IdentityKeyPair::generate();
        let other = IdentityKeyPair::generate();
        let signed = identity.sign(b"hello");
        assert!(verify_signed(&other.public_key(), &signed).is_err());
    }

    #[test]
    fn seal_and_unseal_round_trips() {
        let host_ephemeral = EphemeralKeyPair::generate();
        let session_key = b"0123456789abcdef0123456789abcdef".to_vec();
        let sealed = seal(&host_ephemeral.public_key(), &session_key[..32]);
        let opened = unseal(&host_ephemeral, &sealed).unwrap();
        assert_eq!(opened, session_key[..32]);
    }

    #[test]
    fn unseal_fails_with_wrong_recipient() {
        let host_ephemeral = EphemeralKeyPair::generate();
        let other_ephemeral = EphemeralKeyPair::generate();
        let sealed = seal(&host_ephemeral.public_key(), b"secret-session-key-32-bytes-long");
        assert!(unseal(&other_ephemeral, &sealed).is_err());
    }

    #[test]
    fn directional_ciphers_round_trip_independently() {
        let key = [7u8; 32];
        let mut a_send = DirectionalCipher::new(key);
        let mut b_recv = DirectionalCipher::new(key);

        let ct1 = a_send.seal(b"first message");
        assert_eq!(b_recv.open(&ct1).unwrap(), b"first message");

        let ct2 = a_send.seal(b"second message");
        assert_eq!(b_recv.open(&ct2).unwrap(), b"second message");
    }

    #[test]
    fn first_nonce_counter_is_one_not_zero() {
        // The nonce's low 8 bytes are the counter; encrypting the same
        // plaintext under a fresh cipher must not reuse nonce=0.
        let key = [3u8; 32];
        let mut cipher = DirectionalCipher::new(key);
        let nonce = cipher.next_nonce();
        assert_eq!(&nonce[..8], &1u64.to_le_bytes());
        assert_eq!(&nonce[8..], &[0u8; 16]);
    }

    #[test]
    fn open_fails_on_tampered_ciphertext() {
        let key = [1u8; 32];
        let mut sender = DirectionalCipher::new(key);
        let mut receiver = DirectionalCipher::new(key);
        let mut ct = sender.seal(b"tamper me");
        let last = ct.len() - 1;
        ct[last] ^= 0xFF;
        assert!(receiver.open(&ct).is_err());
    }

    #[test]
    fn password_hash_matches_known_vector() {
        // Cross-checked byte-for-byte against an independent Node
        // implementation of remote-core's own scheme (double SHA-256 over
        // UTF-8 secret/salt/challenge) for inputs
        // ("hunter2", "device-salt", "session-challenge") — not just
        // internal self-consistency.
        let hash = hash_password("hunter2", "device-salt", "session-challenge");
        assert_eq!(
            hex::encode(hash),
            "c8012a72ce16cf43a6e804f8a657cd5d7ab11041f8dad2862d49d67f500f3ddc"
        );
        // Different challenge must change the output (replay resistance).
        assert_ne!(hash, hash_password("hunter2", "device-salt", "other-challenge"));
    }
}
