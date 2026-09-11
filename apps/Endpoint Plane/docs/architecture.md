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

**Implemented and tested (M1 — zero I/O, no sockets, no OS dependency):**

- `va-crypto` — Ed25519 identity signing (attached signatures), X25519
  sealed-box key exchange, XSalsa20-Poly1305 secretbox with the wire
  protocol's exact per-direction counter-based nonce scheme, and the
  double-SHA-256 password hashing scheme. The password hash is cross-checked
  **byte-for-byte** against an independent Node computation of the same
  algorithm `remote-core` uses. 9 tests.
- `va-wire` — hand-written proto3 varint/zigzag/length-delimited codec
  (verified against Google's own published varint example) plus
  `BytesCodec`-style variable length-prefix TCP framing (1-4 byte header,
  low 2 bits select width) — framing `remote-core` never needed (browser
  WebSocket frames already delimit one message each), new territory here.
  15 tests.
- `va-protocol` — the message catalog (`SignedId`, `PublicKey`, `Hash`,
  `LoginRequest`/`Response`, `PeerInfo`/`DisplayInfo`, `VideoFrame`
  (VP9, batched `EncodedVideoFrame`s), `TestDelay`, `Misc`/`PermissionInfo`,
  `SwitchDisplay`) and `HostSession`, the host-side per-connection state
  machine (handshake -> login -> session pump). 14 unit tests plus **4
  end-to-end tests** that drive `HostSession` through a complete real
  handshake and login against `va-testkit`'s independent `FakeController`
  implementation — covering a full successful login, a wrong-password
  rejection followed by a successful retry on the same connection,
  signature rejection against the wrong identity key, and the
  `TestDelay{from_client: true}` echo contract once connected.
- `va-testkit` — `FakeController`, the inverse of `remote-core`'s own
  `tests/doubles/FakeRustDeskPeer.ts`: an independent implementation of the
  controller side (own ephemeral keypair, own sealing, own login-request
  construction) used only to test `HostSession`, never shipped.

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

**M2b — relay pairing: the wire spec is now resolved, the code isn't
written yet.**

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
- **What the host does on `nat_type == SYMMETRIC`** (the ONLY branch
  Verevon's deployment ever needs — `ALWAYS_USE_RELAY=Y` means our host
  never takes the direct-hole-punch branches at all), read directly from
  the native client's own `handle_punch_hole`/`create_relay`
  (`rustdesk@1ec1b9e7`, `src/rendezvous_mediator.rs:645-717,516-559`):
  1. Generate a fresh random UUID string (the real client uses
     `Uuid::new_v4()`; format compatibility with hbbr, not the exact RNG,
     is what matters).
  2. Resolve a relay server address from THIS DEVICE's own configuration
     (Support Plane's `SUPPORT_PLANE_RELAY_ADDR`) — the live-captured
     `PunchHole` carried an EMPTY `relay_server` field, so the host cannot
     rely on hbbs supplying one here; the real client falls back to its own
     configured default for exactly this reason.
  3. Open a NEW plain TCP connection to hbbs — the SAME host:port already
     used for UDP registration (port 21116 accepts both transports for the
     rendezvous protocol; confirmed by `RendezvousMediator::start`'s
     `host` string being shared verbatim between `start_udp` and this TCP
     `connect_tcp(&*self.host, ...)` call).
  4. Send `RelayResponse{socket_addr: <the PunchHole's socket_addr field,
     UNCHANGED>, uuid, relay_server, id, version}` as a `RendezvousMessage`
     (oneof field 19) framed with the SAME `BytesCodec`-style length prefix
     `va-wire` already implements for the peer session — this is a plain,
     short-lived TCP send, not a persistent control channel; the real
     client never reuses this socket afterward.
  5. Separately, open the actual relay data connection to the relay server
     from step 2, using the SAME uuid, and run `HostSession` over it exactly
     as `va-testkit`'s `FakeController` already exercises against a fake
     TCP stream — this is the point M1's protocol state machine and M2b's
     networking finally meet.
  - `PunchHoleSent`/`LocalAddr` (the TCP/WS-only messages
    `rendezvous_server.rs` handles) are for the DIRECT hole-punch case
    only, which a forced-relay deployment like Verevon's never takes — the
    earlier "disagreement" recorded here was real, but doesn't matter for
    our actual configuration, and does not need resolving to proceed.
  - Not yet implemented: `va-net` doesn't send `RelayResponse` or connect
    to hbbr yet. This is the concrete next step, not a research gap.
- Input injection, file transfer, a Verevon Control-Plane identity contract
  for the agent, cross-platform support, packaging/installer, and any UI —
  all explicitly out of scope for this pass; see the build plan for why.

## Verification

```bash
cd "apps/Endpoint Plane"
cargo test -p va-crypto -p va-wire -p va-protocol -p va-testkit -p va-net   # 49 unit tests, 4 live (ignored)
cargo clippy -p va-crypto -p va-wire -p va-protocol -p va-testkit -p va-net --all-targets  # 0 warnings
# Live, not mocked -- needs `docker compose up -d hbbs hbbr readiness` in apps/Support Plane first:
cargo test -p va-net --test register_live -- --ignored --nocapture
cargo build --workspace --exclude va-encode   # everything except the pkg-config-blocked crate
cargo build -p verevon-agent                  # the actual binary

# Diagnostic tool (not a test) that produced the PunchHole live capture above:
# registers a device id and dumps whatever hbbs forwards next. Drive a real
# connection attempt against the same id (e.g. via apps/remote-dev) while it runs.
cargo run -p va-net --example listen_for_punch_hole -- <device-id>
```
