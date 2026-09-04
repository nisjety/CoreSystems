# Architecture

## Where this fits

```text
Verevon SPA (Solid, verevonv3)
        │
        ▼
@verevon/remote-core           <- this package (packages/remote-core)
        │
        ├── connection/session management   (client/, protocol/RemoteProtocol.ts)
        ├── RustDesk protocol               (protocol/rustdesk/*  — isolated, see below)
        ├── crypto                          (crypto/*)
        ├── video/audio decoding            (media/*)
        ├── input encoding                  (input/*, clipboard/*)
        ├── permissions                     (permissions/*)
        ├── AI frame access                 (ai/*)
        │
        ▼
WSS (wss://…:21118 rendezvous, wss://…:21119 relay — see rustdesk-protocol.md)
        │
        ▼
RustDesk hbbs / hbbr            <- apps/Support Plane, unmodified upstream server
        │
        ▼
Verevon Agent (native, on the customer's machine — registers as a normal
                RustDesk peer over UDP; out of scope for this package)
        │
        ▼
Remote Windows/macOS/Linux computer
```

Verevon's Convex-based backend (users, auth, tickets, session metadata, billing) is a separate system entirely. `remote-core` only ever deals with the live remote-desktop session itself; a Verevon session token flows in as `ConnectOptions.auth.token` and is used exactly like a RustDesk temporary password (see `rustdesk-protocol.md`, "Authentication").

## RustDesk protocol isolation

This is the single most load-bearing design decision in the package, and it is enforced structurally, not just by convention:

- Every module outside `src/protocol/rustdesk/` talks only to the `RemoteProtocol` interface ([`src/protocol/RemoteProtocol.ts`](../src/protocol/RemoteProtocol.ts)) — `connect`, `sendAction`, `captureFrame`, `disconnect`, and a small typed event stream (`frame`, `display-change`, `latency`, `quality`, `permission-change`, `clipboard`, `state`, `disconnect`).
- `RustDeskProtocol` (under `src/protocol/rustdesk/`) is the only class that implements it today. A future, different backend (or a second RustDesk-compatible server with a different transport) implements the same interface without touching `client/`, `actions/`, `input/`, `renderer/`, `ai/`, or any public type.
- The public type surface (`src/types/public.ts`) never imports anything from `protocol/rustdesk/`. `AuthenticationChallenge`/`AuthenticationResponse` are shaped like RustDesk's actual salt/challenge scheme (because that's a real, useful, protocol-agnostic pattern), but the concrete SHA-256 algorithm lives in `protocol/rustdesk/RustDeskPasswordAuthenticator.ts`, not in the public types.
- `createRemoteClient({ rendezvousUrl, relayUrl })`'s zero-config shorthand constructs a `RustDeskProtocol` internally; `createRemoteClient({ protocol })` accepts any `RemoteProtocol` implementation directly. Both return the same `RemoteClient` type.

## Module map

```text
src/
  client/            RemoteClient, RemoteSession, the session state machine, stats tracking
  protocol/
    RemoteProtocol.ts        <- the seam described above
    rustdesk/
      RustDeskPasswordAuthenticator.ts
      wire/                  <- hand-written proto3 varint/zigzag/length-delimited codec
      messages/              <- message-specific encode/decode, using verified field numbers
      (RustDeskProtocol.ts, RendezvousClient.ts, RelayClient.ts — see Status below)
  crypto/            SessionCrypto (X25519/XSalsa20-Poly1305 handshake) — see Status below
  transport/         Transport interface + WebSocketTransport (the only concrete impl today)
  actions/           ActionExecutor — permission check, then middleware chain, then protocol.sendAction
  permissions/       PermissionManager — live, updatable permission set
  input/             PointerController, KeyboardController (public convenience wrappers)
  clipboard/         ClipboardController
  renderer/           CoordinateMapper (pure math) + CanvasRenderer (paints frames, owns no protocol state)
  ai/                FrameSampler (rate-limited frame stream) + AIObserver
  media/             RemoteVideoFrame helpers (resource release)
  events/            EventBus — generic typed pub/sub
  errors/            Typed error hierarchy
  logging/           Injectable Logger interface (no console.* in production code)
  types/             public.ts (exported surface) / internal.ts (state machine + permission-mapping helpers)
```

## State machine

Internal states are more granular than the public `session.state` union, because RustDesk's rendezvous phase is a real, distinct sub-step that the protocol layer needs to reason about even though the UI doesn't need to render it separately:

```text
idle -> connecting -> rendezvous -> authenticating -> connected -> reconnecting -> connected
                                                            \-> disconnected / failed (terminal)
```

`idle` and `rendezvous` collapse into the public `"connecting"` state. Transitions are validated by [`SessionStateMachine`](../src/client/SessionStateMachine.ts) against an explicit table — an illegal transition throws a `ProtocolError` rather than silently succeeding. `classifyDisconnect()` ([`types/internal.ts`](../src/types/internal.ts)) decides whether a given `DisconnectReason` lands in `reconnecting` (only `transient-network`/`relay-failure`, and only if the session was actually connected), `failed` (`authentication-failed`, `protocol-error`, `unknown`), or `disconnected` (everything else — user-initiated, remote shutdown, revoked permission, session timeout: all real, non-error terminations).

`RemoteClient.connect()` mirrors whatever the protocol implementation reports via its own `state` events into a local state machine as it progresses through `connecting → rendezvous → authenticating`, and only resolves the returned `Promise<RemoteSession>` once `connected` is actually reached (or rejects with a typed error otherwise). This means a `RemoteSession` object is never observed by calling code in a non-connected state — matches the "Target developer experience" in the top-level spec, where `await client.connect(...)` is followed by attaching listeners for *subsequent* transitions (`reconnecting`, `disconnected`, `failed`), not the initial ramp-up.

## Data flow (target, once the media pipeline lands)

```text
WebSocket (relay)
    ↓
decrypt (SessionCrypto — XSalsa20-Poly1305, per rustdesk-protocol.md)
    ↓
protobuf decode (protocol/rustdesk/messages)
    ↓
VideoFrame dispatch (vp9s/vp8s/av1s/h264s/h265s -> WebCodecs VideoDecoder)
    ↓
RemoteVideoFrame (ImageBitmap | VideoFrame | rgba) — the shared, decoder-agnostic shape
    ↓                                    ↓
CanvasRenderer (paints)          FrameSampler / AIObserver (rate-limited, opt-in)
```

`RemoteVideoFrame` is deliberately not tied to a canvas (`media/VideoFrame.ts`) — the renderer and the AI observer are both independent *consumers* of the same event stream (`session.on('frame', ...)`), exactly per the "This separation is extremely important" requirement in the top-level spec. Decode should eventually move to a Worker to keep the main thread free (Phase 6 in the top-level plan); this version runs everything on the main thread and documents that as a known, deliberate scope cut (see Status).

## Status: what's real, what's pending

Per this project's explicit "do not fake functionality" rule, this section is the honest map of implementation depth. Everything listed as "implemented" has passing unit tests; nothing here claims to have been verified against a live `rustdesk-server` or a real Verevon Agent, because no such infrastructure was deployed during this work.

**Implemented and tested:**
- The entire public API surface (types, events, errors, permissions, action middleware chain, state machine, coordinate math, frame sampling/rate-limiting, session/client wiring) — protocol-agnostic, fully unit-tested with `MockProtocol`/`MockTransport` test doubles, no real network needed.
- `RustDeskPasswordAuthenticator` — the exact double-SHA-256 challenge-response scheme, verified against a hand-computed test vector.
- The proto3 wire codec (`protocol/rustdesk/wire/`) — varint, zigzag sint32, 64-bit values via `bigint`, length-delimited fields, packed repeated fields, golden-tested against Google's own public protobuf encoding example, plus a regression test for large payloads (video-frame-sized byte arrays) that specifically guards against a spread-argument stack overflow.
- Message-level encode/decode (`protocol/rustdesk/messages/`) for the MVP set: login handshake (`Hash`, `PublicKey`, `SignedId`, `LoginRequest`, `LoginResponse`, `PeerInfo`, `DisplayInfo`), input (`MouseEvent`, `KeyEvent`), `Clipboard`, `PermissionInfo`, `VideoFrame`/`EncodedVideoFrame` (codec variants only, not the raw rgb/yuv path), and the rendezvous set (`PunchHoleRequest`/`Response`, `RequestRelay`/`RelayResponse`, `KeyExchange`) — all round-trip tested against the verified field numbers.

**Interface-defined, not yet implemented (isolated behind `RemoteProtocol`, not faked):**
- `SessionCrypto` — the X25519/XSalsa20-Poly1305 handshake described in `rustdesk-protocol.md`. Needs a WASM libsodium binding (the same crypto library RustDesk itself uses, via its official distribution — not a hand-rolled crypto primitive) wired to real key material.
- `RendezvousClient`/`RelayClient` — the WebSocket orchestration that drives `PunchHoleRequest → PunchHoleResponse → RequestRelay → RelayResponse` against a real hbbs/hbbr, using the message codecs above.
- `RustDeskProtocol` itself — the class that implements `RemoteProtocol` by wiring the crypto, framing, and message layers together into the full connect/login/session-pump sequence.
- `createRemoteClient`'s zero-config (`rendezvousUrl`-only) branch depends on the above existing; today `createRemoteClient` requires an explicit `protocol` instance.
- The WebCodecs-backed `VideoDecoder`, the codec-negotiation logic (advertising `SupportedDecoding`), and Worker-based decode.
- 2FA (`Auth2FA`) completion — detected, not driven end-to-end.
- Wire-level `BytesCodec` variable-length framing on top of the WebSocket transport (WebSocket already provides message framing at the transport level, so this is only needed if/when a raw-TCP-equivalent transport is added).
- `CanvasRenderer`'s actual paint path is implemented against real Canvas2D/WebCodecs APIs but has not been exercised in a live browser in this session (no dev server with a real video stream existed to test against).

**Explicitly out of scope for this version** (per the top-level spec's MVP prioritization): file transfer, remote audio, remote printing, TCP tunneling, chat, address book, recording, and the raw RGB/YUV video path (unresolved wire format — see `rustdesk-protocol.md`, Unknowns).

## Extension points

- **Transports**: `Transport` ([`transport/Transport.ts`](../src/transport/Transport.ts)) is deliberately narrow (`connect`/`send`/`close`/`onMessage`/`onClose`/`onError`). `WebSocketTransport` is the only implementation; per `rustdesk-protocol.md`'s finding that RustDesk has no WebTransport or RTP-media path at all, a `WebTransportTransport` would need a translation proxy with no protocol benefit — not planned unless that changes upstream.
- **Protocols**: any class implementing `RemoteProtocol` plugs into `RemoteClient`/`RemoteSession` unchanged.
- **Action middleware**: `session.actions.use((action, next) => ...)` — the same seam used for AI-approval gating is available for audit logging, rate limiting, or replay, per the top-level spec's design intent.
