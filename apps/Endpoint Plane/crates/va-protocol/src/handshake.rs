//! The host-side per-connection state machine: handshake -> login -> session
//! pump. I/O-agnostic — callers feed it received bytes and get back bytes to
//! send plus events; no sockets, no tokio. This is what `va-testkit`'s
//! `FakeController` drives in tests, and what `va-net` drives with real
//! sockets.
//!
//! Sequence (host's perspective): generate an ephemeral X25519 keypair, sign
//! `IdPk{id, ephemeral_pk}` with the identity key, send `SignedId` in
//! PLAINTEXT. Receive `PublicKey{asymmetric_value, symmetric_value}` in
//! plaintext; `asymmetric_value` is the CONTROLLER's own ephemeral public
//! key (needed for `va_crypto::unseal`'s X25519 DH — this is a real
//! authenticated `crypto_box`, not an anonymous sealed box; see
//! `va-crypto`'s docs for the live interop bug that discovered this),
//! `symmetric_value` is the session key sealed to it, recovered via
//! `unseal(our_ephemeral, their_ephemeral_public, symmetric_value)`. From
//! here every `Message` is encrypted. Send `Hash`
//! unprompted. Receive `LoginRequest`; check its password hash against the
//! salt/challenge just sent and whichever secret(s) this device accepts.
//! Send `LoginResponse` (success carries `PeerInfo`). Then: echo any
//! `TestDelay{from_client: true}` verbatim, allow the caller to push video
//! frames, and decode any `MouseEvent`/`KeyEvent` the controller sends —
//! but ONLY into `Event::MouseInput`/`Event::KeyInput` once input control
//! has actually been granted; otherwise into `Event::InputDenied`. `screen.
//! view` is implicit from the moment login succeeds (video always flows);
//! input control is not, and is never implied by login success alone —
//! see `Event::LoggedIn::requires_consent` and `grant_input_permission`/
//! `deny_input_permission` for the actual model. This state machine still
//! has no I/O of its own: it hands `requires_consent` upward and waits to
//! be told the answer, rather than showing any prompt itself. A permanent
//! password means `requires_consent: false` (pre-authorized, unattended
//! access — the caller may grant immediately); a temporary one means
//! `requires_consent: true` (real local operator consent is needed first) —
//! this distinction is this crate's own design, not something verified
//! against a live stock `RustDesk` host (unlike most of this file, whose
//! wire-level facts ARE independently confirmed).

use thiserror::Error;
use va_crypto::{unseal, CryptoError, EphemeralKeyPair, IdentityKeyPair, SessionCipher};

use crate::messages::{
    decode_key_event, decode_login_request, decode_mouse_event, decode_public_key,
    decode_switch_display_request, decode_test_delay, encode_hash, encode_login_response,
    encode_permission_info, encode_signed_id, encode_switch_display, encode_test_delay,
    message_field, read_envelope_field, wrap_message, DisplayInfo, Hash, KeyEvent, LoginResponse,
    MouseEvent, PeerInfo, Permission, SwitchDisplay, TestDelay,
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
    #[error("PublicKey.asymmetric_value was {0} bytes, expected exactly 32 (an X25519 public key)")]
    MalformedPublicKey(usize),
}

/// What this device will accept as valid credentials. Real secure storage
/// and rotation policy are a later phase's concern (see the build plan's
/// "explicitly deferred" list) — this is deliberately just enough for M2's
/// fixed-device-id integration test.
pub struct Credentials {
    pub permanent_password: Option<String>,
    pub temporary_password: Option<String>,
}

/// Which of the two secrets a successful login actually matched — this is
/// what decides whether the new session needs local operator consent
/// before input control is granted (see `Event::LoggedIn::requires_consent`
/// and this module's own docs). A permanent password models pre-authorized,
/// unattended access (the operator configured it in advance, specifically
/// to skip a prompt); a temporary password models one-time, attended
/// access, where nobody agreed to anything in advance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CredentialKind {
    Permanent,
    Temporary,
}

impl Credentials {
    #[must_use]
    fn accepts(&self, salt: &str, challenge: &str, candidate_hash: &[u8]) -> Option<CredentialKind> {
        if let Some(secret) = &self.permanent_password {
            if va_crypto::hash_password(secret, salt, challenge) == candidate_hash {
                return Some(CredentialKind::Permanent);
            }
        }
        if let Some(secret) = &self.temporary_password {
            if va_crypto::hash_password(secret, salt, challenge) == candidate_hash {
                return Some(CredentialKind::Temporary);
            }
        }
        None
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
    /// `requires_consent` tells the caller whether this login used the
    /// TEMPORARY password (`true` — a real local operator must explicitly
    /// approve input control before `grant_input_permission` is called) or
    /// the PERMANENT one (`false` — pre-authorized, unattended access; the
    /// caller may call `grant_input_permission` immediately with no
    /// prompt). This state machine never shows a prompt itself — it has no
    /// I/O — see `grant_input_permission`/`deny_input_permission`.
    LoggedIn { response: Vec<u8>, requires_consent: bool },
    /// Login failed with the given wire error string (e.g. "Wrong
    /// Password") — still a normal, non-fatal outcome. `response` is the
    /// `LoginResponse::Error` envelope to send; the client may retry on the
    /// same connection.
    LoginFailed { reason: String, response: Vec<u8> },
    /// A pointer action the controller sent, ALREADY checked against this
    /// session's own granted-permission state — this only ever fires once
    /// `grant_input_permission` has been called. The caller still decides
    /// whether to actually inject it (e.g. via `va-input`); this state
    /// machine just won't hand it up at all otherwise (see `InputDenied`).
    MouseInput(MouseEvent),
    /// A keyboard action the controller sent. Same gating as `MouseInput`.
    KeyInput(KeyEvent),
    /// A `MouseEvent`/`KeyEvent` arrived but input control isn't granted
    /// (no consent given yet, denied, or revoked) — surfaced as its own
    /// variant rather than folded into `None` so a caller can distinguish
    /// "nothing happened" from "the controller tried to do something it
    /// isn't allowed to do" for logging/telemetry.
    InputDenied,
    /// The controller asked to switch the streamed display (`Misc.
    /// switch_display` carrying only the target index). This state
    /// machine has no capture/monitor knowledge of its own -- the caller
    /// decides whether the index is valid, does whatever real work
    /// switching implies, and calls `confirm_display_switch` with the new
    /// display's real geometry once it's actually active.
    DisplaySwitchRequested { display: i32 },
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
    /// Whether the controller may currently send `MouseEvent`/`KeyEvent`.
    /// Always starts `false` on a fresh login, regardless of which
    /// credential was used — `grant_input_permission` is a separate,
    /// explicit step the caller takes (immediately for a permanent
    /// password, after real operator consent for a temporary one), never
    /// implied by login success alone.
    input_granted: bool,
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
            input_granted: false,
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
        let controller_ephemeral_public: [u8; 32] = public_key
            .asymmetric_value
            .clone()
            .try_into()
            .map_err(|v: Vec<u8>| ProtocolError::MalformedPublicKey(v.len()))?;
        let session_key_bytes = unseal(ephemeral, &controller_ephemeral_public, &public_key.symmetric_value)?;
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
                if let Some(kind) = self.credentials.accepts(salt, challenge, &request.password) {
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
                    self.input_granted = false;
                    let envelope = wrap_message(message_field::LOGIN_RESPONSE, &response);
                    Ok(Event::LoggedIn {
                        response: self.encrypt(&envelope)?,
                        requires_consent: kind == CredentialKind::Temporary,
                    })
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
        if field_number == message_field::MOUSE_EVENT {
            if !self.input_granted {
                return Ok(Event::InputDenied);
            }
            return Ok(Event::MouseInput(decode_mouse_event(payload)?));
        }
        if field_number == message_field::KEY_EVENT {
            if !self.input_granted {
                return Ok(Event::InputDenied);
            }
            return Ok(Event::KeyInput(decode_key_event(payload)?));
        }
        if field_number == message_field::MISC {
            if let Some(display) = decode_switch_display_request(payload)? {
                return Ok(Event::DisplaySwitchRequested { display });
            }
            // Some other Misc variant this phase doesn't act on -- ignored,
            // matching proto3 forward-compatibility.
            return Ok(Event::None);
        }
        // Unknown/unhandled field for this phase — ignored rather than
        // erroring, matching proto3's forward-compatibility contract.
        Ok(Event::None)
    }

    /// Grants input control: the caller decides WHEN to call this (see
    /// `Event::LoggedIn::requires_consent`) — immediately for a permanent
    /// password, or after real local operator consent for a temporary one.
    /// Returns the encrypted `PermissionInfo{Keyboard, true}` envelope to
    /// send, so the controller's own permission UI reflects reality (both
    /// sides already had this wire message; nothing sent it until now).
    ///
    /// # Errors
    ///
    /// [`ProtocolError::NotYetEncrypted`] if called before the session
    /// cipher is established (shouldn't happen: this is only meaningful
    /// once `Event::LoggedIn` has already fired, which implies it is).
    pub fn grant_input_permission(&mut self) -> Result<Vec<u8>, ProtocolError> {
        self.input_granted = true;
        self.encode_permission_info(true)
    }

    /// Explicitly denies input control (consent refused, or a timeout with
    /// no answer) — distinct from simply never calling
    /// `grant_input_permission`, because this ALSO tells the controller so
    /// its UI can show "access denied" rather than sitting in an
    /// indefinite, unexplained "waiting" state. Idempotent: safe to call
    /// even though `input_granted` already defaults to `false`.
    ///
    /// # Errors
    ///
    /// Same as `grant_input_permission`.
    pub fn deny_input_permission(&mut self) -> Result<Vec<u8>, ProtocolError> {
        self.input_granted = false;
        self.encode_permission_info(false)
    }

    fn encode_permission_info(&mut self, enabled: bool) -> Result<Vec<u8>, ProtocolError> {
        let envelope = wrap_message(message_field::MISC, &encode_permission_info(Permission::Keyboard, enabled));
        self.encrypt(&envelope)
    }

    /// Confirms a display switch with the new display's REAL geometry --
    /// this state machine has no capture/monitor knowledge of its own, so
    /// the caller (which does) builds `display` from whatever it actually
    /// switched to, not necessarily the index the controller requested
    /// (e.g. an out-of-range request could be clamped, or ignored in favor
    /// of whatever was already active -- that policy is entirely the
    /// caller's call).
    ///
    /// # Errors
    ///
    /// Same as `grant_input_permission`.
    pub fn confirm_display_switch(&mut self, display: &SwitchDisplay) -> Result<Vec<u8>, ProtocolError> {
        let envelope = wrap_message(message_field::MISC, &encode_switch_display(display));
        self.encrypt(&envelope)
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
