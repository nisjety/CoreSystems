//! Verevon Agent — the native host side of the RustDesk-compatible
//! remote-desktop protocol. Registers with hbbs, waits for a real
//! connection attempt, pairs via the relay, and runs a real login +
//! continuous video session.
//!
//! **Deliberately still a single-session-at-a-time build.** Video is the
//! real primary monitor via [`va_capture::DxgiDuplicationSource`], resized
//! by `va-encode` down to this binary's fixed, measured-real-time encode
//! resolution (see `WIDTH`/`HEIGHT` below) -- never the display's own
//! native size. Mouse/keyboard input is real too (`va-input`, gated by
//! nothing but the encrypted session itself -- see `va_protocol::
//! handshake`'s docs on the still-missing permission model). File transfer
//! and multi-session support remain explicitly out of scope for this pass.
//!
//! **The device identity IS persisted to a local file across restarts** —
//! not a nicety, a correctness requirement discovered live: hbbs rejects a
//! CHANGED `uuid` reusing an already-registered `id` (`UuidMismatch`; a
//! real client's `uuid` is machine-derived and stable for exactly this
//! reason). An earlier version of this binary generated a fresh identity
//! every process start, which worked for a single connection but broke
//! every reconnection attempt after the first: hbbs kept vouching for the
//! PREVIOUS run's `pk`, so the new run's `SignedId` failed signature
//! verification on the controller's end (`va-net`'s crate docs have the
//! full live-reproduction). A real Control-Plane-issued device identity
//! contract is still deferred — this is a local file, not that — but a
//! stable identity across restarts is not optional even for this pass.
#![allow(clippy::cast_possible_wrap)]

use std::env;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use bytes::BytesMut;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::UdpSocket;
use tokio::sync::Mutex as AsyncMutex;

use va_capture::{CaptureError, DxgiDuplicationSource, FrameSource};
use va_crypto::IdentityKeyPair;
use va_encode::{Av1Encoder, RawFrame, VideoEncoder};
use va_protocol::{
    decode_punch_hole, decode_register_peer_response, decode_register_pk_response,
    encode_av1_video_frame, encode_register_peer, encode_register_pk, read_envelope_field,
    rendezvous_field, wrap_message, Credentials, DisplayInfo, Event, HostIdentity, HostInfo,
    HostSession, RegisterPeer, RegisterPk, NAT_TYPE_SYMMETRIC,
};

// 480x270, not 1280x720: this is a real, tested resolution choice, not
// last milestone's stopgap. Measured live with `crates/va-encode/examples/
// bench.rs`: a DEBUG build of pure-Rust rav1e (no "asm"/SIMD -- see
// va-encode's Cargo.toml for why that's deliberate) took 6-38 SECONDS per
// 720p frame -- but a `--release` build alone (this binary was never once
// run with `--release` before that measurement) brought 720p down to
// 0.6-2.4s/frame and 480x270 down to 0.04-0.19s/frame, comfortably inside
// the 12fps target. **This binary MUST be run with `cargo run --release`
// (or an equivalent release build) to be usable at all** -- a debug build
// is not a slower version of this program, it is a fundamentally
// different, ~10x-slower one for CPU-bound encoding. 720p is still not
// real-time even in release mode; 480x270 is the resolution this pass
// actually delivers on.
const WIDTH: u32 = 480;
const HEIGHT: u32 = 270;
const TARGET_FPS: u64 = 12;
const TARGET_BITRATE_KBPS: u32 = 1500;
const REG_INTERVAL: Duration = Duration::from_secs(10);
const REGISTER_TIMEOUT: Duration = Duration::from_secs(5);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let device_id = env::var("VEREVON_AGENT_ID").unwrap_or_else(|_| "verevon-agent-dev".to_string());
    let password = env::var("VEREVON_AGENT_PASSWORD").unwrap_or_else(|_| "changeme".to_string());
    let hbbs_addr: SocketAddr = env::var("VEREVON_RENDEZVOUS_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:21116".to_string())
        .parse()
        .context("VEREVON_RENDEZVOUS_ADDR must be a valid host:port")?;
    let relay_addr: SocketAddr = env::var("VEREVON_RELAY_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:21117".to_string())
        .parse()
        .context("VEREVON_RELAY_ADDR must be a valid host:port")?;
    let licence_key = env::var("VEREVON_LICENCE_KEY")
        .context("VEREVON_LICENCE_KEY must be set (hbbs's own logged `Key:` value)")?;
    let identity_path = env::var("VEREVON_IDENTITY_PATH")
        .map_or_else(|_| PathBuf::from("verevon-agent-identity.key"), PathBuf::from);

    let identity = load_or_create_identity(&identity_path)?;
    tracing::info!(
        device_id = %device_id, %hbbs_addr, %relay_addr, identity_path = %identity_path.display(),
        "starting Verevon Agent"
    );

    loop {
        let outcome = run_one_cycle(identity, &device_id, &password, hbbs_addr, relay_addr, &licence_key).await;
        if let Err(err) = outcome {
            tracing::error!(error = %err, "session cycle ended with an error -- re-registering from scratch");
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

/// Loads the persistent identity from `path` if it exists (exactly 64
/// bytes, a raw Ed25519 secret key), or generates and saves a fresh one.
fn load_or_create_identity(path: &PathBuf) -> anyhow::Result<IdentityKeyPair> {
    if let Ok(bytes) = std::fs::read(path) {
        let secret: [u8; 64] = bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("{} does not contain a valid 64-byte identity", path.display()))?;
        tracing::info!(path = %path.display(), "loaded existing identity");
        return Ok(IdentityKeyPair::from_secret_key(secret));
    }
    let identity = IdentityKeyPair::generate();
    std::fs::write(path, identity.secret_key_bytes())
        .with_context(|| format!("failed to save a new identity to {}", path.display()))?;
    tracing::info!(path = %path.display(), "generated and saved a new identity");
    Ok(identity)
}

/// One full cycle: register, wait for a connection attempt, pair via the
/// relay, run the session until it ends, then return so the caller
/// restarts from a clean registration (reusing the SAME `identity` --
/// see the module docs for why that isn't optional).
async fn run_one_cycle(
    identity: IdentityKeyPair,
    device_id: &str,
    password: &str,
    hbbs_addr: SocketAddr,
    relay_addr: SocketAddr,
    licence_key: &str,
) -> anyhow::Result<()> {
    let pk = identity.public_key().to_vec();

    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    socket.connect(hbbs_addr).await?;

    ensure_online(&socket, device_id, &pk).await?;
    tracing::info!("online with hbbs -- waiting for a connection attempt");

    let mut next_reregister = tokio::time::Instant::now() + REG_INTERVAL;
    let mut buf = [0u8; 2048];
    let punch_hole_socket_addr = loop {
        let wait = next_reregister.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(wait, socket.recv(&mut buf)).await {
            Ok(Ok(n)) => {
                if let Ok(Some((field, payload))) = read_envelope_field(&buf[..n]) {
                    if field == rendezvous_field::PUNCH_HOLE {
                        match decode_punch_hole(payload) {
                            Ok(ph) if ph.nat_type == NAT_TYPE_SYMMETRIC => break ph.socket_addr,
                            Ok(ph) => tracing::warn!(
                                nat_type = ph.nat_type,
                                "PunchHole with a non-SYMMETRIC nat_type -- direct hole-punch \
                                 isn't implemented (Support Plane's ALWAYS_USE_RELAY=Y should \
                                 make this unreachable), ignoring"
                            ),
                            Err(err) => tracing::warn!(error = %err, "failed to decode a PunchHole"),
                        }
                    }
                }
            }
            Ok(Err(err)) => return Err(err.into()),
            Err(_elapsed) => {
                ensure_online(&socket, device_id, &pk).await?;
                next_reregister = tokio::time::Instant::now() + REG_INTERVAL;
            }
        }
    };

    tracing::info!("PunchHole received -- pairing via the relay");
    let relay_sock =
        va_net::relay::respond_via_relay(hbbs_addr, relay_addr, device_id, licence_key, punch_hole_socket_addr)
            .await?;
    tracing::info!("paired via the relay -- running the host session");

    run_host_session(relay_sock, device_id, identity, password).await
}

async fn run_host_session(
    stream: tokio::net::TcpStream,
    device_id: &str,
    identity: IdentityKeyPair,
    password: &str,
) -> anyhow::Result<()> {
    let (mut read_half, write_half) = stream.into_split();
    let write_half = Arc::new(AsyncMutex::new(write_half));

    let (session, first_envelope) = HostSession::start(
        HostIdentity { keypair: identity, device_id: device_id.to_string() },
        HostInfo {
            hostname: "verevon-agent".to_string(),
            platform: env::consts::OS.to_string(),
            displays: vec![DisplayInfo {
                x: 0,
                y: 0,
                width: WIDTH as i32,
                height: HEIGHT as i32,
                name: "Display1".to_string(),
                online: true,
                cursor_embedded: false,
                original_resolution: None,
                scale: 1.0,
            }],
            version: env!("CARGO_PKG_VERSION").to_string(),
        },
        Credentials { permanent_password: None, temporary_password: Some(password.to_string()) },
    );
    let session = Arc::new(AsyncMutex::new(session));
    send_frame(&write_half, &first_envelope).await?;

    let mut injector = init_injector();

    let mut inbuf = BytesMut::with_capacity(4096);
    let mut read_chunk = [0u8; 4096];
    let mut got_public_key = false;
    let mut video_task: Option<(tokio::task::JoinHandle<()>, Arc<AtomicBool>)> = None;

    let result: anyhow::Result<()> = 'session: loop {
        loop {
            match va_wire::decode_frame(&inbuf) {
                Ok(Some((consumed, payload))) => {
                    let _ = inbuf.split_to(consumed);
                    let mut session_guard = session.lock().await;
                    if !got_public_key {
                        got_public_key = true;
                        match session_guard.on_plaintext_message(&payload) {
                            Ok(hash_envelope) => {
                                drop(session_guard);
                                if let Err(err) = send_frame(&write_half, &hash_envelope).await {
                                    break 'session Err(err.into());
                                }
                            }
                            Err(err) => break 'session Err(err.into()),
                        }
                        continue;
                    }
                    match session_guard.on_encrypted_message(&payload) {
                        Ok(Event::LoggedIn { response }) => {
                            drop(session_guard);
                            if let Err(err) = send_frame(&write_half, &response).await {
                                break 'session Err(err.into());
                            }
                            tracing::info!("logged in -- starting the video pump");
                            let stop = Arc::new(AtomicBool::new(false));
                            let task = tokio::spawn(pump_video(session.clone(), write_half.clone(), stop.clone()));
                            video_task = Some((task, stop));
                        }
                        Ok(Event::LoginFailed { reason, response }) => {
                            drop(session_guard);
                            if let Err(err) = send_frame(&write_half, &response).await {
                                break 'session Err(err.into());
                            }
                            tracing::warn!(reason = %reason, "login failed -- connection stays open for a retry");
                        }
                        Ok(Event::SendEncrypted(bytes)) => {
                            drop(session_guard);
                            if let Err(err) = send_frame(&write_half, &bytes).await {
                                break 'session Err(err.into());
                            }
                        }
                        Ok(event @ (Event::MouseInput(_) | Event::KeyInput(_))) => {
                            drop(session_guard);
                            inject_input_event(&mut injector, &event);
                        }
                        Ok(Event::None) => {}
                        Err(err) => break 'session Err(err.into()),
                    }
                }
                Ok(None) => break,
                Err(err) => break 'session Err(err.into()),
            }
        }

        match read_half.read(&mut read_chunk).await {
            Ok(0) => {
                tracing::info!("relay connection closed by the peer");
                break 'session Ok(());
            }
            Ok(n) => inbuf.extend_from_slice(&read_chunk[..n]),
            Err(err) => break 'session Err(err.into()),
        }
    };

    if let Some((mut task, stop)) = video_task {
        stop_video_pump(&mut task, &stop).await;
    }
    result
}

/// Signals `pump_video` to stop and waits briefly for it to exit on its
/// own, falling back to `abort()` only if it doesn't. This matters now in a
/// way it didn't when video came from `TestPatternSource`: a real
/// `DxgiDuplicationSource`'s `Drop` calls `windows-capture`'s
/// `CaptureControl::stop()`, which POSTS `WM_QUIT` and **joins** the
/// capture thread -- a real blocking call (see `va-capture::dxgi`'s doc
/// comment). An `abort()`'d task's local variables (including `source`)
/// still drop synchronously wherever the task's stack unwinds, which could
/// be a tokio worker thread -- stalling it for as long as that join takes.
/// Letting `pump_video` notice the stop flag and return normally means that
/// `Drop` runs on ITS OWN task, still not ideal but no worse than any other
/// blocking call already tolerated elsewhere in this codebase, and avoiding
/// the additional hazard of dropping mid-abort's forced unwind. The 2s
/// fallback exists only so a stuck capture session can't hang session
/// teardown forever.
async fn stop_video_pump(task: &mut tokio::task::JoinHandle<()>, stop: &Arc<AtomicBool>) {
    stop.store(true, Ordering::Relaxed);
    tokio::select! {
        _ = &mut *task => {}
        () = tokio::time::sleep(Duration::from_secs(2)) => {
            tracing::warn!("video pump did not stop cooperatively in time -- aborting");
            task.abort();
        }
    }
}

/// `None` (rather than failing the whole session) if `enigo` can't
/// initialize -- e.g. no interactive desktop session is available. A real
/// permission/consent gate belongs somewhere in this path too (see
/// `va_protocol::handshake`'s own docs on why this pass doesn't build one);
/// this injects whatever the controller sends, unconditionally.
fn init_injector() -> Option<va_input::EnigoInjector> {
    match va_input::EnigoInjector::new(WIDTH, HEIGHT) {
        Ok(injector) => Some(injector),
        Err(err) => {
            tracing::warn!(error = %err, "failed to initialize input injection -- input will be ignored");
            None
        }
    }
}

/// Injects a decoded `MouseInput`/`KeyInput` event via `injector`, if input
/// injection is available (a no-op, with a warning already logged once at
/// startup, if `EnigoInjector::new` failed). Any other `Event` variant is
/// silently ignored -- callers only ever pass one of the two input variants.
fn inject_input_event(injector: &mut Option<va_input::EnigoInjector>, event: &Event) {
    let Some(injector) = injector.as_mut() else { return };
    let result = match event {
        Event::MouseInput(mouse) => injector.handle_mouse_event(mouse),
        Event::KeyInput(key) => injector.handle_key_event(key),
        _ => return,
    };
    if let Err(err) = result {
        tracing::warn!(error = %err, "failed to inject input event");
    }
}

/// Runs until `stop` is set (checked once per tick -- see
/// `stop_video_pump`'s docs for why this is cooperative rather than an
/// `abort()`) or the shared write half errors (the connection is gone).
async fn pump_video(
    session: Arc<AsyncMutex<HostSession>>,
    write_half: Arc<AsyncMutex<OwnedWriteHalf>>,
    stop: Arc<AtomicBool>,
) {
    // Real GPU/COM setup that briefly blocks the calling thread (see
    // `va-capture::dxgi`'s doc comment) -- must not run directly on this
    // async task.
    let mut source = match tokio::task::spawn_blocking(DxgiDuplicationSource::new_for_primary_monitor).await {
        Ok(Ok(source)) => source,
        Ok(Err(err)) => {
            tracing::error!(error = %err, "failed to start screen capture -- no video will be sent");
            return;
        }
        Err(err) => {
            tracing::error!(error = %err, "screen capture setup task panicked");
            return;
        }
    };
    let mut encoder = match Av1Encoder::new(WIDTH, HEIGHT, TARGET_BITRATE_KBPS) {
        Ok(encoder) => encoder,
        Err(err) => {
            tracing::error!(error = %err, "failed to construct the AV1 encoder -- no video will be sent");
            return;
        }
    };

    let mut interval = tokio::time::interval(Duration::from_millis(1000 / TARGET_FPS));
    let mut frame_index: u64 = 0;
    loop {
        interval.tick().await;
        if stop.load(Ordering::Relaxed) {
            tracing::info!("video pump stopping cooperatively");
            return;
        }

        let captured = match source.capture_frame() {
            Ok(frame) => frame,
            // The capture session is alive but hasn't produced a frame
            // this tick (normal on a very first call, or a startup race) --
            // try again next tick rather than tearing down the whole pump.
            Err(CaptureError::NotReady) => {
                tracing::debug!(frame_index, "no captured frame ready yet, skipping this tick");
                continue;
            }
            Err(err) => {
                tracing::error!(error = %err, "frame capture failed, stopping video pump");
                return;
            }
        };
        let raw = RawFrame { width: captured.width, height: captured.height, stride: captured.stride, bgra: captured.bgra };
        let force_keyframe = frame_index == 0;
        frame_index += 1;

        let chunks = match encoder.encode(&raw, force_keyframe) {
            Ok(chunks) => chunks,
            Err(err) => {
                tracing::error!(error = %err, "AV1 encode failed, stopping video pump");
                return;
            }
        };
        if chunks.is_empty() {
            tracing::debug!(frame_index, "encode produced no packet yet (still warming up)");
            continue;
        }
        tracing::debug!(
            frame_index,
            chunk_count = chunks.len(),
            first_chunk_bytes = chunks[0].data.len(),
            first_chunk_key = chunks[0].key,
            "sending video chunk(s)"
        );

        let frames: Vec<va_protocol::EncodedVideoFrame> = chunks
            .into_iter()
            .map(|c| va_protocol::EncodedVideoFrame { data: c.data, key: c.key, pts: c.pts })
            .collect();
        let envelope = wrap_message(va_protocol::message_field::VIDEO_FRAME, &encode_av1_video_frame(&frames, 0));

        let encrypted = {
            let mut session_guard = session.lock().await;
            session_guard.encrypt_for_send(&envelope)
        };
        let encrypted = match encrypted {
            Ok(bytes) => bytes,
            Err(err) => {
                tracing::error!(error = %err, "failed to encrypt a video frame, stopping video pump");
                return;
            }
        };
        if send_frame(&write_half, &encrypted).await.is_err() {
            tracing::info!("write side closed, stopping video pump");
            return;
        }
    }
}

async fn send_frame(write_half: &Arc<AsyncMutex<OwnedWriteHalf>>, envelope: &[u8]) -> std::io::Result<()> {
    let frame = va_wire::encode_frame(envelope).expect("encode_frame only fails past MAX_FRAME_LEN");
    write_half.lock().await.write_all(&frame).await
}

/// The real per-cycle keepalive: `RegisterPeer` first (this is what
/// actually refreshes hbbs's `last_reg_time` -- see `va-net`'s crate docs),
/// and only send `RegisterPk` when `request_pk` asks for it. Reimplemented
/// here on a raw socket (mirroring `va-net::RegistrationClient`'s own
/// logic) because this loop also needs the same socket free to listen for
/// a forwarded `PunchHole` between keepalives -- see the module docs on
/// why `RegistrationClient`'s own encapsulated socket isn't used directly.
async fn ensure_online(socket: &UdpSocket, id: &str, pk: &[u8]) -> anyhow::Result<()> {
    if register_peer(socket, id).await? {
        register_pk(socket, id, pk).await?;
        let still_wants_pk = register_peer(socket, id).await?;
        if still_wants_pk {
            anyhow::bail!("hbbs asked for a fresh RegisterPk again immediately after we sent one");
        }
    }
    Ok(())
}

async fn register_peer(socket: &UdpSocket, id: &str) -> anyhow::Result<bool> {
    let payload = encode_register_peer(&RegisterPeer { id: id.to_string(), serial: 0 });
    socket.send(&wrap_message(rendezvous_field::REGISTER_PEER, &payload)).await?;

    let mut buf = [0u8; 512];
    let n = tokio::time::timeout(REGISTER_TIMEOUT, socket.recv(&mut buf))
        .await
        .context("hbbs did not reply to RegisterPeer in time")??;
    let (field, payload) = read_envelope_field(&buf[..n])?.context("empty RegisterPeer reply")?;
    anyhow::ensure!(field == rendezvous_field::REGISTER_PEER_RESPONSE, "unexpected reply field {field}");
    Ok(decode_register_peer_response(payload)?.request_pk)
}

async fn register_pk(socket: &UdpSocket, id: &str, pk: &[u8]) -> anyhow::Result<()> {
    let payload = encode_register_pk(&RegisterPk { id: id.to_string(), uuid: pk.to_vec(), pk: pk.to_vec() });
    socket.send(&wrap_message(rendezvous_field::REGISTER_PK, &payload)).await?;

    let mut buf = [0u8; 512];
    let n = tokio::time::timeout(REGISTER_TIMEOUT, socket.recv(&mut buf))
        .await
        .context("hbbs did not reply to RegisterPk in time")??;
    let (field, payload) = read_envelope_field(&buf[..n])?.context("empty RegisterPk reply")?;
    anyhow::ensure!(field == rendezvous_field::REGISTER_PK_RESPONSE, "unexpected reply field {field}");
    match decode_register_pk_response(payload)?.result {
        va_protocol::RegisterPkResult::Ok => Ok(()),
        other => anyhow::bail!("hbbs rejected RegisterPk: {other:?}"),
    }
}
