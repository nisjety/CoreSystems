//! UDP networking against hbbs: `RegisterPk` on startup and every
//! [`REGISTRATION_INTERVAL`], decoding `RegisterPkResponse`. `va-protocol`
//! owns message shapes/encoding; this crate owns the socket.
//!
//! `RegisterPk`'s field numbers, required fields, and the real client's
//! re-registration cadence are protocol FACTS verified against
//! `hbb_common`'s public `protos/rendezvous.proto` (commit
//! `69cea8dafee147848ae88702029f4bf7df7224c3`) and `rustdesk-server`'s
//! `rendezvous_server.rs` (commit `a7736be5e40f85bfc141120dce587e836e5d4b80`)
//! — see `va-protocol`'s `messages.rs` for the full citation. No `.proto`
//! text or server/client code is copied here.
//!
//! What a real hbbs forwards to a registered host once a controller sends
//! `PunchHoleRequest` for it (`PunchHole`/`FetchLocalAddr`, and which
//! channel the host is expected to reply on) is a still-open research gap —
//! deliberately not guessed at or implemented here. See the Endpoint Plane
//! build plan / `docs/architecture.md` for the live-capture test that closes
//! it before that handling is added.
#![allow(clippy::missing_panics_doc, clippy::missing_errors_doc)]

use std::net::SocketAddr;
use std::time::Duration;

use thiserror::Error;
use tokio::net::UdpSocket;
use tokio::time::timeout;

use va_protocol::{
    decode_register_pk_response, read_envelope_field, rendezvous_field, wrap_message, RegisterPk,
    RegisterPkResult,
};

/// The real client's steady-state re-registration cadence (`REG_INTERVAL` in
/// `hbb_common`'s `config.rs`) — comfortably under hbbs's 30s offline
/// threshold (`REG_TIMEOUT`) and its 2-attempts-per-6s abuse limit.
pub const REGISTRATION_INTERVAL: Duration = Duration::from_secs(15);

const RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Error)]
pub enum NetError {
    #[error("network I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("wire decoding failed: {0}")]
    Wire(#[from] va_wire::WireError),
    #[error("hbbs rejected registration: {0:?}")]
    Rejected(RegisterPkResult),
    #[error("hbbs did not reply to RegisterPk within {0:?}")]
    Timeout(Duration),
    #[error("hbbs replied with an unexpected message (field {0})")]
    UnexpectedReply(u32),
}

/// One registered UDP session against hbbs. Owns the socket; callers drive
/// [`RegistrationClient::register_once`] on a timer, or call
/// [`RegistrationClient::run_keepalive`] to do that automatically forever.
pub struct RegistrationClient {
    socket: UdpSocket,
    hbbs_addr: SocketAddr,
    id: String,
    uuid: Vec<u8>,
    pk: Vec<u8>,
}

impl RegistrationClient {
    /// Binds an ephemeral local UDP socket and connects it to `hbbs_addr` —
    /// a connected UDP socket restricts `recv` to datagrams from that exact
    /// address, which is all registration ever needs.
    ///
    /// `pk` should be the same persistent Ed25519 public key later used to
    /// sign `SignedId`: a real client never uses a separate key for
    /// registration vs. the session handshake (verified against its own
    /// source). `uuid` only needs to be a stable, non-empty per-device
    /// value; hbbs never interprets it beyond consistency-checking it
    /// against whatever was stored on a prior registration of the same `id`.
    ///
    /// # Errors
    ///
    /// Returns [`NetError::Io`] if the local socket can't be bound or
    /// `connect` fails.
    pub async fn connect(
        hbbs_addr: SocketAddr,
        id: String,
        uuid: Vec<u8>,
        pk: Vec<u8>,
    ) -> Result<Self, NetError> {
        let bind_addr: SocketAddr = if hbbs_addr.is_ipv6() { "[::]:0" } else { "0.0.0.0:0" }
            .parse()
            .expect("hardcoded wildcard address is always valid");
        let socket = UdpSocket::bind(bind_addr).await?;
        socket.connect(hbbs_addr).await?;
        Ok(Self { socket, hbbs_addr, id, uuid, pk })
    }

    #[must_use]
    pub fn hbbs_addr(&self) -> SocketAddr {
        self.hbbs_addr
    }

    /// Sends one `RegisterPk` and waits up to `RESPONSE_TIMEOUT` for
    /// `RegisterPkResponse`. Returns `Ok(())` only for
    /// [`RegisterPkResult::Ok`] — every other result (including one this
    /// catalog doesn't recognize) is a [`NetError::Rejected`] the caller
    /// should surface, not silently retry forever on.
    ///
    /// # Errors
    ///
    /// [`NetError::Timeout`] if hbbs never replies, [`NetError::Rejected`]
    /// if it replies with a non-`OK` result, [`NetError::UnexpectedReply`]
    /// if it replies with a different message entirely, or [`NetError::Io`]
    /// / [`NetError::Wire`] for a transport or decode failure.
    pub async fn register_once(&self) -> Result<(), NetError> {
        let payload = va_protocol::encode_register_pk(&RegisterPk {
            id: self.id.clone(),
            uuid: self.uuid.clone(),
            pk: self.pk.clone(),
        });
        let envelope = wrap_message(rendezvous_field::REGISTER_PK, &payload);
        self.socket.send(&envelope).await?;

        let mut buf = [0u8; 1500];
        let n = timeout(RESPONSE_TIMEOUT, self.socket.recv(&mut buf))
            .await
            .map_err(|_elapsed| NetError::Timeout(RESPONSE_TIMEOUT))??;

        let (field_number, response_payload) =
            read_envelope_field(&buf[..n])?.ok_or(NetError::UnexpectedReply(0))?;
        if field_number != rendezvous_field::REGISTER_PK_RESPONSE {
            return Err(NetError::UnexpectedReply(field_number));
        }
        let response = decode_register_pk_response(response_payload)?;
        match response.result {
            RegisterPkResult::Ok => Ok(()),
            other => Err(NetError::Rejected(other)),
        }
    }

    /// Registers once immediately, then re-registers every
    /// [`REGISTRATION_INTERVAL`] forever — returns only on the first
    /// [`register_once`](Self::register_once) failure, which the caller
    /// should treat as fatal (reconnect from scratch rather than retrying
    /// this same loop).
    ///
    /// # Errors
    ///
    /// Whatever [`register_once`](Self::register_once) returns on its first
    /// failure.
    pub async fn run_keepalive(&self) -> Result<(), NetError> {
        loop {
            self.register_once().await?;
            tracing::debug!(hbbs = %self.hbbs_addr, id = %self.id, "RegisterPk accepted");
            tokio::time::sleep(REGISTRATION_INTERVAL).await;
        }
    }
}
