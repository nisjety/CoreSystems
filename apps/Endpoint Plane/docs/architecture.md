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

**Not started at all:**

- `va-net` — real UDP/TCP networking (hbbs registration + keepalive, hbbr
  relay pairing). Blocked on a real, separate research gap: the exact
  `RegisterPk` request wire shape and keepalive cadence were never
  determined by `remote-core`'s own research (a browser can never become a
  dialable RustDesk peer, so it never needed to construct that message) —
  only the *response* shape was captured, from a single ad hoc poke against
  a live hbbs, not a sustained registration proof. Guessing this would be
  worse than leaving it unbuilt: closing it needs the same citation-backed
  source read every other field number in this codebase was held to,
  before `va-net` is written.
- Input injection, file transfer, a Verevon Control-Plane identity contract
  for the agent, cross-platform support, packaging/installer, and any UI —
  all explicitly out of scope for this pass; see the build plan for why.

## Verification

```bash
cd "apps/Endpoint Plane"
cargo test -p va-crypto -p va-wire -p va-protocol -p va-testkit   # 42 tests
cargo clippy -p va-crypto -p va-wire -p va-protocol -p va-testkit --all-targets  # 0 warnings
cargo build --workspace --exclude va-encode   # everything except the pkg-config-blocked crate
cargo build -p verevon-agent                  # the actual binary
```
