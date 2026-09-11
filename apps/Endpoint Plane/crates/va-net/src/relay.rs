//! Relay pairing: answering a forwarded `PunchHole` when its `nat_type` is
//! [`va_protocol::NAT_TYPE_SYMMETRIC`] — the only branch Verevon's own
//! deployment ever takes, since Support Plane sets `ALWAYS_USE_RELAY=Y`.
//!
//! The sequence is read directly from `rustdesk`'s own
//! `src/rendezvous_mediator.rs` (`handle_punch_hole`/`create_relay`) and
//! `src/server.rs` (`create_relay_connection_`) at commit
//! `1ec1b9e7e3808b24bfc923ee1e6800ab9694e9a5` — see `va-protocol`'s
//! `messages.rs` and `docs/architecture.md` for the full citation trail and
//! the live `PunchHole` capture that motivated it:
//!
//! 1. Open a fresh, short-lived TCP connection to hbbs (the SAME
//!    host:port already used for UDP registration — port 21116 accepts
//!    both transports for the rendezvous protocol) and send `RelayResponse`
//!    — no reply is read, and this connection is never reused.
//! 2. Open a SEPARATE TCP connection to hbbr and send `RequestRelay` as the
//!    very first message on it. From that point on, the SAME stream IS the
//!    raw peer-session channel — feed its bytes into
//!    [`va_protocol::HostSession`] exactly as `va-testkit`'s
//!    `FakeController` already exercises against a fake stream.

use std::net::SocketAddr;

use rand::RngCore;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

use va_protocol::{rendezvous_field, wrap_message, RelayResponse, RequestRelay};

use crate::NetError;

/// Answers a forwarded [`va_protocol::PunchHole`] whose `nat_type` was
/// [`va_protocol::NAT_TYPE_SYMMETRIC`], completing the relay pairing and
/// returning the TCP stream to hbbr — already past the `RequestRelay`
/// handshake, ready to carry the raw peer-session protocol
/// ([`va_protocol::HostSession`]).
///
/// `punch_hole_socket_addr` must be [`va_protocol::PunchHole::socket_addr`]
/// UNCHANGED — it is an `AddrMangle`-obfuscated encoding this host never
/// needs to (and must not) decode itself; hbbs uses it to correlate this
/// reply back to the original controller.
///
/// # Errors
///
/// [`NetError::Io`] if either TCP connection fails, or [`NetError::Wire`]
/// if framing the outgoing messages fails (it never should, for
/// well-formed inputs).
pub async fn respond_via_relay(
    hbbs_addr: SocketAddr,
    relay_addr: SocketAddr,
    device_id: &str,
    licence_key: &str,
    punch_hole_socket_addr: Vec<u8>,
) -> Result<TcpStream, NetError> {
    let uuid = generate_uuid_v4();

    {
        let mut hbbs_sock = TcpStream::connect(hbbs_addr).await?;
        let payload = va_protocol::encode_relay_response(&RelayResponse {
            socket_addr: punch_hole_socket_addr,
            uuid: uuid.clone(),
            relay_server: relay_addr.to_string(),
            id: device_id.to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        });
        let envelope = wrap_message(rendezvous_field::RELAY_RESPONSE, &payload);
        let frame = va_wire::encode_frame(&envelope)?;
        hbbs_sock.write_all(&frame).await?;
        // Deliberately no read here, and `hbbs_sock` is dropped at the end
        // of this block: the real client never reads a reply on this
        // connection or reuses it — see the module docs.
    }

    let mut relay_sock = TcpStream::connect(relay_addr).await?;
    let payload = va_protocol::encode_request_relay(&RequestRelay {
        uuid,
        licence_key: licence_key.to_string(),
    });
    let envelope = wrap_message(rendezvous_field::REQUEST_RELAY, &payload);
    let frame = va_wire::encode_frame(&envelope)?;
    relay_sock.write_all(&frame).await?;

    Ok(relay_sock)
}

/// A random RFC 4122 version-4 UUID string. hbbr only ever compares this
/// value for equality between the two connections it pairs — the exact RNG
/// doesn't matter, only that the format matches what a real client sends
/// (`Uuid::new_v4().to_string()`), which this reproduces byte-for-shape
/// without pulling in the `uuid` crate for one call site.
fn generate_uuid_v4() -> String {
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    bytes[6] = (bytes[6] & 0x0f) | 0x40; // version 4
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // RFC 4122 variant
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5],
        bytes[6], bytes[7],
        bytes[8], bytes[9],
        bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
    )
}

#[cfg(test)]
mod tests {
    use super::generate_uuid_v4;

    #[test]
    fn generated_uuid_matches_the_standard_hyphenated_shape() {
        let uuid = generate_uuid_v4();
        let parts: Vec<&str> = uuid.split('-').collect();
        assert_eq!(parts.len(), 5, "{uuid}");
        assert_eq!([parts[0].len(), parts[1].len(), parts[2].len(), parts[3].len(), parts[4].len()], [8, 4, 4, 4, 12]);
        assert_eq!(&parts[2][..1], "4", "version nibble must be 4: {uuid}");
        assert!(
            matches!(parts[3].chars().next(), Some('8' | '9' | 'a' | 'b')),
            "variant nibble must be 8/9/a/b: {uuid}"
        );
    }

    #[test]
    fn two_generated_uuids_are_not_equal() {
        assert_ne!(generate_uuid_v4(), generate_uuid_v4());
    }
}
