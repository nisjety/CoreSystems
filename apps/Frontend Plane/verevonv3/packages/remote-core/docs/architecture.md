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

Verevon's own backend — the Rust gateway (`apps/gateway`) with Control Plane behind it for identity — is a separate system. `remote-core` only ever deals with the live remote-desktop session itself; the credential flows in as `ConnectOptions.auth.token` and is used exactly like a RustDesk temporary password (see `rustdesk-protocol.md`, "Authentication").

**How the SPA is wired to this today (2026-09-07):**

- **Gateway** — `apps/gateway/src/domains/remote_support.rs` serves `GET /api/v1/remote-support/config` (session-gated), returning the Support Plane connection details from `REMOTE_SUPPORT_RENDEZVOUS_URL` / `REMOTE_SUPPORT_RELAY_URL` / `REMOTE_SUPPORT_SERVER_PUBLIC_KEY`. It reports `configured: false` plus the missing variable names rather than inventing a default host, and `configured` is strict — it requires the server public key, because the product UI must never fall back to an unverified handshake. The gateway is only a *config read*: the live session runs browser→hbbs/hbbr over WSS and never transits it.
- **UI** — `src/features/support/components/RemoteSupportPage.tsx`, the fourth surface of the Support workspace (`/support?surface=remote`). Device ID + the password the customer reads out, connect, `CanvasRenderer` on a canvas that forwards pointer/keyboard input through `session.pointer`/`session.keyboard` only when the customer has granted the matching permission (view-only otherwise), live state/latency/fps, and a real `AIObserver` at 0.5 fps that starts paused.
- **Deliberately not built yet, and the UI says so**: (1) a durable remote-session record for audit — no core owns one, so there is no action-registry contract for it (a dispatcher with no persistence would be fabricated state); (2) forwarding AI-sampled frames to Verevon AI — needs a Model Plane contract with ZDR handling. The observer samples and counts locally, nothing leaves the browser.

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

Per this project's explicit "do not fake functionality" rule, this section is the honest map of implementation depth. It distinguishes three levels of confidence deliberately, because they are not the same thing:

- **unit-tested** — verified in isolation.
- **fake-peer verified** — verified against `tests/doubles/FakeRustDeskPeer.ts`, which independently implements the *host* side of the handshake (its own signing keys, its own protobuf encoders in the test tree, its own secretbox channel). This exercises the real sequence, the two-level signature chain, nonce/counter synchronisation in both directions, and the login challenge-response.
- **browser-verified** — actually executed in a real browser.
- **live-verified** — actually exchanged bytes with a real `rustdesk/rustdesk-server` container.

**The full connection path is now live-verified end to end (2026-09-10/11), against a real, unmodified stock RustDesk 1.4.9 Windows client registered with a locally-run Support Plane** — the first time any of this has exchanged bytes with a real host rather than `FakeRustDeskPeer`. `remote-dev` connected, reached `connected` state, and streamed real decoded video (30 fps at first, settling to ~16 fps against a mostly-static screen; 4 ms latency, consistent with a loopback relay hop) for over an hour without disconnecting. Concretely, this settles:

- **Rendezvous**: real registration/lookup against a real hbbs (already live-verified before this pass; see `rustdesk-protocol.md`, "Verified against a live hbbs").
- **Relay pairing**: hbbr logged a real `New relay request … got paired` for this session — the negotiation genuinely completes against the real relay binary, not just the fake peer's approximation of it.
- **Peer handshake / `SecureChannel` vouch chain**: since `verifyServerVouch` fails closed on any format mismatch (see below) and the session still reached `connected`, the real host's vouch actually matches what `SecureChannel` expects. This settles open question 3 from the old "Next verification step" list: **yes, the vouch format matches.**
- **Login and the session pump**: reaching `connected` requires a successful `LoginRequest`/`LoginResponse` round trip; the subsequent live video stream requires the session pump to actually be decoding and dispatching real frames.
- **Initial permission state**: a second live pass (2026-09-11) read `session.permissions.list()` right after connecting — `['screen.view']` only, no `input.pointer`/`input.keyboard`, no consent dialog ever appeared. Settles the old "full push vs. deltas" question: on a one-time-password connection to a stock client with default settings, it's neither — only view is ever granted. `assumeInputPermittedOnLogin` staying `false` by default is the correct call.

**Still open even after these passes** — real plumbing proven, not every remaining protocol unknown: which of the two relay-negotiation reply shapes was actually used (no debug-level log was captured to tell `PunchHoleResponse` from `RelayResponse`), which video codec was actually negotiated (so the H264/H265 Annex-B assumption is neither confirmed nor contradicted — VP9, the guaranteed/default codec, is the likely candidate but wasn't explicitly checked), scroll delta scaling, and real-host cursor decompression. Pointer, keyboard, scroll, and cursor remain unexercised — not skipped this time but genuinely **blocked**: the host never granted `input.pointer`/`input.keyboard` at all, so there was nothing for those code paths to send (see "Next verification step" below).

**Implemented and fake-peer verified — the whole Phase 2 connection path:**
- `RustDeskProtocol` (`protocol/rustdesk/RustDeskProtocol.ts`) — the full sequence: rendezvous → relay pairing → X25519/Ed25519 handshake → `Hash`/`LoginRequest`/`LoginResponse` login → session pump. Implements `RemoteProtocol`, so nothing above it knows RustDesk exists.
- `RendezvousClient` — the hbbs conversation, always with `force_relay: true`. Handles **both** plausible relay-negotiation replies (`PunchHoleResponse` *and* `RelayResponse`) because the research could not settle which one a force-relay request actually gets; both are tested.
- `SecureChannel` + `verifyServerVouch` — the two-level certificate chain (server vouches for the peer's identity key; that key signs the peer's ephemeral exchange key). **Fails closed**: an empty `SignedId`, a vouch signed by the wrong key, an identity/peer-ID mismatch, or a missing server key all refuse the session rather than continuing unauthenticated. RustDesk's own client silently degrades here; we do not.
- `MessageInbox` — buffered await-by-predicate, which removes a real race (a message arriving between subscribing and awaiting).
- Login, actions (`pointer.*`, `keyboard.*`, `clipboard.write`, `display.select`), host-pushed permission changes, host clipboard updates, host display-switch confirmations, and `TestDelay` in both directions — all round-tripped through the fake peer.
- **Two-factor login**: a host answering `"2FA Required"` gets one `Auth2FA` from `SessionAuthenticator.provideSecondFactor()` (or `auth.secondFactor` with the token shorthand); wrong code and missing provider both fail with a typed `AuthenticationError`.
- **Latency**: host probes are echoed verbatim (which is what keeps the host's adaptive bitrate working) and their `last_delay` is surfaced; the client also sends its own probe every 2 s (`latencyProbeIntervalMs`, 0 disables) and measures the echo. Both feed `latency` events and `stats.latencyMs`. An echo carrying no timestamp, or an implausible round trip, is discarded rather than reported as a measurement.
- **Codec declaration**: `LoginRequest.option` now carries `supported_decoding` built from a real `detectMediaCapabilities()` probe, plus `disable_audio: Yes` (the host otherwise subscribes its audio service by default and Opus-encodes a stream we drop). `preferredCodec` defaults to VP9 — the only codec the protocol guarantees. The `i444`/`prefer_chroma` fields are deliberately never sent; see `rustdesk-protocol.md` for why copying the reference client there would produce an undecodable stream.
- **Quality control**: `quality.set` is a first-class action (`low`/`medium`/`high`/`auto`) sent as `Misc.option` carrying only `image_quality`. The `quality` event reports the level actually in effect **plus the host's live `target_bitrate` in kbps** — the level is never guessed from the number.
- **Cancellation**: `ConnectOptions.signal` is now honoured throughout the connect sequence (rendezvous, relay connect, every `expect()`, the authenticator, and the two-factor prompt). It was previously accepted, threaded through two layers, and read by nobody.
- **Automatic reconnection**: a `relay-failure`/`transient-network` loss on a connected session puts it in `reconnecting`, and `Reconnector` (`client/Reconnector.ts`) re-runs the full connect with the same credentials under exponential backoff (`ReconnectPolicy`, default 5 attempts, 0.5 s → 8 s, 45 s per-attempt deadline). Success returns to `connected` on the same `RemoteSession` object with its listeners intact; `session.disconnect()` mid-loop cancels it. A session built without a reconnect capability goes straight to `disconnected` rather than sitting in a `reconnecting` state nothing will ever leave. The outcome is a discriminated union, so the three endings are reported honestly and differently: **exhausted** (all attempts failed), **refused** (an `AuthenticationError`/`PermissionDeniedError` that retrying cannot fix — the loop stops after one attempt), and **cancelled**. A non-retryable disconnect arriving *during* a reconnect (a clean remote shutdown, a revoked permission) cancels the loop and reports that real reason immediately, instead of burning every attempt and then inventing a "gave up after 5 attempts" error. An automatic retry never prompts for a two-factor code — five background attempts would mean five dialogs the operator never asked for.
- **Remote cursor**: `CursorData`/`cursor_id`/`CursorPosition` are decoded; `colors` is zstd-decompressed with **fzstd** (pure JS, MIT, zero dependencies, ~65 KB unpacked — the one runtime dependency added for this, because the wire format is always compressed with no flag) into a preallocated buffer of exactly `width*height*4` bytes, so a lying frame header cannot force a large allocation, and dimensions are bounds-checked (≤ 1024 px) before anything is allocated. Shapes are cached per session by id (capped at 64, like the host's own cache) because the host sends a bare `cursor_id` for shapes it already sent; an unknown id keeps the current cursor rather than blanking it. Positions arrive in the host's global desktop coordinates and are converted to the streamed display's local space using `DisplayInfo.x/y` (refreshed from `SwitchDisplay`). `show_remote_cursor: Yes` is now sent at login — without it the host never sends positions. `CanvasRenderer` draws the overlay from its own retained clone of the last frame (so the pointer keeps moving on a static screen, where no new frames arrive), hides it until the first position, and skips it when the display reports `cursorEmbedded`. Every failure path here is soft: a bad cursor costs one cursor, never the session.
- **Real statistics**: `bitrateKbps` is measured from bytes actually received over a rolling second (the host's own target is reported separately on the `quality` event); `droppedFrames` comes from the decoder's pre-keyframe drops; `renderedFrames` is reported by whatever renderer is attached, through an internal `RenderStatsSink` that keeps the reporting channel off the public `RemoteSession` type. With no renderer attached, `renderedFrames` stays 0 — which is correct, not a hole.
- `createRemoteClient({ rendezvousUrl })` — the zero-config path now works, including the `auth: { token, secondFactor }` shorthand.

**Browser-verified:**
- libsodium's WASM crypto actually initialises and round-trips in a browser: X25519 sealed-key exchange plus XSalsa20-Poly1305 secretbox between two channel instances.
- `detectMediaCapabilities()` probes WebCodecs for real (reported vp8/vp9/av1/h264/h265 on a current Chromium).
- The `remote-dev` harness loads the library, runs the real connect path, and reports failures honestly.

**Unit-tested (unchanged from the first pass):** the protocol-agnostic public surface, the proto3 wire codec (golden-tested against Google's published encoding example), the message codecs, `RustDeskPasswordAuthenticator`, coordinate math, frame sampling, and the session state machine.

**Implemented but NOT verified anywhere yet:**
- `RemoteVideoDecoder` — the WebCodecs pipeline is written (codec-string mapping, lazy configure, codec switching on keyframes, dropping delta frames before the first keyframe) but has never decoded a real RustDesk bitstream. Node has no `VideoDecoder`, so the test only asserts that a frame arriving without WebCodecs degrades gracefully instead of tearing down the session.
- `CanvasRenderer`'s paint path — exercised structurally, never with real frames.

**Known gaps, deliberately not faked:**
- **Initial permission state is a genuine unknown.** Whether the host pushes a full permission set at login or only deltas was never confirmed. Default is safe (only `screen.view` after login, input denied); `assumeInputPermittedOnLogin` opts into granting input up front, and exists precisely because if the host is delta-only, a strict client would reject all input forever. This is the flag most likely to need flipping during the first live test.
- **Scroll units are undefined by the protocol.** Deltas are passed through unscaled; the host applies platform-specific sign flips and a ×120 factor on Windows. Expect empirical tuning.
- **Keyboard is character/ControlKey-based, not scancode-based.** RustDesk's position-based "Map" mode needs a USB-HID→scancode table that exists only inside its `rdev` crate. Text and navigation work; software that reads raw scancodes (games) will not.
- **2FA asks for exactly one code.** Whether a real host tolerates a second `Auth2FA` on the same connection after `"Wrong 2FA Code"` is unverified, so a wrong code fails the connect; the UI reconnects to retry. A *re-issued* challenge (the host answering our code with another `"2FA Required"`, i.e. an expired TOTP window) is reported with its own distinct message rather than the one used when no code was ever requested.
- Worker-based decode and region-cropped AI observation remain Phase 6 items.

**Explicitly out of scope for this version** (per the top-level spec's MVP prioritization): file transfer, remote audio, remote printing, TCP tunneling, chat, address book, recording, and the raw RGB/YUV video path (unresolved wire format — see `rustdesk-protocol.md`, Unknowns).

## Next verification step

The first live run against a real host is done (see Status above) — done with a stock RustDesk 1.4.9 client as the Verevon Agent stand-in, exactly as anticipated below. Two things worth not rediscovering:

- **`SUPPORT_PLANE_KEY=_` breaks a real connection the moment a relay is needed**, even though it looks fine through rendezvous alone. hbbs and hbbr each auto-generate their *own* identity keypair on `_` (they use separate data volumes/processes), so `hbbr` rejects the relay request with "invalid key" — a real host never gets past that point. Support Plane's own README previously called `_` "fine locally"; that was only true for a rendezvous-only smoke test. Use a real, explicit, shared `SUPPORT_PLANE_KEY` for both services — a base64 libsodium/sodiumoxide Ed25519 **secret** key (64 raw bytes) works cleanly, since `rustdesk-server`'s `get_server_sk()` derives the same public key from it deterministically on both sides; see Support Plane's README for the exact recipe.
- The stock client's custom server settings live in `%APPDATA%\RustDesk\config\RustDesk2.toml`'s `[options]` table as `custom-rendezvous-server` / `relay-server` / `key` — **not** the file's top-level `rendezvous_server` field, which the app treats as its own resolved-value cache and freely overwrites. This isn't discoverable by editing that field and watching it "take" once, because the app echoes your value back into it before you notice it was actually `custom-rendezvous-server` doing the work; only a restart exposes the difference.

A second live pass (2026-09-11) connected again with `remote-dev`'s new permission readout live, and answered question 2 for real — see below.

What the first two runs settled is in Status above, plus:

1. Which relay-negotiation reply a `force_relay` request actually gets (`PunchHoleResponse` vs `RelayResponse` — both branches are implemented and tested; the live runs didn't capture debug-level logs to tell which one fired).
2. ~~Whether the host pushes an initial permission set or only deltas.~~ **Settled, for a one-time-password connection on a stock RustDesk 1.4.9 client with default settings: neither, really — the host grants exactly `screen.view` and nothing else.** `session.permissions.list()` read `['screen.view']` right after login; no `input.pointer`/`input.keyboard`, and no consent dialog or notification ever appeared on the host asking to grant more. This matches remote-core's existing safe default (`assumeInputPermittedOnLogin: false`) — that default should **stay** false; flipping it would have made this client falsely believe it had input control it was never granted. What remains genuinely open: whether a *permanent* password / "Approve Mode" / explicit "Enable Keyboard/Mouse" host-side setting changes this, since only the default one-time-password path was tested. Until an agent-side setting grants it, pointer/keyboard/scroll cannot be exercised against this exact client at all — items below were blocked by this, not skipped.
3. ~~Whether the peer-handshake vouch format matches what `SecureChannel` expects.~~ **Settled: yes** (see Status above).
4. Whether the H264/H265 Annex-B assumption in `RemoteVideoDecoder` holds against a real stream — not confirmed either way, since which codec the live runs actually negotiated wasn't captured.
5. Scroll delta scaling — still blocked on item 2 (no `input.pointer` permission to send a scroll through).
6. Whether a real host's `CursorData.colors` decompresses cleanly through fzstd and lands on the RGBA/hotspot/coordinate semantics documented in `rustdesk-protocol.md` — the fake peer proves our own encode/decode round-trips, not that a real capture backend's zstd frames match.

`apps/remote-dev` only rendered video until the first pass — it had no pointer/keyboard wiring at all. It now mirrors `RemoteSupportPage.tsx`'s exact `canPoint`/`canType`-gated pointer/keyboard handlers and shows granted permissions in its status bar; that readout is what actually settled item 2. Testing 1, 4, and 6 needs either debug-level server logs or reading `remote-core`'s own decoded state, not just harness changes. Testing 5 additionally needs an agent-side setting change to grant `input.pointer` at all, per item 2.

A live test rig is inherently a loopback if the "remote" machine is the same one running the browser controller (there being no separate Verevon Agent machine yet) — mind that pointer/keyboard testing this way would drive the *tester's own* live desktop, not an expendable target, once/if a client ever does grant that permission.

## Extension points

- **Transports**: `Transport` ([`transport/Transport.ts`](../src/transport/Transport.ts)) is deliberately narrow (`connect`/`send`/`close`/`onMessage`/`onClose`/`onError`). `WebSocketTransport` is the only implementation; per `rustdesk-protocol.md`'s finding that RustDesk has no WebTransport or RTP-media path at all, a `WebTransportTransport` would need a translation proxy with no protocol benefit — not planned unless that changes upstream.
- **Protocols**: any class implementing `RemoteProtocol` plugs into `RemoteClient`/`RemoteSession` unchanged.
- **Action middleware**: `session.actions.use((action, next) => ...)` — the same seam used for AI-approval gating is available for audit logging, rate limiting, or replay, per the top-level spec's design intent.
