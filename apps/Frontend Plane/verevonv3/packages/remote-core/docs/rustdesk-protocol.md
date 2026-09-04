# RustDesk protocol research

Research pass performed 2026-09-01 for `@verevon/remote-core`. Every claim below comes from fetching and reading actual source at the cited commit — not from blog posts, memory, or prior training data. Where something could not be confirmed, it is listed under **Unknowns** rather than guessed.

## Method and scope

Seven independent research passes were run against the live GitHub repositories, each resolving the current default branch's HEAD commit before citing permalinks. Two passes (`hbb_common`'s message catalog and its exact field numbers) downloaded the raw `.proto` files directly and read them byte-for-byte rather than through a summarizing fetch, specifically to avoid transcription errors in load-bearing numeric field IDs.

## Repository map

RustDesk is split across three repositories, not one:

| Repo | Role | Commit inspected | License |
|---|---|---|---|
| [`rustdesk/rustdesk`](https://github.com/rustdesk/rustdesk) | Native client (Rust + Flutter UI) | [`1ec1b9e7e3808b24bfc923ee1e6800ab9694e9a5`](https://github.com/rustdesk/rustdesk/commit/1ec1b9e7e3808b24bfc923ee1e6800ab9694e9a5) (`master`, 2026-08-31), manifest version `1.4.9` | AGPL-3.0 (`LICENCE`, British spelling) |
| [`rustdesk/hbb_common`](https://github.com/rustdesk/hbb_common) | Shared protocol crate — protobuf schema, crypto, framing. A **separate repo**, pulled into the client as a git submodule at `libs/hbb_common` (older writing that treats it as a subdirectory is stale) | [`1a43eb2873e8edca67ebf6fc0518c84394f3984d`](https://github.com/rustdesk/hbb_common/commit/1a43eb2873e8edca67ebf6fc0518c84394f3984d) (`main`, 2026-08-31) | **No `LICENSE` file, no `license` field in `Cargo.toml`, GitHub API reports `license: null`.** Flagged explicitly in `docs/licensing.md`. |
| [`rustdesk/rustdesk-server`](https://github.com/rustdesk/rustdesk-server) | `hbbs` (rendezvous) + `hbbr` (relay) server binaries | [`a7736be5e40f85bfc141120dce587e836e5d4b80`](https://github.com/rustdesk/rustdesk-server/commit/a7736be5e40f85bfc141120dce587e836e5d4b80) (`master`, 2026-08-07), tagged release `1.1.16` (2026-07-20) is one commit behind | AGPL-3.0 (`LICENSE`) |

## Connection lifecycle (native client)

`Client::start`/`_start` in [`src/client.rs:181-357`](https://github.com/rustdesk/rustdesk/blob/1ec1b9e7e3808b24bfc923ee1e6800ab9694e9a5/src/client.rs#L226-L357) resolves the target, optionally races a UDP hole-punch against a direct TCP attempt, and otherwise contacts the rendezvous server with a `PunchHoleRequest`. Once a `Stream` to the peer exists (direct, relayed, or LAN), everything above that is peer-to-peer application protocol.

**This entire native P2P/UDP path is unavailable to a browser.** See "What this means for a browser client" below — it is the single biggest architectural fork between the native client and `remote-core`.

## Rendezvous protocol (hbbs)

Verified directly in `rendezvous_server.rs`/`relay_server.rs` at [commit `a7736be5`](https://github.com/rustdesk/rustdesk-server/commit/a7736be5e40f85bfc141120dce587e836e5d4b80e).

| Port | Protocol | Purpose |
|---|---|---|
| 21115 | TCP | NAT-type test |
| 21116 | TCP + UDP | ID registration, rendezvous control, hole-punch signaling |
| 21117 | TCP | hbbr relay data plane |
| **21118** | **TCP, WebSocket (`ws://`)** | hbbs — **for the web client** |
| **21119** | **TCP, WebSocket (`ws://`)** | hbbr — **for the web client** |

**hbbs and hbbr already have native WebSocket listeners**, implemented directly with `tokio-tungstenite` ([`rendezvous_server.rs:1180-1216`](https://github.com/rustdesk/rustdesk-server/blob/a7736be5e40f85bfc141120dce587e836e5d4b80/src/rendezvous_server.rs#L1180-L1216), [`relay_server.rs:427-454`](https://github.com/rustdesk/rustdesk-server/blob/a7736be5e40f85bfc141120dce587e836e5d4b80/src/relay_server.rs#L427-L454)), present since at least tag `1.1.6` (Nov 2022). The bytes carried inside WS `Binary` frames are byte-identical to the raw-TCP path — same protobuf, same framing, no separate "web protocol." This directly answers the spec's question 7-8 (browser connections must use relay; the required endpoints are `ws(s)://host:21118` for rendezvous and `ws(s)://host:21119` for relay).

**It is plain `ws://`, not `wss://`.** No TLS code exists in hbbs/hbbr's own listeners — the source comments explicitly document that a reverse proxy is expected to terminate TLS and forward `X-Real-IP` (citing [issue #634](https://github.com/rustdesk-server/issues/634)). A page served over `https://` cannot open `ws://` (mixed-content blocking), so **Support Plane must front ports 21118/21119 with a TLS-terminating reverse proxy** (see `architecture.md`).

**Critical asymmetry: peer registration is UDP-only.** `RegisterPeer`/`RegisterPk` sent over the TCP/WS control channel are explicitly rejected with `NOT_SUPPORT` ([`rendezvous_server.rs:577-585`](https://github.com/rustdesk/rustdesk-server/blob/a7736be5e40f85bfc141120dce587e836e5d4b80/src/rendezvous_server.rs#L577-L585)), and there is no TCP/WS handler for it at all. **A browser can never become a dialable RustDesk peer ID — only the connecting/controlling side.** This is not a limitation for Verevon's architecture: the customer's machine runs the native Verevon Agent (which registers normally over UDP), and the browser-based support agent is always the controller. It does mean `remote-core` never needs a "become dialable" code path at all.

Rendezvous flow for a WS-only controller:

1. **Lookup**: send `PunchHoleRequest{id, conn_type, force_relay: true, version}` over WS to hbbs. `force_relay: true` (field 8) tells the server to skip the P2P/hole-punch negotiation entirely and go straight to relay — the correct choice for a client with no UDP capability, per the server's own documented intent.
2. hbbs looks up the target's registered UDP address and either replies `PunchHoleResponse{failure: ...}` (peer not found/offline) or proceeds toward a relay hint.
3. **Relay**: either side sends `RequestRelay{id, uuid}` (a client-generated session token) over WS; hbbs forwards it to the peer over UDP. Both sides then independently dial hbbr with that same `uuid`.

Steps 1 and 3 are 100% reachable over WS; step 2's *native* P2P/hole-punch branch (client-to-client raw UDP) is simply never taken by a browser client — it always falls to relay.

## Relay protocol (hbbr)

Each peer opens hbbr (TCP 21117 or WS 21119) and its first message must be `RequestRelay{uuid}` — the only message hbbr ever parses ([`relay_server.rs:461-497`](https://github.com/rustdesk/rustdesk-server/blob/a7736be5e40f85bfc141120dce587e836e5d4b80/src/relay_server.rs#L461-L497)). The first arrival is held for up to 30s awaiting its match. **After pairing, hbbr is a raw byte pipe** — it never re-parses the RustDesk message stream, just forwards bytes (wrapping each chunk in one outgoing WS `Binary` frame when either side is WS). A native-TCP peer and a WS browser peer pair transparently.

## Encryption / handshake

Crate: `sodiumoxide` (Rust bindings to libsodium/NaCl) — declared in `hbb_common/Cargo.toml`. **No TLS, no Noise/snow protocol** for this layer (rustls exists in the same crate tree only for the separate HTTPS/WSS transport option). Two independent handshake instances exist:

**1. Client ↔ rendezvous/relay server** (`secure_tcp`, [`src/common.rs:~2053-2145`](https://github.com/rustdesk/rustdesk/blob/1ec1b9e7e3808b24bfc923ee1e6800ab9694e9a5/src/common.rs)) — only engaged when a self-hosted server's Ed25519 public key is configured; skipped when TLS already covers the transport (i.e. skipped over `wss://`).

**2. Client ↔ client, end-to-end** (`secure_connection`, [`src/client.rs:~750`](https://github.com/rustdesk/rustdesk/blob/1ec1b9e7e3808b24bfc923ee1e6800ab9694e9a5/src/client.rs)) — this is the layer that matters regardless of relay, since the relay never sees these keys:

```
Bootstrap (once): each install generates a persistent Ed25519 identity keypair
and registers the public half with the ID server (RegisterPk). The server later
vouches for it: PunchHoleResponse.pk / RelayResponse.pk carries
Sign_serverKey(IdPk{id, pk}).

Controller (browser)                        Controlled (Verevon Agent)
                                               generates ephemeral X25519 keypair
  <---- Message::SignedId ---------------------  id = Sign_agentSK(IdPk{id, pk_ephemeral})
  verify signature against the peer's known
  public key -> recover pk_ephemeral
  generate random secretbox key K
  generate own ephemeral X25519 keypair
  seal K to pk_ephemeral (NaCl box, nonce=0)
  ----- Message::PublicKey{asym, sealed(K)} --->  unseal with own secret key -> K
  ==== all further Message frames: XSalsa20-Poly1305 secretbox(K), independent
       of any transport-layer encryption ====
```

- **Key exchange**: X25519 via NaCl's `box` construction (crypto_box). One random 32-byte key is generated once (`secretbox::gen_key()`) by whichever side sends `PublicKey` first, box-sealed to the recipient's ephemeral public key, and installed **verbatim, identically, on both sides** via `set_key()` — there is no independent per-side derivation.
- **Symmetric cipher**: **XSalsa20-Poly1305** ("NaCl secretbox") — *not* ChaCha20-Poly1305 or AES-GCM as might be assumed.
- **Signing**: Ed25519 (`sodiumoxide::sign`), for the long-term peer identity, not the session key itself.
- **Nonce scheme, confirmed by reading `hbb_common/src/tcp.rs` directly** (`Encrypt(Key, u64 send_counter, u64 recv_counter)`, [`tcp.rs:28,203-207,296-321`](https://github.com/rustdesk/hbb_common/blob/1a43eb2873e8edca67ebf6fc0518c84394f3984d/src/tcp.rs#L296-L321)): a 24-byte nonce with the low 8 bytes holding a little-endian `u64` counter (bytes 8-23 always zero), incremented *before* each seal/open. **Both directions' counters start at 0 and increment independently, with no role/direction marker anywhere in the nonce or the key.** This is the exact same `Encrypt` mechanism for both the client↔rendezvous-server hop and the client↔host peer-to-peer hop.
- **This is a confirmed nonce-reuse weakness, not a hypothetical one** — see `security.md`'s dedicated section for the full security analysis and why `remote-core` still replicates it (wire compatibility with an unmodified host). One caveat found in the client source: the client↔rendezvous-server hop's `secure_tcp_impl()` skips this layer entirely when the transport is already WSS (relying on TLS instead); the client↔host peer-to-peer hop does not have an equivalent skip, since it exists specifically to protect content from the relay, independent of transport.
- If the browser never received a valid signed peer key from the server, this layer is skipped and the session falls back to whatever layer-1 security is active (or plaintext P2P) — not a state `remote-core` should ever accept for a production session; see `security.md`.

## Authentication / password handling

Verified in [`src/client.rs:3495-3714`](https://github.com/rustdesk/rustdesk/blob/1ec1b9e7e3808b24bfc923ee1e6800ab9694e9a5/src/client.rs#L3495-L3714) and [`src/server/connection.rs:429-434`](https://github.com/rustdesk/rustdesk/blob/1ec1b9e7e3808b24bfc923ee1e6800ab9694e9a5/src/server/connection.rs#L429-L434).

Immediately after the crypto handshake, the controlled side sends `Hash{salt, challenge}` unprompted — `salt` is that peer's stable, persisted password salt; `challenge` is a fresh random value generated per incoming connection. The connecting client computes:

```
h1   = SHA256(password ‖ salt)
wire = SHA256(h1 ‖ challenge)
```

and sends `wire` as `LoginRequest.password`. **Plain double SHA-256 — no scrypt/Argon2/PBKDF2 anywhere in this path.** `h1` is what a client may cache (never the plaintext); `wire` is single-use and replay-resistant because `challenge` is fresh per connection. This is implemented identically in `remote-core` as [`RustDeskPasswordAuthenticator`](../src/protocol/rustdesk/RustDeskPasswordAuthenticator.ts), verified against a hand-computed test vector.

`LoginResponse` is a `oneof` of `string error` (drawn from a small fixed vocabulary: `"Empty Password"`, `"Wrong Password"`, `"Wrong 2FA Code"`, `"2FA Required"`, `"No Password Access"`, `"Offline"`) or a `PeerInfo` marking success.

**Two-factor auth is host-local TOTP** (SHA-1, 30s step, `totp_rs` crate) layered after the password check — not a central-account or SMS/email OTP system. If required, the client sends a follow-up `Message{auth_2fa: Auth2FA{code, hwid}}`. **`remote-core` does not implement the 2FA flow in this version** — it is detected (the `"2FA Required"` error surfaces as a typed `AuthenticationError`) but not yet driven to completion; see Unknowns.

Independently, each host keeps a persistent "permanent password" and an auto-rotating "temporary"/one-time password; from the wire's perspective both are just "a password" checked the same way — the host alone decides which stored value(s) a hash may match. This is the basis for Verevon's own short-lived session tokens: **a token issued by Verevon's backend and used as the RustDesk password works today, unmodified, exactly the way a temporary/one-time password does** — no protocol change is required for that eventual flow described in the top-level spec's "Authentication" section.

## Message catalog and wire framing

Both `.proto` files (`message.proto`, `rendezvous.proto`) declare `syntax = "proto3"` and `package hbb`, compiled via Google's `protobuf` crate (not `prost`). Wire framing is a custom variable-length prefix (`hbb_common/src/bytes_codec.rs`, `BytesCodec`): the low 2 bits of the first byte select a 1/2/3/4-byte header, the length is the header value right-shifted by 2. This wraps the (possibly encrypted) serialized `Message`/`RendezvousMessage`. **No sequence numbers or reassembly** — the underlying transport (TCP, WS, or WebRTC data channel) is always reliable and ordered, so a message is written and read whole. Optional **zstd** compression is applied per-field by the sender (confirmed for `Clipboard.compress`; the video raw-pixel path's `RGB.compress`/`YUV.compress` flags are proto-confirmed but their sender call site was not traced — see Unknowns), never to the whole envelope.

`remote-core` reimplements this independently (not copied from the `.proto` source) in [`src/protocol/rustdesk/wire/`](../src/protocol/rustdesk/wire/) (hand-written proto3 varint/zigzag/length-delimited writer+reader, golden-tested against Google's own public protobuf encoding example) and [`src/protocol/rustdesk/messages/`](../src/protocol/rustdesk/messages/) (message-specific encode/decode using the field numbers below). See `licensing.md` for why this is a clean-room reimplementation, not a copy.

Exact field numbers below were extracted by downloading the raw `.proto` files directly (not through a summarizing fetch) and are reproduced here as protocol facts (interoperability data, not the `.proto` file's own copyrightable text/comments):

**`Message` envelope** (`oneof union`) — the fields `remote-core` currently sends or decodes:

| Field | # | Direction in remote-core |
|---|---|---|
| `signed_id` | 3 | send + receive (handshake) |
| `public_key` | 4 | send + receive (handshake) |
| `video_frame` | 6 | receive only |
| `login_request` | 7 | send only |
| `login_response` | 8 | receive only |
| `hash` | 9 | receive only |
| `mouse_event` | 10 | send only |
| `key_event` | 15 | send only |
| `clipboard` | 16 | send + receive |
| `misc` | 19 | receive only (`permission_info` sub-field) |
| `peer_info` | 25 | receive only (nested inside `LoginResponse`) |

Everything else on the envelope (audio, file transfer, terminal, chat, voice call, screenshot, cliprdr, plugin, elevation — fields 5, 11-14, 17-18, 20-24, 26-32) is explicitly out of MVP scope per the top-level product spec and decodes to a typed `{ kind: 'unknown', fieldNumber }` rather than being silently dropped or misinterpreted.

**Auth/handshake**: `LoginRequest{username(target peer id)=1, password=2, my_id=4, my_name=5, video_ack_required=9, session_id(uint64)=10, version=11, my_platform=13, hwid=14, avatar=17}` (fields 6, 7-8, 12, 15-16 — `option`, file-transfer/port-forward/view-camera/terminal oneof branches, `os_login` — intentionally unsent). `LoginResponse{error(string,oneof)=1, peer_info=2, enable_trusted_devices=3}`. `Hash{salt=1, challenge=2}`. `PublicKey{asymmetric_value=1, symmetric_value=2}`. `SignedId{id=1}`. `PeerInfo{username=1, hostname=2, platform=3, displays(repeated DisplayInfo)=4, current_display=5, sas_enabled=6, version=7}`. `DisplayInfo{x(sint32)=1, y(sint32)=2, width=3, height=4, name=5, online=6, cursor_embedded=7, original_resolution=8, scale(double)=9}`.

**Input**: `MouseEvent{mask(int32)=1, x(sint32)=2, y(sint32)=3, modifiers(repeated ControlKey, packed)=4}`, where `mask = (buttonFlags << 3) | eventKind` — kinds `Move=0, Down=1, Up=2, Wheel=3, Trackpad=4, MoveRelative=5`; button flags `Left=0x01, Right=0x02, Wheel/Middle=0x04, Back=0x08, Forward=0x10`. `KeyEvent{down=1, press=2, control_key/chr/unicode/seq/win2win_hotkey(oneof)=3-7, modifiers(packed)=8, mode(enum: Legacy=0/Map=1/Translate=2/Auto=3)=9}`. Coordinates are **absolute pixels in the host's virtual-desktop space**, not normalized and not display-relative on the wire — the client is responsible for adding the target display's own (possibly negative) origin before sending, and RustDesk's own Flutter client does this in Dart, not Rust.

**Clipboard**: `Clipboard{compress=1, content(bytes)=2, width=3, height=4, format(enum)=5, special_name=6}`, `format` ∈ `{Text=0, Rtf=1, Html=2, ImageRgba=21, ImagePng=22, ImageSvg=23, Special=31}`. Bidirectional, event-driven with a 333ms polling fallback, gated by `Misc.PermissionInfo{permission=Clipboard(2)}`.

**Video**: `VideoFrame` oneof — `vp9s=6, rgb=7(unsupported, see Unknowns), yuv=8(unsupported), h264s=10, h265s=11, vp8s=12, av1s=13`, plus `display(int32)=14` outside the oneof. `EncodedVideoFrame{data=1, key(bool)=2, pts(int64)=3}`; `EncodedVideoFrames{frames(repeated)=1}` — **one wire message can batch multiple encoded chunks (e.g. a keyframe plus catch-up frames); a single encoded frame is never fragmented across multiple wire messages.** This materially simplifies a browser client: no jitter buffer or sequence-number reassembly is needed, only "decode whatever chunks arrived in this batch."

**Permissions**: `Misc` is itself a ~30-variant oneof; `remote-core` decodes only `permission_info=6` (`PermissionInfo{permission(enum)=1, enabled(bool)=2}`, `permission` ∈ `{Keyboard=0, Clipboard=2, Audio=3, File=4, Restart=5, Recording=6, BlockInput=7, PrivacyMode=8}`). Permissions are pushed as **discrete per-toggle messages, not a bundled snapshot** — confirmed by the client's handler living in the same long-running per-connection loop as video/clipboard traffic, not a login-only handler. This is why `remote-core`'s `PermissionManager` models permissions as a live, updatable set rather than a fixed grant issued once at connect time.

**Rendezvous**: `RendezvousMessage` oneof — `punch_hole_request=8, punch_hole_response=11, request_relay=18, relay_response=19, key_exchange=25` (the fields `remote-core` uses; registration/hole-punch/NAT-test fields are not needed by a WS-only controller). `PunchHoleRequest{id=1, nat_type=2, licence_key=3, conn_type=4, token=5, version=6, force_relay(bool)=8}`. `PunchHoleResponse{pk=2, failure(enum)=3, relay_server=4, other_failure=7}` — see Unknowns for a real ambiguity in how to detect success here. `RequestRelay{id=1, uuid=2, relay_server=4, secure=5, licence_key=6, conn_type=7, token=8}`. `RelayResponse{relay_server=3, refuse_reason=6, version=7}`. `KeyExchange{keys(repeated bytes)=1}`.

## Video codec and display handling

Verified in `libs/scrap/build.rs` and `libs/scrap/src/common/*.rs`.

| Codec | Encode | Decode | Notes |
|---|---|---|---|
| VP8 | libvpx | libvpx | Software, always available |
| VP9 | libvpx | libvpx | Software, always available |
| AV1 | libaom | libaom | Software, always available |
| H264 | Hardware only (`hwcodec` → NVENC/QSV/AMF/VAAPI/VideoToolbox/MediaCodec) | Hardware only | **No software fallback anywhere in RustDesk** |
| H265 | Hardware only, same path | Hardware only | No software fallback |

Negotiation: `PeerInfo.encoding: SupportedEncoding{h264, h265, vp8, av1, i444}` at login, plus a runtime `OptionMessage.supported_decoding: SupportedDecoding{ability_vp9/h264/h265/vp8/av1, prefer, prefer_chroma}` a viewer can send later to change its declared capability. Priority order on the encoder side: `h265 > h264 > av1/vp9/vp8`, gated by whether *every* connected viewer supports a codec. **Implication for `remote-core`: since H264/H265 have no software path, and WebCodecs hardware support varies by browser/device, `remote-core` should declare and prefer VP9/VP8/AV1 in its `SupportedDecoding` capability advertisement** — these are guaranteed decodable (via WebCodecs where hardware-accelerated, falling back to WebCodecs' software path) regardless of the viewer's GPU, whereas advertising H264 support risks the host choosing a codec the browser can only decode if hardware acceleration happens to be available.

Frame packetization: no fragmentation (see Message catalog above) — a real jitter-buffer/reassembly layer, as native RTP would need, is not required.

Display/resolution: `PeerInfo.displays: DisplayInfo[]` + `current_display` at login; `Misc.switch_display` announces a live monitor change; every `VideoFrame.display` tags which monitor a given frame batch belongs to, so a client demultiplexes multiple simultaneous displays by that index rather than by a side channel.

## Official browser client precedent

**An official browser-based RustDesk client exists** — "RustDesk Web Client V2" (`rustdesk.com/web`, announced October 2024) — built from Flutter's own web-compilation target for the UI, paired with hand-written TypeScript/JS (`flutter/web/js`, `flutter/lib/web`) for what Flutter-web cannot do: transport and video decode. Its transport is WebSocket (confirmed via `hbb_common/src/websocket.rs`, which implements the identical `Message`/framing contract as the TCP path). Third-party automated code analysis (not independently verified byte-for-byte in this research pass) describes its decode path as WebCodecs `VideoDecoder` for VP8/VP9/H264/H265 with a WASM-compiled software VP8 fallback, composited via `createImageBitmap` — architecturally very close to what `remote-core` is built to do.

**RustDesk never exposes a native `<video>`-attachable media stream.** Even its WebRTC support (`hbb_common/src/webrtc.rs`) is data-channel-only (SCTP), carrying the same opaque application-protocol bytes — there is no RTP/SRTP video track anywhere in the project. This confirms the top-level spec's assumption that WebSocket (not WebTransport, not WebRTC media) is the correct transport for `remote-core`: WebTransport has no RustDesk-side counterpart to speak to, and building one would require a translation proxy with no protocol-compatibility benefit.

Per the product requirement to build Verevon's own UI rather than embed RustDesk's ("Do NOT embed, iframe, reproduce, or depend on the standard RustDesk UI"), `remote-core` does not use or depend on the Flutter web client's code in any way — this section exists purely to document that the general approach (browser + WebSocket + WebCodecs) is proven feasible, and as evidence for the transport/codec decisions above. Independent third-party reimplementations in the same spirit as `remote-core` (`marcpope/cortendesk`, `lichon/rustdesk-web-ts`) were found during this research but not inspected for license or code quality — they are cited only as evidence that from-scratch browser reimplementation of this protocol is a validated, previously-attempted approach, not as a source of any kind.

## What this means for a browser client (summary)

1. **Browser is always the controller, never the controlled host** — no UDP registration path exists for it. `remote-core`'s `RemoteProtocol` never needs a "become dialable" mode.
2. **Always request relay** (`PunchHoleRequest.force_relay = true`) — a browser cannot do UDP hole-punching, and the server has a first-class, documented way to skip straight to it.
3. **WSS via a reverse proxy is mandatory** — hbbs/hbbr's native WS listeners are plaintext; Support Plane must terminate TLS in front of ports 21118/21119 (see `architecture.md`).
4. **No jitter buffer needed** — frames arrive whole, in order, over a reliable transport.
5. **Prefer VP9/VP8/AV1** in codec capability negotiation — H264/H265 have no software decode fallback anywhere in the ecosystem.
6. **The key-code mapping table (browser → RustDesk `KeyEvent`) has no published spec** — even RustDesk's own official client crosses this boundary awkwardly (Flutter → USB HID → Windows scancode, with a filed, acknowledged bug for IME/soft-keyboard input). This is real, unsolved integration work, not a formality.

## Unknowns / could not verify

Listed explicitly per the product spec's instruction to document gaps rather than guess:

- **Raw RGB/YUV pixel transport** (`VideoFrame.rgb`/`.yuv`, fields 7-8): these messages carry only a `compress` flag with no `bytes` field; a source comment says plane data is "sent directly in binary" outside the protobuf message proper. The exact call site was not located. `remote-core` does not support these variants (`decodeVideoFrame` returns `undefined` for them) — only the encoded-codec variants (vp9s/vp8s/h264s/h265s/av1s) are implemented, which is sufficient for the WebCodecs-based pipeline this library targets.
- **`PunchHoleResponse` success/failure discriminant**: `failure` (field 3) is not marked as part of any `oneof`, and its `IdNotExist=0` value is indistinguishable on the wire from "field absent." `remote-core` treats an empty `relay_server`/`pk` as the failure signal instead (documented in `RdPunchHoleResponse`'s own doc comment) — this should be verified against a real hbbs response before shipping.
- **Host-side `Auth2FA` validation and re-login sequencing** — the client-side send was confirmed; the host's validation/response was not traced.
- **`OSLogin`'s own field numbers** — referenced as a nested type in `LoginRequest` field 12 but not independently confirmed; `remote-core` never sends this field as a result (also the safer default — it never transmits OS-level credentials).
- **`KeyboardMode::Auto`'s selection logic** and the exact rule for choosing `KeyEvent.unicode` vs `KeyEvent.seq` for composed/IME text — not found in the inspected source.
- **Exact timing of the first `PermissionInfo` push** relative to login (an initial full set vs. delta-only) — not confirmed; `remote-core`'s `PermissionManager` starts from whatever `RemoteProtocol.permissions` reports at connect time (currently empty by default, the safe choice) and waits for explicit `permission-change` events, never assuming a default-allow state.
- **Whether the free/OSS `rustdesk-server` needs anything beyond the WSS reverse-proxy requirement** to fully back "Web Client V2" — the announcement blog could only be read via a search-engine snippet (direct fetch returned 403), so this is not fully confirmed from a primary source.
