//! UDP networking against hbbs: registration and the online keepalive.
//! `va-protocol` owns message shapes/encoding; this crate owns the socket.
//!
//! Field numbers, required fields, and the real client's re-registration
//! cadence are protocol FACTS verified against `hbb_common`'s public
//! `protos/rendezvous.proto` (commit
//! `69cea8dafee147848ae88702029f4bf7df7224c3`) and `rustdesk-server`'s
//! `rendezvous_server.rs` (commit `a7736be5e40f85bfc141120dce587e836e5d4b80`)
//! — see `va-protocol`'s `messages.rs` for the full citation. No `.proto`
//! text or server/client code is copied here.
//!
//! **`RegisterPk` alone does not keep a peer online.** An early version of
//! this crate sent only `RegisterPk`, which hbbs accepted (`OK` every time),
//! yet a real controller's `PunchHoleRequest` for that id still came back
//! "offline" — a real, live-observed bug, not a hypothetical. Reading
//! `rendezvous_server.rs`'s `update_addr` (invoked only from the
//! `RegisterPeer` handler, never from `RegisterPk`'s) explained why: hbbs
//! checks a peer's `last_reg_time` against a 30s offline threshold, and only
//! `RegisterPeer` ever refreshes it. [`RegistrationClient::ensure_online_once`]
//! is the corrected per-cycle operation; [`RegistrationClient::run_keepalive`]
//! drives it forever.
//!
//! **A second live-found bug on top of that one**: `request_pk` (hbbs's
//! signal that it wants a fresh `RegisterPk`) reflects hbbs's OWN state,
//! not whether THIS `RegistrationClient` instance has ever actually sent
//! one. A caller that reuses the same device id with a fresh identity
//! keypair every process start (no persistent identity yet — see the
//! Endpoint Plane build docs) can register successfully, yet the very
//! first real connection attempt fails signature verification, because
//! hbbs is still vouching for a PREVIOUS run's `pk` that this run's
//! identity no longer holds the secret half of. Fixed by always sending
//! `RegisterPk` on THIS client's own first `ensure_online_once` call, not
//! only when hbbs's `request_pk` asks for it — see that method's doc for
//! the full live-reproduction.
//!
//! What a real hbbs forwards to a registered host once a controller sends
//! `PunchHoleRequest` for it was live-captured (a real `PunchHole`, oneof
//! field 9) and, for Support Plane's `ALWAYS_USE_RELAY=Y` deployment (the
//! only branch this host needs), fully resolved by reading the real
//! client's own relay-pairing code — see the [`relay`] module for the
//! implementation and `docs/architecture.md` for the citation trail.
#![allow(clippy::missing_panics_doc, clippy::missing_errors_doc)]

pub mod relay;

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use thiserror::Error;
use tokio::net::UdpSocket;
use tokio::time::timeout;

use va_protocol::{
    decode_register_peer_response, decode_register_pk_response, read_envelope_field,
    rendezvous_field, wrap_message, RegisterPeer, RegisterPk, RegisterPkResult,
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
    #[error("hbbs rejected RegisterPk: {0:?}")]
    Rejected(RegisterPkResult),
    #[error("hbbs did not reply within {0:?}")]
    Timeout(Duration),
    #[error("hbbs replied with an unexpected message (field {0})")]
    UnexpectedReply(u32),
    #[error(
        "hbbs asked for a fresh RegisterPk again immediately after we sent one \
         -- registration is not sticking, this is not the expected steady state"
    )]
    PkStillRequested,
}

/// One registered UDP session against hbbs. Owns the socket; callers drive
/// [`RegistrationClient::ensure_online_once`] on a timer, or call
/// [`RegistrationClient::run_keepalive`] to do that automatically forever.
pub struct RegistrationClient {
    socket: UdpSocket,
    hbbs_addr: SocketAddr,
    id: String,
    uuid: Vec<u8>,
    pk: Vec<u8>,
    /// Set once this client has successfully sent `RegisterPk` for its own
    /// `pk` at least once. See [`ensure_online_once`](Self::ensure_online_once)
    /// for why this can't just defer to hbbs's `request_pk` flag.
    sent_register_pk: AtomicBool,
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
        Ok(Self { socket, hbbs_addr, id, uuid, pk, sent_register_pk: AtomicBool::new(false) })
    }

    #[must_use]
    pub fn hbbs_addr(&self) -> SocketAddr {
        self.hbbs_addr
    }

    /// Sends one `RegisterPeer` and waits up to `RESPONSE_TIMEOUT` for
    /// `RegisterPeerResponse`, returning its `request_pk` flag. This — not
    /// `RegisterPk` — is what refreshes hbbs's `last_reg_time` for this
    /// peer (see the module docs). Prefer
    /// [`ensure_online_once`](Self::ensure_online_once) unless you
    /// specifically need to inspect `request_pk` yourself.
    ///
    /// # Errors
    ///
    /// [`NetError::Timeout`], [`NetError::UnexpectedReply`],
    /// [`NetError::Io`], or [`NetError::Wire`].
    pub async fn register_peer_once(&self) -> Result<bool, NetError> {
        let payload = va_protocol::encode_register_peer(&RegisterPeer {
            id: self.id.clone(),
            // We never apply a server-pushed rendezvous-server-list config
            // (Verevon's Support Plane config is fixed, not server-pushed),
            // so we always report having applied revision 0.
            serial: 0,
        });
        let envelope = wrap_message(rendezvous_field::REGISTER_PEER, &payload);
        self.socket.send(&envelope).await?;

        let (field_number, payload) = self.recv_envelope().await?;
        if field_number != rendezvous_field::REGISTER_PEER_RESPONSE {
            return Err(NetError::UnexpectedReply(field_number));
        }
        Ok(decode_register_peer_response(&payload)?.request_pk)
    }

    /// Sends one `RegisterPk` and waits up to `RESPONSE_TIMEOUT` for
    /// `RegisterPkResponse`. Establishes the id<->pk association; does NOT
    /// by itself keep this peer online for punch-hole routing — call
    /// [`register_peer_once`](Self::register_peer_once) (or
    /// [`ensure_online_once`](Self::ensure_online_once)) for that. Returns
    /// `Ok(())` only for [`RegisterPkResult::Ok`] — every other result
    /// (including one this catalog doesn't recognize) is a
    /// [`NetError::Rejected`] the caller should surface, not silently retry
    /// forever on.
    ///
    /// # Errors
    ///
    /// [`NetError::Timeout`], [`NetError::Rejected`],
    /// [`NetError::UnexpectedReply`], [`NetError::Io`], or
    /// [`NetError::Wire`].
    pub async fn register_pk_once(&self) -> Result<(), NetError> {
        let payload = va_protocol::encode_register_pk(&RegisterPk {
            id: self.id.clone(),
            uuid: self.uuid.clone(),
            pk: self.pk.clone(),
        });
        let envelope = wrap_message(rendezvous_field::REGISTER_PK, &payload);
        self.socket.send(&envelope).await?;

        let (field_number, payload) = self.recv_envelope().await?;
        if field_number != rendezvous_field::REGISTER_PK_RESPONSE {
            return Err(NetError::UnexpectedReply(field_number));
        }
        match decode_register_pk_response(&payload)?.result {
            RegisterPkResult::Ok => {
                self.sent_register_pk.store(true, Ordering::Relaxed);
                Ok(())
            }
            other => Err(NetError::Rejected(other)),
        }
    }

    /// The real per-cycle keepalive operation: sends `RegisterPeer`, and if
    /// hbbs asks for a fresh `pk` (no `pk` on file yet for this id, or this
    /// registration's source IP changed since the last one), sends
    /// `RegisterPk` and then `RegisterPeer` again so `last_reg_time` is
    /// actually refreshed within this same call — hbbs's own handler skips
    /// that update on any `RegisterPeer` call where `request_pk` came back
    /// `true`.
    ///
    /// **Live-discovered edge case this method exists to close**: hbbs's
    /// `request_pk` flag reflects only ITS OWN state (does it already have
    /// a `pk` on file for this `id`, from a possibly-earlier process run
    /// under the same `id`?) — not whether the identity THIS
    /// `RegistrationClient` was constructed with has ever actually been
    /// told to hbbs. A caller that reuses the same device `id` across
    /// restarts with a FRESH keypair each time (no persistent identity —
    /// see `apps/Endpoint Plane/docs/architecture.md` for why that's
    /// currently deferred) can hit exactly this: hbbs still remembers the
    /// PREVIOUS run's `pk` (same `id`, unchanged IP), so `request_pk`
    /// comes back `false` on the very first call, and this client would
    /// silently never register its own new `pk` at all — leaving hbbs
    /// vouching for a key the client no longer holds the secret half of,
    /// which fails signature verification on the controller's end the
    /// instant a real connection is attempted. Live-reproduced against a
    /// real hbbs and a real `remote-core` controller (`Signature
    /// verification failed`) before this fix. Fixed by always sending
    /// `RegisterPk` on this client's OWN first `ensure_online_once` call,
    /// regardless of what `request_pk` says.
    ///
    /// # Errors
    ///
    /// Whatever [`register_peer_once`](Self::register_peer_once) or
    /// [`register_pk_once`](Self::register_pk_once) return, or
    /// [`NetError::PkStillRequested`] if hbbs asks for `RegisterPk` again
    /// immediately after we just sent one (registration not sticking).
    pub async fn ensure_online_once(&self) -> Result<(), NetError> {
        let hbbs_wants_pk = self.register_peer_once().await?;
        let never_sent_our_own_pk = !self.sent_register_pk.load(Ordering::Relaxed);
        if hbbs_wants_pk || never_sent_our_own_pk {
            self.register_pk_once().await?;
            if self.register_peer_once().await? {
                return Err(NetError::PkStillRequested);
            }
        }
        Ok(())
    }

    /// Calls [`ensure_online_once`](Self::ensure_online_once) immediately,
    /// then every [`REGISTRATION_INTERVAL`] forever — returns only on its
    /// first failure, which the caller should treat as fatal (reconnect
    /// from scratch rather than retrying this same loop).
    ///
    /// # Errors
    ///
    /// Whatever [`ensure_online_once`](Self::ensure_online_once) returns on
    /// its first failure.
    pub async fn run_keepalive(&self) -> Result<(), NetError> {
        loop {
            self.ensure_online_once().await?;
            tracing::debug!(hbbs = %self.hbbs_addr, id = %self.id, "online with hbbs");
            tokio::time::sleep(REGISTRATION_INTERVAL).await;
        }
    }

    async fn recv_envelope(&self) -> Result<(u32, Vec<u8>), NetError> {
        let mut buf = [0u8; 1500];
        let n = timeout(RESPONSE_TIMEOUT, self.socket.recv(&mut buf))
            .await
            .map_err(|_elapsed| NetError::Timeout(RESPONSE_TIMEOUT))??;
        let (field_number, payload) =
            read_envelope_field(&buf[..n])?.ok_or(NetError::UnexpectedReply(0))?;
        Ok((field_number, payload.to_vec()))
    }
}
