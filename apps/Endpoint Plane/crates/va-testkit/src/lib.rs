//! `FakeController` — a test double playing the CONTROLLER side of the
//! handshake, the inverse of `@verevon/remote-core`'s own
//! `tests/doubles/FakeRustDeskPeer.ts` (which plays the HOST side for
//! testing the browser client). Bypasses hbbs/hbbr entirely, exactly like
//! that TS double does — this drives `va-protocol`'s `HostSession` directly
//! over in-memory byte buffers, no sockets.
#![allow(clippy::missing_errors_doc)]

use thiserror::Error;
use va_crypto::{seal, DirectionalCipher, EphemeralKeyPair};
use va_protocol::messages::{
    decode_login_response, decode_signed_id, encode_login_request, encode_public_key,
    message_field, read_envelope_field, wrap_message, LoginRequest as WireLoginRequest, PublicKey,
};

#[derive(Debug, Error)]
pub enum FakeControllerError {
    #[error("wire decoding failed: {0}")]
    Wire(#[from] va_wire::WireError),
    #[error("crypto operation failed: {0}")]
    Crypto(#[from] va_crypto::CryptoError),
    #[error("expected {expected}, got field {got}")]
    UnexpectedField { expected: &'static str, got: u32 },
    #[error("empty envelope")]
    EmptyEnvelope,
    #[error("signature verification against the host's known identity key failed")]
    InvalidHostSignature,
}

pub enum LoginOutcome {
    Success { hostname: String, platform: String },
    Failed { reason: String },
}

/// Plays the controller side. Constructed knowing the host's IDENTITY
/// public key in advance (mirroring how a real controller learns it from
/// hbbs's server-signed vouch — this double skips that vouch entirely, the
/// same simplification `FakeRustDeskPeer.ts` makes for the reverse role).
pub struct FakeController {
    host_identity_public_key: [u8; 32],
    my_ephemeral: EphemeralKeyPair,
    host_ephemeral_public_key: Option<[u8; 32]>,
    send_cipher: Option<DirectionalCipher>,
    recv_cipher: Option<DirectionalCipher>,
}

impl FakeController {
    #[must_use]
    pub fn new(host_identity_public_key: [u8; 32]) -> Self {
        Self {
            host_identity_public_key,
            my_ephemeral: EphemeralKeyPair::generate(),
            host_ephemeral_public_key: None,
            send_cipher: None,
            recv_cipher: None,
        }
    }

    /// Feeds the host's plaintext `SignedId` envelope. Verifies the
    /// signature, extracts the host's ephemeral public key, generates a
    /// fresh session key, seals it, and returns the `PublicKey` envelope
    /// (still plaintext) to send back.
    pub fn on_signed_id(&mut self, envelope: &[u8]) -> Result<Vec<u8>, FakeControllerError> {
        let (field_number, payload) =
            read_envelope_field(envelope)?.ok_or(FakeControllerError::EmptyEnvelope)?;
        if field_number != message_field::SIGNED_ID {
            return Err(FakeControllerError::UnexpectedField {
                expected: "SignedId",
                got: field_number,
            });
        }
        let signed = decode_signed_id(payload)?;
        let id_pk_bytes = va_crypto::verify_signed(&self.host_identity_public_key, &signed)
            .map_err(|_| FakeControllerError::InvalidHostSignature)?;
        let id_pk = va_protocol::messages::decode_id_pk(&id_pk_bytes)?;

        let mut host_pk = [0u8; 32];
        host_pk.copy_from_slice(&id_pk.pk[..32.min(id_pk.pk.len())]);
        self.host_ephemeral_public_key = Some(host_pk);

        let session_key: [u8; 32] = rand::random();
        let sealed = seal(&host_pk, &session_key);

        // Mirrors va-crypto's SessionCipher, but with send/recv swapped
        // relative to the host: what the CONTROLLER sends, the host
        // receives, and vice versa — both directions still key off the
        // same shared K, each with its own independently-incrementing
        // counter (see va_crypto::SessionCipher's own doc comment).
        self.send_cipher = Some(DirectionalCipher::new(session_key));
        self.recv_cipher = Some(DirectionalCipher::new(session_key));

        let public_key_envelope = wrap_message(
            message_field::PUBLIC_KEY,
            &encode_public_key(&PublicKey {
                asymmetric_value: self.my_ephemeral.public_key().to_vec(),
                symmetric_value: sealed,
            }),
        );
        Ok(public_key_envelope)
    }

    /// Feeds an encrypted `Message` FROM the host (using our recv cipher,
    /// which mirrors the host's send cipher).
    pub fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, FakeControllerError> {
        let cipher = self
            .recv_cipher
            .as_mut()
            .ok_or(FakeControllerError::EmptyEnvelope)?;
        Ok(cipher.open(ciphertext)?)
    }

    /// Encrypts a `Message` envelope TO the host (using our send cipher,
    /// which mirrors the host's recv cipher).
    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, FakeControllerError> {
        let cipher = self
            .send_cipher
            .as_mut()
            .ok_or(FakeControllerError::EmptyEnvelope)?;
        Ok(cipher.seal(plaintext))
    }

    /// Builds and encrypts a `LoginRequest` in response to the host's
    /// `Hash` challenge.
    pub fn build_login_request(
        &mut self,
        device_id: &str,
        secret: &str,
        salt: &str,
        challenge: &str,
    ) -> Result<Vec<u8>, FakeControllerError> {
        let password = va_crypto::hash_password(secret, salt, challenge).to_vec();
        let envelope = wrap_message(
            message_field::LOGIN_REQUEST,
            &encode_login_request(&WireLoginRequest {
                username: device_id.to_string(),
                password,
                my_id: "fake-controller".to_string(),
                my_name: "Verevon Support (test)".to_string(),
                video_ack_required: false,
                session_id: 1,
                version: "0.1.0-test".to_string(),
                my_platform: "Web".to_string(),
                hwid: Vec::new(),
            }),
        );
        self.encrypt(&envelope)
    }

    /// Decrypts and parses a `LoginResponse` envelope from the host.
    pub fn parse_login_response(&mut self, ciphertext: &[u8]) -> Result<LoginOutcome, FakeControllerError> {
        let plaintext = self.decrypt(ciphertext)?;
        let (field_number, payload) =
            read_envelope_field(&plaintext)?.ok_or(FakeControllerError::EmptyEnvelope)?;
        if field_number != message_field::LOGIN_RESPONSE {
            return Err(FakeControllerError::UnexpectedField {
                expected: "LoginResponse",
                got: field_number,
            });
        }
        let response = decode_login_response(payload)?;
        Ok(match response {
            va_protocol::messages::LoginResponse::Error(reason) => LoginOutcome::Failed { reason },
            va_protocol::messages::LoginResponse::Success(peer_info) => LoginOutcome::Success {
                hostname: peer_info.hostname,
                platform: peer_info.platform,
            },
        })
    }
}
