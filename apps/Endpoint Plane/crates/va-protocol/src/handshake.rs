//! The host-side per-connection state machine: handshake -> login -> session
//! pump. I/O-agnostic — callers feed it received bytes and get back bytes to
//! send plus events; no sockets, no tokio. This is what `va-testkit`'s
//! `FakeController` drives in tests, and what `va-net` drives with real
//! sockets.
//!
//! Sequence (host's perspective): generate an ephemeral X25519 keypair, sign
//! `IdPk{id, ephemeral_pk}` with the identity key, send `SignedId` in
//! PLAINTEXT. Receive `PublicKey{asymmetric_value, symmetric_value}` in
//! plaintext; unseal `symmetric_value` with the ephemeral keypair to recover
//! the session key `K`. From here every `Message` is encrypted. Send `Hash`
//! unprompted. Receive `LoginRequest`; check its password hash against the
//! salt/challenge just sent and whichever secret(s) this device accepts.
//! Send `LoginResponse` (success carries `PeerInfo`). Then: echo any
//! `TestDelay{from_client: true}` verbatim, and allow the caller to push
//! video frames — permissions start and stay at `screen.view` only in this
//! phase (no `PermissionInfo` push), matching what a real stock `RustDesk`
//! host was observed doing for a one-time-password connection.

use thiserror::Error;
use va_crypto::{unseal, CryptoError, EphemeralKeyPair, IdentityKeyPair, SessionCipher};

use crate::messages::{
    decode_login_request, decode_public_key, decode_test_delay, encode_hash,
    encode_login_response, encode_signed_id, encode_test_delay, message_field,
    read_envelope_field, wrap_message, DisplayInfo, Hash, LoginResponse, PeerInfo, TestDelay,
};

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("wire decoding failed: {0}")]
    Wire(#[from] va_wire::WireError),
    #[error("crypto operation failed: {0}")]
    Crypto(#[from] CryptoError),
    #[error("received {got}, expected {expected} for the current handshake step")]
    UnexpectedMessage { expected: &'static str, got: u32 },
    #[error("empty envelope where a message was expected")]
    EmptyEnvelope,
    #[error("message received before the session key was established")]
    NotYetEncrypted,
}

/// What this device will accept as valid credentials. Real secure storage
/// and rotation policy are a later phase's concern (see the build plan's
/// "explicitly deferred" list) — this is deliberately just enough for M2's
/// fixed-device-id integration test.
pub struct Credentials {
    pub permanent_password: Option<String>,
    pub temporary_password: Option<String>,
}

impl Credentials {
    #[must_use]
    fn accepts(&self, salt: &str, challenge: &str, candidate_hash: &[u8]) -> bool {
        for secret in [&self.permanent_password, &self.temporary_password]
            .into_iter()
            .flatten()
        {
            if va_crypto::hash_password(secret, salt, challenge) == candidate_hash {
                return true;
            }
        }
        false
    }
}

pub struct HostIdentity {
    pub keypair: IdentityKeyPair,
    pub device_id: String,
}

pub struct HostInfo {
    pub hostname: String,
    pub platform: String,
    pub displays: Vec<DisplayInfo>,
    pub version: String,
}

/// One outcome of feeding a received message into the state machine. The
/// caller always sends `response` (already encrypted) when present,
/// regardless of which variant it came wrapped in — the variant tells the
/// caller what HAPPENED, not just what to transmit.
pub enum Event {
    /// Bytes the caller must send back, still needing encryption via the
    /// now-established session cipher's send side.
    SendEncrypted(Vec<u8>),
    /// Login succeeded; `response` is the `LoginResponse::Success` envelope
    /// to send, and the session is now in its steady-state pump.
    LoggedIn { response: Vec<u8> },
    /// Login failed with the given wire error string (e.g. "Wrong
    /// Password") — still a normal, non-fatal outcome. `response` is the
    /// `LoginResponse::Error` envelope to send; the client may retry on the
    /// same connection.
    LoginFailed { reason: String, response: Vec<u8> },
    /// Nothing to send back for this message (e.g. the echo of our own
    /// outbound `TestDelay` probe, which needs no further reply).
    None,
}

enum State {
    AwaitingPublicKey { ephemeral: EphemeralKeyPair },
    AwaitingLoginRequest { salt: String, challenge: String },
    Connected,
}

pub struct HostSession {
    identity: HostIdentity,
    info: HostInfo,
    credentials: Credentials,
    cipher: Option<SessionCipher>,
    state: State,
}

impl HostSession {
    /// Starts a new connection attempt: generates a fresh ephemeral keypair,
    /// signs `IdPk{id, ephemeral_pk}`, and returns the `SignedId` `Message`
    /// envelope bytes to send in PLAINTEXT (the only message this state
    /// machine ever asks the caller to send unencrypted).
    #[must_use]
    pub fn start(identity: HostIdentity, info: HostInfo, credentials: Credentials) -> (Self, Vec<u8>) {
        let ephemeral = EphemeralKeyPair::generate();
        let id_pk = crate::messages::encode_id_pk(&crate::messages::IdPk {
            id: identity.device_id.clone(),
            pk: ephemeral.public_key().to_vec(),
        });
        let signed = identity.keypair.sign(&id_pk);
        let envelope = wrap_message(message_field::SIGNED_ID, &encode_signed_id(&signed));

        let session = Self {
            identity,
            info,
            credentials,
            cipher: None,
            state: State::AwaitingPublicKey { ephemeral },
        };
        (session, envelope)
    }

    /// Feeds one PLAINTEXT `Message` envelope (only ever the controller's
    /// `PublicKey` reply, in this state machine's lifetime). Establishes the
    /// session cipher and returns the `Hash` challenge to send — already
    /// encrypted, since the channel is live from this point on.
    pub fn on_plaintext_message(&mut self, envelope: &[u8]) -> Result<Vec<u8>, ProtocolError> {
        let (field_number, payload) =
            read_envelope_field(envelope)?.ok_or(ProtocolError::EmptyEnvelope)?;
        if field_number != message_field::PUBLIC_KEY {
            return Err(ProtocolError::UnexpectedMessage {
                expected: "PublicKey",
                got: field_number,
            });
        }
        let State::AwaitingPublicKey { ephemeral } = &self.state else {
            return Err(ProtocolError::UnexpectedMessage {
                expected: "PublicKey (already past this step)",
                got: field_number,
            });
        };
        let public_key = decode_public_key(payload)?;
        let session_key_bytes = unseal(ephemeral, &public_key.symmetric_value)?;
        let mut session_key = [0u8; 32];
        session_key.copy_from_slice(&session_key_bytes[..32.min(session_key_bytes.len())]);
        let cipher = SessionCipher::new(session_key);

        let salt = format!("salt-{}", self.identity.device_id);
        let challenge = generate_challenge();
        let hash_envelope = wrap_message(
            message_field::HASH,
            &encode_hash(&Hash {
                salt: salt.clone(),
                challenge: challenge.clone(),
            }),
        );

        self.cipher = Some(cipher);
        self.state = State::AwaitingLoginRequest { salt, challenge };

        self.encrypt(&hash_envelope)
    }

    /// Feeds one ENCRYPTED `Message` (everything from `LoginRequest`
    /// onward). Decrypts, dispatches on the current state, and returns what
    /// (if anything) to send back — still encrypted, ready for the wire.
    pub fn on_encrypted_message(&mut self, ciphertext: &[u8]) -> Result<Event, ProtocolError> {
        let plaintext = {
            let cipher = self.cipher.as_mut().ok_or(ProtocolError::NotYetEncrypted)?;
            cipher.recv.open(ciphertext)?
        };
        let (field_number, payload) =
            read_envelope_field(&plaintext)?.ok_or(ProtocolError::EmptyEnvelope)?;

        match &self.state {
            State::AwaitingLoginRequest { salt, challenge } => {
                if field_number != message_field::LOGIN_REQUEST {
                    return Err(ProtocolError::UnexpectedMessage {
                        expected: "LoginRequest",
                        got: field_number,
                    });
                }
                let request = decode_login_request(payload)?;
                if self.credentials.accepts(salt, challenge, &request.password) {
                    let response = encode_login_response(&LoginResponse::Success(PeerInfo {
                        username: self.identity.device_id.clone(),
                        hostname: self.info.hostname.clone(),
                        platform: self.info.platform.clone(),
                        displays: std::mem::take(&mut self.info.displays),
                        current_display: 0,
                        sas_enabled: false,
                        version: self.info.version.clone(),
                    }));
                    self.state = State::Connected;
                    let envelope = wrap_message(message_field::LOGIN_RESPONSE, &response);
                    Ok(Event::LoggedIn { response: self.encrypt(&envelope)? })
                } else {
                    let response = encode_login_response(&LoginResponse::Error("Wrong Password".into()));
                    let envelope = wrap_message(message_field::LOGIN_RESPONSE, &response);
                    // Stay in AwaitingLoginRequest: a real client may retry
                    // with a corrected password on the same connection.
                    Ok(Event::LoginFailed {
                        reason: "Wrong Password".into(),
                        response: self.encrypt(&envelope)?,
                    })
                }
            }
            State::Connected => self.handle_session_message(field_number, payload),
            State::AwaitingPublicKey { .. } => Err(ProtocolError::NotYetEncrypted),
        }
    }

    fn handle_session_message(&mut self, field_number: u32, payload: &[u8]) -> Result<Event, ProtocolError> {
        if field_number == message_field::TEST_DELAY {
            let probe = decode_test_delay(payload)?;
            if probe.from_client {
                // The client's own RTT probe: echo verbatim, per the wire
                // contract ("echo it back unchanged").
                let envelope = wrap_message(message_field::TEST_DELAY, &encode_test_delay(probe));
                return Ok(Event::SendEncrypted(self.encrypt(&envelope)?));
            }
            // Our own probe being echoed back: nothing further to send;
            // va-net's caller is responsible for using the round-trip time.
            return Ok(Event::None);
        }
        // Unknown/unhandled field for this phase (e.g. pointer/keyboard
        // actions this phase never grants permission for) — ignored rather
        // than erroring, matching proto3's forward-compatibility contract.
        Ok(Event::None)
    }

    /// Builds a host-initiated probe to send periodically (roughly once a
    /// second) once connected, per the wire contract that drives the
    /// client's adaptive bitrate.
    pub fn build_host_probe(&mut self, last_delay_ms: u32, target_bitrate_kbps: u32) -> Result<Vec<u8>, ProtocolError> {
        let envelope = wrap_message(
            message_field::TEST_DELAY,
            &encode_test_delay(TestDelay {
                time: current_time_ms(),
                from_client: false,
                last_delay: last_delay_ms,
                target_bitrate: target_bitrate_kbps,
            }),
        );
        self.encrypt(&envelope)
    }

    /// Encrypts and wraps a video-frame envelope (built by the caller via
    /// `messages::encode_vp9_video_frame` + `wrap_message`), ready to send.
    pub fn encrypt_for_send(&mut self, envelope: &[u8]) -> Result<Vec<u8>, ProtocolError> {
        self.encrypt(envelope)
    }

    #[must_use]
    pub fn is_connected(&self) -> bool {
        matches!(self.state, State::Connected)
    }

    fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, ProtocolError> {
        let cipher = self.cipher.as_mut().ok_or(ProtocolError::NotYetEncrypted)?;
        Ok(cipher.send.seal(plaintext))
    }
}

fn generate_challenge() -> String {
    use std::fmt::Write;
    use rand::RngCore;
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    let mut hex = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        // Infallible: writing to a String never fails.
        write!(hex, "{b:02x}").unwrap();
    }
    hex
}

/// Milliseconds since the Unix epoch, for `TestDelay.time`. Not a source of
/// non-determinism callers need to worry about — it only ever rides the
/// wire as an opaque round-trip marker, never compared across processes.
/// The u128->i64 truncation is intentional and harmless here: milliseconds
/// since 1970 won't exceed i64's range for roughly 292 million years.
#[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
fn current_time_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}
