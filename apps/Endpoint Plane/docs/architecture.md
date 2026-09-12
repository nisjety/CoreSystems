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
- **A real, measured performance finding, not yet solved**: pure-Rust
  `rav1e` (no SIMD `asm`, per the codec-choice rationale above) encoding a
  1280x720 frame took **6-38 seconds per frame** on this machine — nowhere
  near real-time. Dropping to 480x270 brought steady-state frame time down
  to roughly ~1 second (still well short of the 12fps target, but a
  sustained, working stream rather than a stall). `main.rs` runs at 480x270
  today for exactly this reason; a real performance pass (tiling to use
  the already-enabled `threading` feature, a faster/lower-quality preset,
  or accepting the `asm`/`nasm` dependency after all) is unstarted follow-up
  work, not solved by this pass.
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
- Input injection, file transfer, a Verevon Control-Plane identity contract
  for the agent, cross-platform support, and packaging/installer are all
  explicitly out of scope for this pass; see the build plan for why.
- **Genuinely still not built**: real screen capture (`DxgiDuplicationSource`
  — video is a synthetic test pattern today), real-time-speed encoding at a
  useful resolution (measured 480x270 at well under the 12fps target — see
  above), audio, and anything for a second concurrent connection (the
  current registration socket design assumes one in-flight punch-hole
  exchange at a time). `va-encode`'s VP9 path (`encode_vp9_video_frame`) is
  tested but unused; `main.rs` sends AV1 only.

## Verification

```bash
cd "apps/Endpoint Plane"
cargo test --workspace   # 65 unit tests, 5 live (ignored)
cargo clippy --workspace --all-targets  # 0 warnings
cargo build --workspace  # everything, including va-encode -- no exclusions needed anymore
cargo build -p verevon-agent

# Live, not mocked -- needs `docker compose up -d hbbs hbbr readiness` in apps/Support Plane first:
cargo test -p va-net --test register_live -- --ignored --nocapture

# The real agent, running for real: registers a device, waits for a real
# connection attempt, pairs via the relay, logs in, and continuously
# streams synthetic video. Needs VEREVON_LICENCE_KEY (hbbs's own logged
# `Key:` value) and a real controller (e.g. apps/Frontend Plane/verevonv3/
# apps/remote-dev, pointed at ws://localhost:21118 with
# VITE_REMOTE_SERVER_PUBLIC_KEY set to that same key) driving a connection
# attempt against the same device id + password while it runs.
VEREVON_AGENT_ID=<id> VEREVON_AGENT_PASSWORD=<password> VEREVON_LICENCE_KEY=<key> \
  cargo run -p verevon-agent

# Diagnostic tool (not the real binary): registers a device id and dumps
# whatever hbbs forwards next -- what produced the original PunchHole live
# capture during M2b's research.
cargo run -p va-net --example listen_for_punch_hole -- <device-id> <password> [licence-key]
```
