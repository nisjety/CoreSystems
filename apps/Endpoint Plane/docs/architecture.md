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

**Confirmed to compile on this real Windows machine, not yet exercised:**

- `va-capture` — wraps the `windows-capture` crate (MIT, built on the
  official `windows` crate; an independent third party's implementation of
  Microsoft's public capture APIs, unrelated to RustDesk's own `scrap`
  crate). Builds cleanly. No actual capture code written yet, and no
  `FrameSource` trait implementation exists beyond the plan's design.

**Blocked on a missing system dependency, not a code problem:**

- `va-encode` — `vpx-encode` (VP9/libvpx bindings) needs `pkg-config` and a
  system libvpx to link, neither of which is installed on this machine.
  This was flagged as a real risk before any code was written (`vpx-encode`
  hasn't published a release since 2022); resolving it (installing
  pkg-config + libvpx for MSVC, or forking/replacing the `-sys` crate) is
  M2 work, not attempted here. The root `verevon-agent` binary deliberately
  does not depend on `va-capture`/`va-encode`/`va-net` yet for exactly this
  reason — `cargo build --workspace --exclude va-encode` and
  `cargo build -p verevon-agent` both succeed cleanly today.

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
  for the agent, cross-platform support, packaging/installer, and any UI —
  all explicitly out of scope for this pass; see the build plan for why.
- **Genuinely still not built**: continuous video (the demo exits right
  after login — it doesn't push frames), input handling, a real
  `main.rs` wiring registration + relay + capture/encode together into one
  long-running process, and anything for a second concurrent connection
  (the current registration socket design assumes one in-flight punch-hole
  exchange at a time).

## Verification

```bash
cd "apps/Endpoint Plane"
cargo test -p va-crypto -p va-wire -p va-protocol -p va-testkit -p va-net   # 56 unit tests, 4 live (ignored)
cargo clippy -p va-crypto -p va-wire -p va-protocol -p va-testkit -p va-net --all-targets  # 0 warnings
# Live, not mocked -- needs `docker compose up -d hbbs hbbr readiness` in apps/Support Plane first:
cargo test -p va-net --test register_live -- --ignored --nocapture
cargo build --workspace --exclude va-encode   # everything except the pkg-config-blocked crate
cargo build -p verevon-agent                  # the actual binary

# The capstone live demo: registers a device, captures a real PunchHole,
# pairs via the relay, and runs a real HostSession login. Needs a real local
# Support Plane AND a real controller driving a connection attempt against
# the same device id + password while it runs (e.g. apps/Frontend Plane/
# verevonv3/apps/remote-dev via `pnpm -C "apps/Frontend Plane/verevonv3"
# --filter remote-dev dev`, pointed at ws://localhost:21118 with
# VITE_REMOTE_SERVER_PUBLIC_KEY set to hbbs's logged Key:).
cargo run -p va-net --example listen_for_punch_hole -- <device-id> <password> [licence-key]
```
