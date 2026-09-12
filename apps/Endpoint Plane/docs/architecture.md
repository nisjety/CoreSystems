# Verevon Agent — Architecture & Status

## What this is

The native, installable "Verevon Agent" — the HOST side of the RustDesk-compatible
remote-desktop protocol (the side that gets controlled), the counterpart to
`@verevon/remote-core` (`apps/Frontend Plane/verevonv3/packages/remote-core`),
which already implements the CLIENT/controller side and has been live-verified
against a real stock RustDesk host. Nothing native existed anywhere in this
monorepo before this plane — every prior test used a stock, off-the-shelf
RustDesk desktop client as a stand-in.

Built as an **original, clean-room reimplementation** — no RustDesk source is
copied or adapted anywhere in this codebase. RustDesk is AGPL-3.0; adapting its
code would obligate releasing this agent's own source under the same license.
Message field numbers and wire shapes are reproduced as protocol FACTS
(verified against a real `rustdesk-server` and reverse-engineered from
`remote-core`'s own already-reviewed research), the same posture
`remote-core`'s own `docs/licensing.md` already took for the browser side.

## Status: what's real, what's pending

**The headline result: a full, real, end-to-end RustDesk-protocol session
now works between two independently-built implementations — `remote-core`
(browser, TypeScript) and Verevon Agent (native, Rust) — with hbbs/hbbr in
between doing nothing special. Rendezvous, relay pairing, the X25519/Ed25519
handshake, and password login all completed for real, live, over a real
Support Plane. This was NOT a given: it took a research workflow, three
separate live-testing sessions, and two genuine bugs found only by actually
connecting the two sides to each other (see below) — this is exactly why
that live testing was worth doing rather than trusting the (self-consistent
but not cross-verified) unit test suite alone.**

**Implemented and tested (M1 — zero I/O, no sockets, no OS dependency):**

- `va-crypto` — Ed25519 identity signing (attached signatures), X25519
  authenticated key exchange (`crypto_box_easy` with an all-zero nonce —
  see the crypto bug below for why this is NOT `crypto_box_seal`),
  XSalsa20-Poly1305 secretbox with the wire protocol's exact per-direction
  counter-based nonce scheme, and the double-SHA-256 password hashing
  scheme. The password hash is cross-checked **byte-for-byte** against an
  independent Node computation of the same algorithm `remote-core` uses.
  10 tests.
- `va-wire` — hand-written proto3 varint/zigzag/length-delimited codec
  (verified against Google's own published varint example) plus
  `BytesCodec`-style variable length-prefix TCP framing (1-4 byte header,
  low 2 bits select width) — framing `remote-core` never needed (browser
  WebSocket frames already delimit one message each), new territory here.
  15 tests, and — as of M2c below — live-proven over a real TCP relay
  stream, not just in-memory.
- `va-protocol` — the message catalog (`SignedId`, `PublicKey`, `Hash`,
  `LoginRequest`/`Response`, `PeerInfo`/`DisplayInfo`, `VideoFrame`
  (VP9, batched `EncodedVideoFrame`s), `TestDelay`, `Misc`/`PermissionInfo`,
  `SwitchDisplay`, `RegisterPeer`/`RegisterPk` and their responses,
  `PunchHole`, `RelayResponse`, `RequestRelay`) and `HostSession`, the
  host-side per-connection state machine (handshake -> login -> session
  pump). 25 unit tests plus **4 end-to-end tests** that drive `HostSession`
  through a complete real handshake and login against `va-testkit`'s
  independent `FakeController` implementation — covering a full successful
  login, a wrong-password rejection followed by a successful retry on the
  same connection, signature rejection against the wrong identity key, and
  the `TestDelay{from_client: true}` echo contract once connected.
- `va-testkit` — `FakeController`, the inverse of `remote-core`'s own
  `tests/doubles/FakeRustDeskPeer.ts`: an independent implementation of the
  controller side (own ephemeral keypair, own sealing, own login-request
  construction) used only to test `HostSession`, never shipped.
- **A genuine crypto bug, only caught by live-testing against the REAL
  `remote-core`, not by any of the above unit tests**: `va-crypto`'s
  `seal`/`unseal` originally used libsodium's anonymous `crypto_box_seal`.
  `va-testkit`'s `FakeController` used the exact same (wrong) primitive on
  the sending side, so the M1 end-to-end tests were perfectly
  self-consistent and passed — right up until the FIRST real connection
  attempt against `remote-core`, which uses a plain `crypto_box_easy` with
  an all-zero nonce and BOTH ephemeral public keys (confirmed by reading
  `sealSessionKey`/`openSealedSessionKey` in `remote-core`'s own
  `crypto/SessionCrypto.ts`) — failing with `sealed-box open failed (wrong
  key or corrupted ciphertext)` on the very first `PublicKey` received from
  a real browser. Fixed in `va-crypto` (`seal`/`unseal` now take both
  parties' ephemeral keys explicitly) and in `HostSession::on_plaintext_message`
  (now reads `PublicKey.asymmetric_value`, the controller's ephemeral
  public key, which the old code ignored entirely). **The lesson, not just
  the fix**: two implementations sharing the same wrong assumption can
  never catch each other's bug in a mutual round-trip test — only testing
  against a genuinely independent third implementation can.

**A deliberate codec change: AV1 (`rav1e`) instead of VP9 (`vpx-encode`/libvpx).**
`vpx-encode` needs `pkg-config` and a system libvpx to link — neither
installed on this machine, and getting them means installing `vcpkg` and
building libvpx from source (a heavy, ~20-30 minute native-toolchain
install). `rav1e` is a mature, actively-maintained pure-Rust AV1 encoder —
no native toolchain at all once its own `asm`/`binaries` default features
are disabled (see `va-encode`'s `Cargo.toml`). This was a real choice, not
a default: `hbb_common`'s `message.proto` already has an `av1s` oneof field
(13) using the exact same `EncodedVideoFrames` wrapper as `vp9s`, and
`remote-core` (the browser side) turned out to **already fully support
AV1** — wire decode (`videoCodec.ts`'s `CODEC_FIELD` map), the WebCodecs
codec string (`av01.0.04M.08`), and capability negotiation
(`SupportedDecoding.abilityAv1`) were all already there from earlier
sessions, needing zero changes. `encode_vp9_video_frame` is kept
alongside `encode_av1_video_frame` in `va-protocol` (tested, unused by
`main.rs` today) in case VP9 is ever revisited.

**Implemented and live-verified end to end (M3 — continuous video, a real
long-running process):**

- `va-encode`'s `Av1Encoder` wraps `rav1e` in low-latency mode (no frame
  reordering, fastest speed preset). A real, live-measured tuning bug: the
  default `reservoir_frame_delay` (rate-control smoothing) is
  `min(max_key_frame_interval * 1.5, 240)` frames — with an
  unexamined `max_key_frame_interval`, that computed to **240 frames (8s
  at 30fps) of startup latency before the first packet**, measured
  directly in this crate's own tests. Pinned to `12` (the value `rav1e`'s
  own source names as its recommended floor) and `max_key_frame_interval`
  dropped to 60 (a keyframe every ~2s — faster recovery matters far more
  here than file-size amortization). Owns the BGRA8->YUV 4:2:0 conversion
  (BT.601, simple 2:1 box-average chroma subsampling) `rav1e` needs, since
  `va-capture` has no reason to know anything about encoding.
- `va-capture`'s `FrameSource` trait ships with `TestPatternSource` — a
  synthetic, deterministic moving checkerboard, no OS dependency at all —
  matching the build plan's own staged order ("prove the wire+decode path
  before swapping in real DXGI frames"). The real `DxgiDuplicationSource`
  (via `windows-capture`) is not built yet.
- **A second identity bug, found by the SAME kind of live testing as the
  registration ones above, not hypothesized in advance**: an early
  `verevon-agent` main.rs generated a fresh `IdentityKeyPair` every
  process start. The FIRST live connection worked; the very next one (same
  device id, after a restart) failed with `Signature verification failed —
  wrong signer key or tampered payload` on the controller's end. Root
  cause: hbbs's `request_pk` signal reflects hbbs's OWN state (does it
  already have a `pk` on file for this `id`?), not whether THIS run's
  identity has ever actually been registered — so a fresh keypair reusing
  an `id` hbbs already has a (different, PREVIOUS run's) `pk` for never
  got its own `RegisterPk` sent at all, leaving hbbs vouching for a key the
  new run doesn't hold the secret half of. Chasing this ALSO surfaced a
  more fundamental constraint: hbbs additionally rejects a *changed*
  `uuid` reusing an existing `id` (`UuidMismatch`) — a real client's `uuid`
  is machine-derived and stable for exactly this reason, so a genuinely
  fresh identity can never legitimately reconnect under the same id at
  all. Fixed two ways: `RegistrationClient::ensure_online_once` now always
  sends `RegisterPk` on its own first call regardless of `request_pk` (a
  new client instance reusing a **persisted** identity needs this even
  when hbbs's own signal says no); and `verevon-agent`'s identity is now
  actually persisted to a local file (`IdentityKeyPair::from_secret_key`/
  `secret_key_bytes`, new in `va-crypto`) and reloaded across restarts,
  which is what makes the `uuid` stable in the first place. Both
  live-verified: `va-crypto`'s secret-key round-trip test, and a new
  `va-net` live test reconnecting with the same persisted identity from a
  fresh client instance.
- **The full pipeline, live-verified, visually, not just by log lines**:
  `verevon-agent`'s real `main.rs` (not a demo) registers, waits for a
  `PunchHole`, pairs via the relay, logs in, and then continuously pumps
  `TestPatternSource` frames through `Av1Encoder` and the wire to a real
  `apps/remote-dev` browser session. Confirmed with actual screenshots of
  the browser's canvas showing the live checkerboard-and-moving-band
  pattern, the band visibly in a different position between two
  screenshots taken 8 seconds apart — proof of a genuinely live, decoding,
  rendering stream, not a frozen first frame. `Bilder per sekund` (fps)
  and `Forsinkelse` (latency) in the UI both showed real nonzero values
  for the first time in this initiative's history.
- **A real, measured performance finding — since resolved by a single root
  cause, not a codec limitation.** Pure-Rust `rav1e` (no SIMD `asm`, per the
  codec-choice rationale above) encoding a 1280x720 frame took **6-38
  seconds per frame** on this machine — but that measurement was taken from
  a **DEBUG build**; this binary had never once been run with `--release`.
  `crates/va-encode/examples/bench.rs` (a new diagnostic tool, kept
  permanently — rerun it after any future encoder tuning change) measured
  the SAME 1280x720 workload in `--release`: **0.6-2.4s/frame**, and
  480x270 in `--release`: **0.04-0.19s/frame**, comfortably inside the
  12fps target. LTO + `codegen-units = 1` were tried and measured **no
  further improvement** over plain `--release` — not added, since they cost
  real build time for nothing. **The practical takeaway: `cargo run -p
  verevon-agent` with no `--release` is not a valid way to evaluate or use
  this binary at all.**
  - **A second, independent finding on top of that**: `rav1e`'s own
    tiling was never actually engaged. `EncoderConfig`'s `tiles`/
    `tile_cols`/`tile_rows` all defaulted to 0, and rav1e computes real
    tile count from RESOLUTION against an AV1-spec minimum, not from
    available CPU parallelism — at both 480x270 and 1280x720 that
    minimum was 1 tile. Since the actual per-block RDO/motion-search cost
    (the dominant per-frame cost) only parallelizes ACROSS tiles, this
    crate was running that cost on exactly one thread the whole time,
    despite `threading` being enabled and rav1e defaulting to a 20-thread
    Rayon pool on this machine (`Config::new()` never calls
    `with_threads`, so it uses Rayon's own global pool sized off
    `available_parallelism()` automatically — no manual thread count is
    needed). Explicitly setting `tiles: 4` (a safe 2x2 split, well clear
    of AV1's tile-size floor at these resolutions) measured a real ~1.5-2x
    additional improvement at 1280x720 (steady state roughly
    0.3-1.05s/frame, down from 0.6-2.4s/frame) and made 854x480 a
    genuinely close-to-viable middle ground (mostly 0.08-0.3s/frame).
    480x270 barely changed with tiling (too small a frame to split
    usefully into 4 tiles at 64x64 superblocks) — it was already the
    resolution clearing the 12fps target reliably, and still is.
  - `main.rs` stays at 480x270: it's the only resolution measured to
    reliably clear the 12fps target, even though 854x480 is now much
    closer than before tiling. Revisiting the resolution once real (not
    synthetic-noise) screen content is flowing through the encoder is a
    reasonable next step — real desktop content is typically far more
    compressible than this crate's adversarial per-pixel-increment
    benchmark pattern.
- `remote-dev`'s `App.tsx` now always passes a console-forwarding logger to
  `createRemoteClient` — discovered as a genuine gap while debugging the
  above: `remote-core` defaults to a silent `noopLogger`, so a real
  decode/protocol issue produced zero console output at all until this was
  added. Kept permanently in this dev-only harness, not reverted.

**Implemented and live-verified against the real local Support Plane (M2a —
hbbs registration):**

- `va-net`'s `RegistrationClient` sends `RegisterPk` over UDP to hbbs:21116
  and decodes `RegisterPkResponse`. The M0 wire-shape gap (field numbers,
  required fields, re-registration cadence) closed via a citation-backed
  read of `hbb_common`'s public `protos/rendezvous.proto` (commit
  `69cea8dafee147848ae88702029f4bf7df7224c3`, the exact submodule pin of
  `rustdesk-server` commit `a7736be5e40f85bfc141120dce587e836e5d4b80`) and
  `rustdesk-server`'s `rendezvous_server.rs` at that same commit — see
  `va-protocol`'s `messages.rs` for the field-by-field citations.
  `RegisterPk = 15`, `RegisterPkResponse = 16` (the latter cross-checked
  against the live wire capture already on record in `remote-core`'s
  `docs/rustdesk-protocol.md`). `REGISTRATION_INTERVAL` (15s) matches the
  real client's own re-registration cadence, comfortably under hbbs's 30s
  offline threshold.
  - **Live-verified, not just unit-tested**: `crates/va-net/tests/
    register_live.rs` (ignored by default; run with `--ignored` against a
    real running `docker compose up -d hbbs` in `apps/Support Plane`) proves
    a fresh `RegisterPk` is accepted, a second immediate one stays accepted
    (within hbbs's 2-attempts-per-6s allowance), and re-registering after
    that window resets is *still* accepted — the "sustained registration"
    proof `remote-core`'s one-off ad hoc poke never gave us. A second test
    proves a different identity claiming an already-registered id is
    correctly rejected. hbbs's own server-side logs confirmed both: a real
    `update_pk <id> <addr> <uuid> <pk>` line for each successful
    registration, and a real `Peer <id> uuid mismatch: <stored> vs <new>`
    warning for the rejected one — not just our own client-side
    interpretation of the response bytes.
  - **A second, more important bug this same live-testing caught**:
    `RegisterPk` alone does NOT keep a peer online. The very first live
    attempt to drive a real `PunchHoleRequest` against a `RegisterPk`-only
    "registered" host got back "the remote device is offline" from
    `remote-core`'s own harness — a real, observed failure, not a
    hypothetical. Reading `rendezvous_server.rs`'s `update_addr` (invoked
    only by the `RegisterPeer` handler, never `RegisterPk`'s) showed why:
    hbbs checks a peer's `last_reg_time` against a 30s offline threshold,
    and only `RegisterPeer` (oneof field 6, response field 7) ever
    refreshes it. `RegistrationClient::ensure_online_once` now does the
    real dance (`RegisterPeer` → if `request_pk`, `RegisterPk` then
    `RegisterPeer` again) and is live-tested end to end: byte-for-byte, a
    fresh id gets `request_pk=true` (`3a021001`), `RegisterPk` gets `OK`
    (`820100`), and the follow-up `RegisterPeer` gets `request_pk=false`
    (`3a00`) — confirmed as the actual fix by then successfully triggering
    a real `PunchHole` forward (see below), where the same host previously
    only got "offline".

**Implemented and live-verified end to end (M2b/M2c — relay pairing +
a real session):**

- **Live-captured, not guessed**: with a host properly online (via
  `ensure_online_once`), a real `PunchHoleRequest` driven from
  `remote-core`'s `apps/remote-dev` harness produced an actual `PunchHole`
  forward on the host's registered UDP socket — `4a0e0a0aa9f64a3684a5a26d84a71802`,
  decoding to oneof field **9** (`PunchHole`, cross-checking `hbb_common`'s
  proto directly), containing `socket_addr` (field 1, the controller's
  `AddrMangle`-obfuscated address, opaque bytes we never need to decode
  ourselves — only pass through) and `nat_type` (field 3) `= 2`
  (`SYMMETRIC`) — forced because Support Plane sets `ALWAYS_USE_RELAY=Y`
  (`rendezvous_server.rs`: `if ALWAYS_USE_RELAY.load(...) { ph.nat_type =
  NatType::SYMMETRIC.into(); // will force relay }`). This also resolves an
  "Unknowns" entry `remote-core`'s own `docs/rustdesk-protocol.md` flagged:
  `RendezvousClient` already implements handling a `RelayResponse` arriving
  in place of `PunchHoleResponse` — because for a forced-relay deployment
  like ours, that IS what happens next, not a hypothetical alternate path.
- **`va-net::relay::respond_via_relay`** implements what the host does on
  `nat_type == SYMMETRIC` (the ONLY branch Verevon's deployment ever
  needs — `ALWAYS_USE_RELAY=Y` means our host never takes the
  direct-hole-punch branches at all), read directly from the native
  client's own `handle_punch_hole`/`create_relay`
  (`rustdesk@1ec1b9e7`, `src/rendezvous_mediator.rs:645-717,516-559`) and
  `create_relay_connection_` (`src/server.rs:287-335`):
  1. Generate a fresh random UUID string (the real client uses
     `Uuid::new_v4()`; format compatibility with hbbr, not the exact RNG,
     is what matters — `va-net` generates its own RFC 4122 v4 string).
  2. Resolve a relay server address from THIS DEVICE's own configuration
     (Support Plane's `SUPPORT_PLANE_RELAY_ADDR`) — the live-captured
     `PunchHole` carried an EMPTY `relay_server` field, so the host cannot
     rely on hbbs supplying one here; the real client falls back to its own
     configured default for exactly this reason.
  3. Open a NEW plain TCP connection to hbbs — the SAME host:port already
     used for UDP registration (port 21116 accepts both transports for the
     rendezvous protocol) — and send `RelayResponse{socket_addr: <the
     PunchHole's socket_addr, UNCHANGED>, uuid, relay_server, id, version}`
     (oneof field 19), framed with `va-wire`'s `BytesCodec`-style length
     prefix. No reply is read; this connection is never reused.
  4. Open a SEPARATE new TCP connection to the relay server (port 21117)
     and send `RequestRelay{uuid, licence_key}` (oneof field 18) as the
     FIRST message on it. From this point on, the SAME stream IS the raw
     peer-session channel.
  - `PunchHoleSent`/`LocalAddr` (the TCP/WS-only messages
    `rendezvous_server.rs` handles) are for the DIRECT hole-punch case
    only, which a forced-relay deployment like Verevon's never takes — the
    earlier "disagreement" recorded here was real, but doesn't matter for
    our actual configuration.
- **The full chain live-verified end to end, for real**: `crates/va-net/
  examples/listen_for_punch_hole.rs` registers a host, captures a real
  `PunchHole`, calls `respond_via_relay`, and runs a REAL `HostSession`
  (unmodified from M1) over the resulting stream against a REAL
  `remote-core` browser client (`apps/remote-dev`) with a real password.
  Result: `*** LOGGED IN -- full handshake + login succeeded end to end
  ***` on the host side, and the browser reached a genuine `connected`
  state (latency/fps stats populated) before correctly entering its
  reconnect logic once the demo process exited (a real disconnect, not a
  simulated one — the FIRST time `remote-core`'s reconnect path has been
  proven against a real, not fake-peer, disconnection). This is the first
  time ANY two independently-built implementations of this protocol have
  completed a full session with each other in this codebase.
- A Verevon Control-Plane identity contract for the agent, file transfer,
  cross-platform support, and packaging/installer remain explicitly out of
  scope; see the build plan for why.

**Implemented and live-verified end to end (M4 — real screen capture, real
mouse/keyboard input, and a resolved performance gap):**

- **Real screen capture, not a synthetic test pattern.** `va-capture::
  dxgi::DxgiDuplicationSource` wraps `windows-capture` (WinRT Graphics
  Capture + DXGI), bridging its callback-based API to this crate's
  synchronous `FrameSource::capture_frame` via a single-slot mailbox (an
  `Arc<Mutex<Option<CapturedFrame>>>` + `Condvar`) that re-serves the
  latest observed frame on every call rather than blocking for a genuinely
  new one — WGC only fires its callback when the screen actually changes,
  so a quiet capture thread over a static desktop is normal, not a hang.
  Every API claim in that module is cited against the actual
  `windows-capture 1.5.0` source (file+line), not docs.rs or memory — see
  its own doc comment. `CaptureControl::stop()` (torn down on `Drop`) is a
  REAL blocking call (posts `WM_QUIT`, then joins the capture thread), so
  `main.rs`'s video-pump shutdown was changed from `task.abort()` to a
  cooperative stop flag (checked once per tick) with a 2s abort fallback —
  an aborted task's `Drop` glue still runs synchronously wherever its stack
  unwinds, which could stall a tokio worker thread for as long as that join
  takes; letting the task exit on its own avoids compounding that with a
  forced-unwind mid-abort.
- **A real, necessary resize step this milestone surfaced**: real capture
  hands frames at the display's native resolution, never this encoder's
  fixed, measured-real-time size (see the M3 performance finding). Rather
  than rejecting a mismatch, `va-encode`'s `bgra_to_yuv420` now
  nearest-neighbor-resamples whatever size it's given (up or down) to
  exactly its configured resolution, folded directly into the existing
  per-pixel BT.601 conversion loop — `EncodeError::DimensionMismatch` is
  gone; only a genuinely empty (zero-width/height) frame is rejected now
  (`EncodeError::EmptyFrame`).
- **A second real tuning fix, found by reading `rav1e`'s own source, not
  guessing**: despite `threading` being enabled and rav1e defaulting to a
  20-thread Rayon pool on this machine, `EncoderConfig`'s `tiles`/
  `tile_cols`/`tile_rows` all defaulted to 0 — and rav1e computes real tile
  count from RESOLUTION against an AV1-spec minimum, not from available
  CPU parallelism, which came out to exactly 1 tile at both 480x270 and
  1280x720. Since the dominant per-frame cost (RDO/motion search) only
  parallelizes ACROSS tiles, this encoder ran that cost on a single thread
  the whole time. Explicitly setting `tiles: 4` (a safe 2x2 split) measured
  a real further ~1.5-2x improvement at 1280x720 on top of the M3
  `--release` fix (steady state ~0.3-1.05s/frame, down from
  0.6-2.4s/frame) and made 854x480 a genuinely close-to-viable middle
  ground. 480x270 barely changed (too small a frame to split usefully at
  64x64 superblocks) — it was already clearing the 12fps target and still
  is. `crates/va-encode/examples/bench.rs`, a new diagnostic tool kept
  permanently (not a throwaway), is what found and measured both fixes —
  rerun it after any future encoder tuning change.
- **Real mouse/keyboard input, decoded and injected, not just wired.**
  `va-protocol` gained `decode_mouse_event`/`decode_key_event`
  (`Message.mouse_event` = oneof field 10, `Message.key_event` = field 15,
  both citation-backed against `remote-core`'s own already-reviewed wire
  encoders) and two new `HostSession::Event` variants
  (`MouseInput`/`KeyInput`); `handle_session_message` decodes and hands
  them upward instead of silently discarding them. A new crate,
  `va-input`, turns a decoded event into a real `SendInput` call via
  `enigo` (confirmed via its actual source to be 100% safe on its public
  API — no `unsafe` in this workspace's own code, the same precedent
  `windows-capture` already relies on despite this workspace's
  `unsafe_code = "forbid"` lint) — absolute mouse positioning, button
  down/up, scroll, named-key down/up (a citation-backed `ControlKey` ->
  `enigo::Key` table; `CtrlAltDel`/`Power`/`LockScreen` correctly rejected
  as `Unsupported` rather than silently mismapped — Windows blocks
  synthetic Ctrl+Alt+Del from `SendInput` unconditionally, by design), and
  bulk text typing (`KeyIdentification::Seq`, what `keyboard.type()` sends)
  are all real.
  - **A real, necessary coordinate-scaling fix this milestone required**:
    incoming `MouseEvent` x/y are expressed in whatever resolution the
    video is actually encoded/reported at (480x270), which is essentially
    NEVER the real display's own resolution once real capture is
    downscaling native frames down to that fixed size. `EnigoInjector` now
    queries the real primary display's size once via `enigo`'s own
    `main_display()` and rescales every incoming coordinate before calling
    `move_mouse` — passing coordinates straight through (which is exactly
    what the synthetic-video build did, harmlessly, since nothing rendered
    was ever meant to be clicked precisely) would silently land every real
    click in the wrong place as soon as capture and encode resolutions
    ever differ, which in practice is always.
  - **Live-verified for real, on a real desktop, with the user's explicit
    live-in-the-moment sign-off** (moving the real OS cursor and injecting
    real keystrokes is a genuine physical side effect, not a sandboxed
    test): a `pointer.move`, a `pointer.click` (Down then Up, correct
    button flag), `keyboard.type` (`KeyIdentification::Seq`), and a named
    key down/up (`keyboard.keyDown`/`keyUp`, `ControlKey::Escape`,
    exercising the OTHER identification code path) were each driven from a
    real `remote-core` session against a real running `verevon-agent`, and
    each logged a real decoded event followed by "injected successfully"
    with no error — confirming the full chain (wire decode -> `Event` ->
    `va-input` -> real `enigo`/`SendInput`) end to end, not just via unit
    tests.
- **The full, now-complete pipeline, live-verified visually, on a real
  desktop**: `verevon-agent` registers, pairs via the relay, logs in, and
  streams REAL captured screen content (confirmed via a real screenshot of
  the browser canvas showing an actual application window, not a
  synthetic pattern) at real, varying encoded frame sizes (thousands to
  tens of thousands of bytes per frame, tracking real content complexity —
  a synthetic pattern's output would be far more uniform) — this is the
  first time in this initiative's history that what the controller sees is
  the actual host desktop, not a stand-in.
- **Genuinely still not built**: audio, file transfer, and anything for a
  second concurrent connection (the current registration socket design
  assumes one in-flight punch-hole exchange at a time). A real
  permission/consent model gating `MouseInput`/`KeyInput` before they ever
  reach `va-input` is still missing — this pass injects whatever the
  authenticated controller sends, unconditionally, which is fine for this
  development milestone but not for a real product. `DxgiDuplicationSource`
  only ever captures the OS-designated PRIMARY monitor
  (`Monitor::primary()`) — multi-monitor selection is unbuilt. The mouse
  wheel's scroll-delta scaling (`enigo::scroll`'s own `WHEEL_DELTA=120`
  multiplier vs `remote-core`'s raw, unscaled deltas) is unverified against
  a real trackpad/mouse. `va-encode`'s VP9 path (`encode_vp9_video_frame`)
  is tested but unused; `main.rs` sends AV1 only.

## Verification

```bash
cd "apps/Endpoint Plane"
cargo test --workspace   # 86 unit/integration tests, 5 live (ignored)
cargo clippy --workspace --all-targets  # 0 warnings
cargo build --workspace  # everything -- no exclusions needed
cargo build -p verevon-agent

# Live, not mocked -- needs `docker compose up -d hbbs hbbr readiness` in apps/Support Plane first:
cargo test -p va-net --test register_live -- --ignored --nocapture

# The real agent, running for real: registers a device, waits for a real
# connection attempt, pairs via the relay, logs in, captures and streams
# the REAL primary monitor, and injects real mouse/keyboard input the
# controller sends. Needs VEREVON_LICENCE_KEY (hbbs's own logged `Key:`
# value) and a real controller (e.g. apps/Frontend Plane/verevonv3/
# apps/remote-dev, pointed at ws://localhost:21118 with
# VITE_REMOTE_SERVER_PUBLIC_KEY set to that same key) driving a connection
# attempt against the same device id + password while it runs.
# --release is not optional -- see the M3 performance finding above.
VEREVON_AGENT_ID=<id> VEREVON_AGENT_PASSWORD=<password> VEREVON_LICENCE_KEY=<key> \
  cargo run --release -p verevon-agent

# Re-run after any encoder tuning change to confirm a real improvement:
cargo run --release -p va-encode --example bench -- 1280 720 30

# Diagnostic tool (not the real binary): registers a device id and dumps
# whatever hbbs forwards next -- what produced the original PunchHole live
# capture during M2b's research.
cargo run -p va-net --example listen_for_punch_hole -- <device-id> <password> [licence-key]
```
