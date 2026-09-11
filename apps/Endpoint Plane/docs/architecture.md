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

**Not started, and a genuine open research question (M2b — relay pairing):**

- What a real host is supposed to do when hbbs forwards a `PunchHole`/
  `FetchLocalAddr` message once a controller sends `PunchHoleRequest` for
  it. Three independent source reads disagree on the reply channel:
  `rustdesk-server`'s own handlers (`handle_hole_sent`/`handle_local_addr`)
  only accept the host's `PunchHoleSent`/`LocalAddr` reply over a TCP/WS
  control connection (its UDP arm for those is an explicit no-op, citing
  reflection/amplification abuse) — but the real native client's traced
  behavior (`rendezvous_mediator.rs::handle_punch_hole`) instead opens a
  **brand-new UDP socket** and sends `PunchHoleSent` straight to the
  controller's decoded address, never touching a TCP/WS channel to hbbs at
  all for this reply (with a third branch for a host-initiated relay
  connection when NAT type is symmetric). Guessing between these would
  repeat exactly the mistake M0 avoided. Closing it needs a live capture:
  register a host, drive a real `PunchHoleRequest` against the same local
  hbbs (e.g. via `remote-core`'s own `RendezvousClient`, which already
  implements sending it with `force_relay: true`), and observe on the
  host's UDP socket and on any TCP/WS connection whether `PunchHole`/
  `FetchLocalAddr` actually arrives and which channel the expected reply
  belongs on — before `va-net` grows any punch-hole/relay-pairing code.
- Input injection, file transfer, a Verevon Control-Plane identity contract
  for the agent, cross-platform support, packaging/installer, and any UI —
  all explicitly out of scope for this pass; see the build plan for why.

## Verification

```bash
cd "apps/Endpoint Plane"
cargo test -p va-crypto -p va-wire -p va-protocol -p va-testkit -p va-net   # 46 unit tests, 2 live (ignored)
cargo clippy -p va-crypto -p va-wire -p va-protocol -p va-testkit -p va-net --all-targets  # 0 warnings
# Live, not mocked -- needs `docker compose up -d hbbs hbbr readiness` in apps/Support Plane first:
cargo test -p va-net --test register_live -- --ignored --nocapture
cargo build --workspace --exclude va-encode   # everything except the pkg-config-blocked crate
cargo build -p verevon-agent                  # the actual binary
```
