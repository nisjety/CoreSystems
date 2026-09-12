//! Live demo, not a test: registers a device id against a real local hbbs
//! (the full `RegisterPeer`/`RegisterPk` dance, so the peer is actually
//! online — see `va-net`'s crate docs for why `RegisterPk` alone isn't
//! enough), waits for hbbs to forward a `PunchHole`, answers it via
//! [`va_net::relay::respond_via_relay`], and then runs a REAL
//! `HostSession` handshake + login over the resulting relay stream against
//! whatever real controller triggered it (e.g. `apps/remote-dev`).
//!
//! This is the capstone live proof for M2a/M2b/M2c: every piece here
//! (registration, the `PunchHole` capture, the relay-pairing sequence, and
//! `HostSession` itself) was built and unit-tested independently; this is
//! what proves they compose into a real, working connection. See
//! `docs/architecture.md` for the full research trail.
//!
//! ```bash
//! cargo run -p va-net --example listen_for_punch_hole -- <device-id> <password> <licence-key>
//! ```
//!
//! Then drive a real connection attempt from `apps/remote-dev` (device id +
//! the SAME password) while it's running.

use std::env;
use std::net::SocketAddr;
use std::time::Duration;

use bytes::BytesMut;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UdpSocket;
use va_crypto::IdentityKeyPair;
use va_protocol::{
    rendezvous_field, wrap_message, Credentials, DisplayInfo, Event, HostIdentity, HostInfo,
    HostSession, PunchHole, RegisterPeer, RegisterPk, NAT_TYPE_SYMMETRIC,
};

const REG_INTERVAL: Duration = Duration::from_secs(10);

#[tokio::main]
async fn main() {
    let mut args = env::args().skip(1);
    let id = args.next().unwrap_or_else(|| "999888777".to_string());
    let password = args.next().unwrap_or_else(|| "testpassword".to_string());
    let licence_key = args.next().unwrap_or_else(|| {
        eprintln!(
            "usage: listen_for_punch_hole <device-id> <password> <licence-key>\n\
             licence-key is hbbs's own logged `Key:` value -- never a value \
             worth hardcoding as a default here, even a local dev one. Get it with:\n\
             docker compose logs hbbs | grep 'Key:'"
        );
        std::process::exit(1);
    });

    let hbbs_addr: SocketAddr = "127.0.0.1:21116".parse().unwrap();
    let relay_addr: SocketAddr = "127.0.0.1:21117".parse().unwrap();

    let identity = IdentityKeyPair::generate();
    let pk = identity.public_key().to_vec();
    println!("registering id={id} pk={}", hex_encode(&pk));

    let socket = UdpSocket::bind("0.0.0.0:0").await.expect("bind local socket");
    socket.connect(hbbs_addr).await.expect("connect to local hbbs");

    ensure_online(&socket, &id, &pk).await;
    println!("online with hbbs -- now waiting up to 120s for a real connection attempt against id={id}...");
    println!("(drive one now, e.g. via apps/remote-dev, with password={password})");

    let deadline = tokio::time::Instant::now() + Duration::from_mins(2);
    let mut next_reregister = tokio::time::Instant::now() + REG_INTERVAL;
    let mut buf = [0u8; 2048];
    loop {
        let now = tokio::time::Instant::now();
        if now >= deadline {
            println!("timed out waiting -- nothing else arrived on this socket.");
            return;
        }
        let wait = deadline.min(next_reregister).saturating_duration_since(now);
        match tokio::time::timeout(wait, socket.recv(&mut buf)).await {
            Ok(Ok(n)) => {
                let bytes = &buf[..n];
                println!("\n=== received {n} bytes: {} ===", hex_encode(bytes));
                match va_protocol::read_envelope_field(bytes) {
                    Ok(Some((field, payload))) if field == rendezvous_field::PUNCH_HOLE => {
                        handle_punch_hole(payload, hbbs_addr, relay_addr, &id, &licence_key, identity, password)
                            .await;
                        return;
                    }
                    Ok(Some((field, _))) => println!("(field {field}, not PunchHole -- ignoring)"),
                    Ok(None) => println!("(empty datagram)"),
                    Err(err) => println!("(failed to decode envelope: {err})"),
                }
            }
            Ok(Err(err)) => {
                println!("recv error: {err}");
                return;
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

async fn handle_punch_hole(
    payload: &[u8],
    hbbs_addr: SocketAddr,
    relay_addr: SocketAddr,
    id: &str,
    licence_key: &str,
    identity: IdentityKeyPair,
    password: String,
) {
    let ph: PunchHole = match va_protocol::decode_punch_hole(payload) {
        Ok(ph) => ph,
        Err(err) => {
            println!("failed to decode PunchHole: {err}");
            return;
        }
    };
    println!("PunchHole: nat_type={} socket_addr={}", ph.nat_type, hex_encode(&ph.socket_addr));
    if ph.nat_type != NAT_TYPE_SYMMETRIC {
        println!("nat_type isn't SYMMETRIC -- this demo only implements the forced-relay path, stopping.");
        return;
    }

    println!("responding via relay (RelayResponse to hbbs, then RequestRelay to hbbr)...");
    let mut relay_sock = match va_net::relay::respond_via_relay(hbbs_addr, relay_addr, id, licence_key, ph.socket_addr).await {
        Ok(sock) => sock,
        Err(err) => {
            println!("respond_via_relay failed: {err}");
            return;
        }
    };
    println!("connected to hbbr, paired via RequestRelay -- starting HostSession over this stream");

    run_host_session(&mut relay_sock, id, identity, password).await;
}

async fn run_host_session(
    relay_sock: &mut tokio::net::TcpStream,
    id: &str,
    identity: IdentityKeyPair,
    password: String,
) {
    let (mut session, first_envelope) = HostSession::start(
        HostIdentity { keypair: identity, device_id: id.to_string() },
        HostInfo {
            hostname: "verevon-agent-dev".into(),
            platform: std::env::consts::OS.to_string(),
            displays: vec![DisplayInfo {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
                name: "Display1".into(),
                online: true,
                cursor_embedded: false,
                original_resolution: None,
                scale: 1.0,
            }],
            version: env!("CARGO_PKG_VERSION").to_string(),
        },
        Credentials { permanent_password: None, temporary_password: Some(password) },
    );

    if let Err(err) = send_frame(relay_sock, &first_envelope).await {
        println!("failed to send SignedId: {err}");
        return;
    }
    println!("sent SignedId (plaintext) -- waiting for PublicKey...");

    let mut inbuf = BytesMut::with_capacity(4096);
    let mut got_public_key = false;
    let mut read_chunk = [0u8; 4096];

    loop {
        loop {
            match va_wire::decode_frame(&inbuf) {
                Ok(Some((consumed, payload))) => {
                    let _ = inbuf.split_to(consumed);
                    if !got_public_key {
                        got_public_key = true;
                        match session.on_plaintext_message(&payload) {
                            Ok(hash_envelope) => {
                                println!("got PublicKey -- session key established, sending Hash challenge");
                                if let Err(err) = send_frame(relay_sock, &hash_envelope).await {
                                    println!("failed to send Hash: {err}");
                                    return;
                                }
                            }
                            Err(err) => {
                                println!("on_plaintext_message failed: {err}");
                                return;
                            }
                        }
                        continue;
                    }
                    match session.on_encrypted_message(&payload) {
                        Ok(Event::LoggedIn { response }) => {
                            if let Err(err) = send_frame(relay_sock, &response).await {
                                println!("failed to send LoginResponse: {err}");
                            }
                            println!("\n*** LOGGED IN -- full handshake + login succeeded end to end ***");
                            return;
                        }
                        Ok(Event::LoginFailed { reason, response }) => {
                            if let Err(err) = send_frame(relay_sock, &response).await {
                                println!("failed to send LoginResponse: {err}");
                                return;
                            }
                            println!("login failed: {reason} (connection stays open for a retry)");
                        }
                        Ok(Event::SendEncrypted(bytes)) => {
                            if let Err(err) = send_frame(relay_sock, &bytes).await {
                                println!("failed to send: {err}");
                                return;
                            }
                        }
                        // This diagnostic tool only proves the
                        // registration/relay/handshake/login sequence;
                        // input injection is `verevon-agent`'s job, not
                        // this example's.
                        Ok(Event::MouseInput(_) | Event::KeyInput(_) | Event::None) => {}
                        Err(err) => {
                            println!("on_encrypted_message failed: {err}");
                            return;
                        }
                    }
                }
                Ok(None) => break,
                Err(err) => {
                    println!("frame decode failed: {err}");
                    return;
                }
            }
        }

        match relay_sock.read(&mut read_chunk).await {
            Ok(0) => {
                println!("relay connection closed by peer");
                return;
            }
            Ok(n) => inbuf.extend_from_slice(&read_chunk[..n]),
            Err(err) => {
                println!("relay read error: {err}");
                return;
            }
        }
    }
}

async fn send_frame(sock: &mut tokio::net::TcpStream, envelope: &[u8]) -> std::io::Result<()> {
    let frame = va_wire::encode_frame(envelope).expect("encode_frame only fails past MAX_FRAME_LEN");
    sock.write_all(&frame).await
}

/// The real per-cycle keepalive: `RegisterPeer` first (this is what
/// actually refreshes hbbs's `last_reg_time`), and only send `RegisterPk`
/// when `request_pk` asks for it -- mirrors
/// `RegistrationClient::ensure_online_once`, reimplemented here on a raw
/// socket since this demo needs long-lived, uninterrupted `recv` access to
/// the same socket for the `PunchHole` capture below it.
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
