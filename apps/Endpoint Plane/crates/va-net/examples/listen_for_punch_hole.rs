//! Diagnostic tool, not a test: registers a known device id against a real
//! local hbbs (the full `RegisterPeer`/`RegisterPk` dance, so the peer is
//! actually online — see `va-net`'s crate docs for why `RegisterPk` alone
//! isn't enough), then listens on the SAME UDP socket for whatever hbbs
//! forwards next, dumping the raw bytes and decoded top-level field number.
//!
//! Exists to empirically close the M2b research gap documented in
//! `docs/architecture.md`: what a host receives (and on which channel) when
//! hbbs relays a controller's `PunchHoleRequest` for it. Uses a raw socket
//! directly (rather than `RegistrationClient`, which deliberately doesn't
//! expose raw `recv` — that's not part of its real API yet, pending the
//! M2b design question this tool is helping answer) so this one-off
//! exploration doesn't widen the crate's real surface.
//!
//! Run this, then drive a real connection attempt from `apps/remote-dev`
//! (or any other `remote-core`-based controller) against the same device id
//! while it's running:
//!
//! ```bash
//! cargo run -p va-net --example listen_for_punch_hole -- 999888777
//! ```

use std::env;
use std::net::SocketAddr;
use std::time::Duration;

use tokio::net::UdpSocket;
use va_crypto::IdentityKeyPair;
use va_protocol::{rendezvous_field, wrap_message, RegisterPeer, RegisterPk};
use va_wire::ProtoReader;

const REG_INTERVAL: Duration = Duration::from_secs(10);

#[tokio::main]
async fn main() {
    let id = env::args().nth(1).unwrap_or_else(|| "999888777".to_string());
    let hbbs_addr: SocketAddr = "127.0.0.1:21116".parse().unwrap();

    let identity = IdentityKeyPair::generate();
    let pk = identity.public_key().to_vec();
    println!("registering id={id} pk={}", hex_encode(&pk));

    let socket = UdpSocket::bind("0.0.0.0:0").await.expect("bind local socket");
    socket.connect(hbbs_addr).await.expect("connect to local hbbs");

    ensure_online(&socket, &id, &pk).await;
    println!("online with hbbs -- now waiting up to 120s for hbbs to forward anything else on this same UDP socket...");
    println!("(drive a real connection attempt against id={id} now, e.g. via apps/remote-dev)");

    let deadline = tokio::time::Instant::now() + Duration::from_mins(2);
    let mut next_reregister = tokio::time::Instant::now() + REG_INTERVAL;
    let mut buf = [0u8; 2048];
    loop {
        let now = tokio::time::Instant::now();
        if now >= deadline {
            println!("timed out waiting -- nothing else arrived on this socket.");
            break;
        }
        let wait = deadline.min(next_reregister).saturating_duration_since(now);
        match tokio::time::timeout(wait, socket.recv(&mut buf)).await {
            Ok(Ok(n)) => {
                let bytes = &buf[..n];
                println!("\n=== received {n} bytes ===");
                println!("hex: {}", hex_encode(bytes));
                dump_top_level_field(bytes);
            }
            Ok(Err(err)) => {
                println!("recv error: {err}");
                break;
            }
            Err(_elapsed) => {
                if tokio::time::Instant::now() >= next_reregister {
                    ensure_online(&socket, &id, &pk).await;
                    next_reregister = tokio::time::Instant::now() + REG_INTERVAL;
                }
            }
        }
    }
}

/// The real per-cycle keepalive: `RegisterPeer` first (this is what
/// actually refreshes hbbs's `last_reg_time`), and only send `RegisterPk`
/// when `request_pk` asks for it -- mirrors
/// `RegistrationClient::ensure_online_once`, reimplemented here on a raw
/// socket since this diagnostic doesn't go through `va-net`'s real API.
async fn ensure_online(socket: &UdpSocket, id: &str, pk: &[u8]) {
    if register_peer(socket, id).await {
        register_pk(socket, id, pk).await;
        let still_wants_pk = register_peer(socket, id).await;
        println!("[ensure_online] request_pk after RegisterPk: {still_wants_pk}");
    }
}

async fn register_peer(socket: &UdpSocket, id: &str) -> bool {
    let payload = va_protocol::encode_register_peer(&RegisterPeer { id: id.to_string(), serial: 0 });
    let envelope = wrap_message(rendezvous_field::REGISTER_PEER, &payload);
    socket.send(&envelope).await.expect("send RegisterPeer");

    let mut buf = [0u8; 512];
    match tokio::time::timeout(Duration::from_secs(5), socket.recv(&mut buf)).await {
        Ok(Ok(n)) => {
            println!("[register_peer] reply: {}", hex_encode(&buf[..n]));
            match va_protocol::read_envelope_field(&buf[..n]) {
                Ok(Some((field, payload))) if field == rendezvous_field::REGISTER_PEER_RESPONSE => {
                    va_protocol::decode_register_peer_response(payload)
                        .map_or(true, |r| r.request_pk)
                }
                other => {
                    println!("[register_peer] unexpected envelope: {other:?}");
                    true
                }
            }
        }
        Ok(Err(err)) => {
            println!("[register_peer] recv error: {err}");
            true
        }
        Err(_) => {
            println!("[register_peer] no reply within 5s");
            true
        }
    }
}

async fn register_pk(socket: &UdpSocket, id: &str, pk: &[u8]) {
    let payload = va_protocol::encode_register_pk(&RegisterPk {
        id: id.to_string(),
        uuid: pk.to_vec(),
        pk: pk.to_vec(),
    });
    let envelope = wrap_message(rendezvous_field::REGISTER_PK, &payload);
    socket.send(&envelope).await.expect("send RegisterPk");

    let mut buf = [0u8; 512];
    match tokio::time::timeout(Duration::from_secs(5), socket.recv(&mut buf)).await {
        Ok(Ok(n)) => println!("[register_pk] reply: {}", hex_encode(&buf[..n])),
        Ok(Err(err)) => println!("[register_pk] recv error: {err}"),
        Err(_) => println!("[register_pk] no reply within 5s"),
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        write!(s, "{b:02x}").unwrap();
    }
    s
}

fn dump_top_level_field(bytes: &[u8]) {
    if bytes.is_empty() {
        println!("(empty datagram)");
        return;
    }
    let mut r = ProtoReader::new(bytes);
    match r.read_tag() {
        Ok(tag) => println!(
            "top-level field_number={} wire_type={} (RendezvousMessage oneof variant)",
            tag.field_number, tag.wire_type
        ),
        Err(err) => println!("failed to decode a tag: {err}"),
    }
}
