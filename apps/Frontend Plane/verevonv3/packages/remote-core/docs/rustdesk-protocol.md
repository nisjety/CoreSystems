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

**Two-factor auth is host-local TOTP** (SHA-1, 30s step, `totp_rs` crate) layered after the password check — not a central-account or SMS/email OTP system. The host answers a *correct* password with `LoginResponse{error: "2FA Required"}` and then waits, on the same encrypted channel, for `Message{auth_2fa: Auth2FA{code(string)=1, hwid(bytes)=2}}` (field 27 of `Message`); it replies with either `PeerInfo` (success) or `"Wrong 2FA Code"`. `remote-core` drives this to completion when the `SessionAuthenticator` exposes `provideSecondFactor()` (the `auth.token` shorthand takes an `auth.secondFactor` callback); without a provider it fails with a typed `AuthenticationError` that says so, rather than hanging. `hwid` is always sent empty — it is RustDesk's "trust this device" handle and the browser persists no device identity. Field numbers verified 2026-09-07 against `libs/hbb_common/protos/message.proto` (see the catalog below).

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

**Latency, display switching, 2FA** (field numbers extracted 2026-09-07 from `libs/hbb_common/protos/message.proto`, `rustdesk/rustdesk` master, download SHA-256 `3d6fb2c…`): `Message.test_delay=5` → `TestDelay{time(int64)=1, from_client(bool)=2, last_delay(uint32)=3, target_bitrate(uint32)=4}`. The host sends one roughly every second and expects it echoed back **verbatim** — that echo is how it measures RTT and drives adaptive bitrate; `last_delay` in a host probe is its previous measurement. A client-originated probe (`from_client=true`) is echoed by the host unchanged. `Misc.switch_display=5` → `SwitchDisplay{display(int32)=1, x(sint32)=2, y(sint32)=3, width(int32)=4, height(int32)=5, cursor_embedded(bool)=6, resolutions=7, original_resolution=8}` — the client sends it with only `display` set; the host confirms with the full geometry, and only that confirmation should move the "current display" marker. `Message.auth_2fa=27` → `Auth2FA{code(string)=1, hwid(bytes)=2}`. Also extracted (not yet used by `remote-core`): `Message.cursor_data=12` → `CursorData{id(uint64)=1, hotx(sint32)=2, hoty(sint32)=3, width=4, height=5, colors(bytes)=6}`, `Message.cursor_position=13` → `CursorPosition{x(sint32)=1, y(sint32)=2}`, `Message.cursor_id=14` (bare uint64), `Misc.change_display_resolution=36` → `DisplayResolution{display=1, resolution=2}`, `OptionMessage.supported_decoding=10` → `SupportedDecoding{ability_vp9=1, ability_h264=2, ability_h265=3, prefer(PreferCodec)=4, ability_vp8=5, ability_av1=6, i444=7, prefer_chroma=8}` with `PreferCodec{Auto=0, VP9=1, H264=2, H265=3, VP8=4, AV1=5}`.

**Client settings (`OptionMessage`)** — extracted 2026-09-07 from `message.proto` plus `src/server/connection.rs` (`update_options`, line ~4703), `libs/scrap/src/common/codec.rs` (`Encoder::update`, `Decoder::supported_decodings`) and `src/client.rs` (`LoginConfigHandler`). Carried two ways: `LoginRequest.option = 6` at login, and `Misc.option = 7` mid-session.

`OptionMessage{image_quality=1, lock_after_session_end=2, show_remote_cursor=3, privacy_mode=4, block_input=5, custom_image_quality=6, disable_audio=7, disable_clipboard=8, enable_file_transfer=9, supported_decoding=10, custom_fps=11, disable_keyboard=12, follow_remote_cursor=15, follow_remote_window=16, disable_camera=17, terminal_persistent=18, show_my_cursor=19}`.

Four facts here are easy to get wrong, and each has a real failure mode:

- **`BoolOption` is `{NotSet=0, No=1, Yes=2}`** — *not* a boolean. Writing `1` for "yes" sends an explicit **No**, and an explicit No is not the same as absent: the host reads presence as `q != BoolOption::NotSet` and truth as `q == BoolOption::Yes`. Because proto3 elides zeroes, `NotSet` is byte-identical to omitting the field, so model it as an optional tri-state. Values ≥ 3 decode to an unknown enum that the host skips but still counts as "set" for its scope checks — never emit anything but 0/1/2.
- **`custom_image_quality` is a percentage shifted left by 8** (`percent << 8`). The host does `ratio = ((q >> 8 & 0xFFF) * 2) / 100`, clamped to `[0.2, 40.0]`, and multiplies a resolution-derived base bitrate by it. Sending a raw `50` gives `50 >> 8 == 0` → ratio 0.0 → clamped to the minimum: **the worst possible quality, silently**. `remote-core` therefore only sends the `image_quality` enum (`Low=2, Balanced=3, Best=4`), never a custom percentage.
- **`SupportedDecoding.ability_*` are `int32` used as 0/1 flags** (the host only tests `> 0`). Declaring `ability_h265 = 0` is a hard guarantee against receiving H265: the host's `Encoder::update` requires `all()` peers to declare an ability before that codec becomes selectable. **But `ability_vp9` is never read at all** — VP9 is the hardcoded baseline (`ENCODE_CODEC_FORMAT` starts as VP9, and the `prefer` filter accepts `PreferCodec::VP9` with no usability gate), so there is no protocol escape from VP9. A browser that cannot decode VP9 cannot be protected by this field; `remote-core` logs a warning instead of pretending otherwise.
- **Do not copy the reference client's `i444 { vp9: true, av1: true }`.** `i444`/`prefer_chroma` negotiate 4:4:4 chroma, which for VP9 means the host encodes **profile 1** — a bitstream most WebCodecs VP9 decoders reject, with no wire flag to explain the black screen. Omitting both fields reads as "cannot do I444", which is what a browser wants. `remote-core` never emits them.

Omitting `LoginRequest.option` entirely is **safe, not broken**: `Connection::update_codec_on_login()` registers such a peer as `NewOnlyVP9`, i.e. VP9-only. It costs two things, which is why `remote-core` now sends it — the host otherwise subscribes the **audio** service by default and streams Opus we would drop (`disable_audio = Yes` prevents the subscription entirely, so the host never even encodes), and without `show_remote_cursor = Yes` the host never sends `CursorPosition` at all.

**Quality/keyframe control**: mid-session changes go as `Misc.option` carrying **only** the changed field — the host's `is_supported_decoding_only_option` compatibility check requires a `supported_decoding` update to travel alone, so never piggyback quality onto it. `Misc.refresh_video = 10` (bool, acts only when true) and `Misc.refresh_video_display = 31` (int32 display index) are the only client-initiated keyframe requests; both make the host tear down and rebuild the encoder, whose first frame is a keyframe. **A display switch does not need one** — the host's `switch_display_to` re-subscribes the video service, which bails `"SWITCH"` and restarts the encoder on its own. The host reports exactly two live quality numbers, both on `TestDelay`: `last_delay` (its measured RTT, ms) and `target_bitrate` (its encoder's current target, kbps). There is no host→client message naming a quality *level*.

**Cursor** (researched 2026-09-07, implemented 2026-09-08 — see `architecture.md`): `Message.cursor_data = 12` → `CursorData{id(uint64)=1, hotx(sint32)=2, hoty(sint32)=3, width=4, height=5, colors(bytes)=6}`; `Message.cursor_position = 13` → `CursorPosition{x(sint32)=1, y(sint32)=2}`; `Message.cursor_id = 14` (bare uint64). The decisive fact: **`colors` is always zstd-compressed** (level 3), with no flag in the message and no negotiation — decoding it as raw pixels yields garbage, so a browser client needs a zstd decoder. Decompressed it is exactly `width*height*4` bytes of **non-premultiplied RGBA**, top-down, tightly packed, which maps directly onto `new ImageData(...)` with no channel swap. `cursor_id` re-selects a previously-sent shape, so a client **must** keep an `id → {bitmap, hotx, hoty}` map for the session; an unknown id should keep the current cursor rather than blank it. `CursorPosition` is in the host's **global virtual-desktop** coordinates (hence sint32/negative), so local position is `CursorPosition - SwitchDisplay.x/y`; `hotx/hoty` are subtracted from that. There is no initial position snapshot — nothing arrives until the remote pointer actually moves — and the host deliberately suppresses position echoes to whichever peer sent input in the last 300 ms. `cursor_embedded` means "already painted into the video, do not draw your own", but no current RustDesk capture backend ever sets it true.

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

## Verified against a live hbbs (2026-09-07)

Everything above came from reading source. This section is different: it is what a real `rustdesk/rustdesk-server:latest` container actually did when `remote-core`'s own compiled code talked to it. These supersede inference.

- **The WebSocket listener is real and on 21118.** hbbs logs `Listening on websocket :21118` at startup, alongside `tcp/udp :21116` and `tcp :21115`. hbbr logs `Listening on websocket :21119`. Exactly as documented above.
- **Our `PunchHoleRequest` encoding is accepted.** Sending 22 bytes — `42 14 | 0a 09 "999000111" | 32 05 "1.4.9" | 40 01` — produced a structured reply rather than silence or a drop. Field 8 (`punch_hole_request`), field 1 (`id`), field 6 (`version`), field 8 (`force_relay: true`) all parsed. This is the first hard evidence that the extracted field numbers interoperate.
- **Our `PunchHoleResponse` decoding is correct.** The reply `5a 02 18 03` decoded to field 11 (`punch_hole_response`) with `failure = 3`, which our failure table maps to `LICENSE_MISMATCH`.
- **The access key and the server public key are the same string.** hbbs generates one Ed25519 pair at first start, writes `id_ed25519`/`id_ed25519.pub`, and logs `Key: <base64>`. Sending that value as `PunchHoleRequest.licence_key` clears the license check; sending an empty one yields `failure = 3`. `RustDeskProtocolOptions.licenceKey` therefore falls back to `serverPublicKey` when omitted, because the alternative is a confusing mismatch error on an otherwise-correct configuration.
- **An unknown device ID returns a completely EMPTY `PunchHoleResponse`** — literally `5a 00`, two bytes. There is no `failure` field on the wire at all; the server relies on proto3's default `0` (`ID_NOT_EXIST`). This is precisely the ambiguity flagged under Unknowns, now confirmed: `failure` cannot be distinguished from "absent", so `RendezvousClient` keys off an empty `relay_server` **and** empty `pk` instead. Captured as a regression test.
- **`RegisterPk` over UDP is accepted.** An empty `RegisterPkResponse` (`82 01 00`, field 16) came back — again proto3 defaults, meaning `result = 0` (`OK`).
- **hbbs goes silent when the target peer does not answer.** After registering an ID whose UDP socket was then closed, a `PunchHoleRequest` for that ID produced *no reply at all*. This confirms hbbs forwards the request to the peer over UDP and only answers the caller once the peer reacts — so a browser client genuinely cannot progress past rendezvous without a live, responsive registered host.

**Still unverified, and why:** the relay pairing, the peer handshake, login, and the session stream all require a real registered host on the other end. The Verevon Agent does not exist yet, and a fake UDP registrant is not enough (see the silence finding above). In particular, **which reply a `force_relay` request gets once a real host is present remains open** — `RendezvousClient` implements both branches.

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
- **Which reply a `force_relay` request actually gets.** Both readings are plausible from the schema: either `PunchHoleResponse` (and the client then picks a uuid and sends `RequestRelay` to hbbs), or `RelayResponse` directly (carrying uuid + relay address + vouch). `RendezvousClient` implements and tests **both** paths rather than betting on one, but only a live hbbs can say which actually happens.
- ~~`TestDelay`'s internal field numbers~~ — **resolved 2026-09-07** (see the message catalog); latency is now measured from both host probes and client probes.
- ~~`SwitchDisplay`'s internal field numbers~~ — **resolved 2026-09-07**; `display.select` sends `Misc.switch_display` and applies the host's confirmation. Still unverified: whether a real host re-sends `PeerInfo`/a keyframe promptly after the switch (the fake peer only confirms with `SwitchDisplay`).
- **Whether the host pushes an initial `PermissionInfo` set at login, or only deltas.** This one has direct functional consequences: if it is delta-only, a client that waits for explicit grants will never be allowed to send input. Hence the `assumeInputPermittedOnLogin` option, defaulting to the safe-but-possibly-non-functional behaviour.
- **Whether RustDesk's H264/H265 streams are Annex-B** (which is what `RemoteVideoDecoder` assumes, configuring WebCodecs without a `description`). Plausible given the hwcodec/FFmpeg pipeline, but unverified against a real stream.
- **Whether hbbs/hbbr inspect or rewrite `LoginRequest.option` in transit.** The research covered the client/host repo only; the peer stream is end-to-end encrypted, so the relay *should* be opaque to it, but that was not verified against `rustdesk-server` sources.
- **Whether the alpha in `CursorData.colors` is premultiplied on X11 hosts specifically.** RustDesk never premultiplies or corrects anywhere, and its own clients render the bytes as straight alpha — but the XFixes protocol defines its cursor image as premultiplied ARGB, and no line of RustDesk source reconciles that. Windows and macOS are confirmed straight alpha.
- **Host-side `Auth2FA` sequencing is implemented from the proto and the login-error vocabulary, not from a traced live exchange.** The fake peer answers a correct password with `"2FA Required"`, accepts one `Auth2FA`, and replies `PeerInfo`/`"Wrong 2FA Code"`; whether a real host allows more than one code attempt on the same connection is unverified, so `remote-core` asks the authenticator exactly once and fails on rejection.
- **`OSLogin`'s own field numbers** — referenced as a nested type in `LoginRequest` field 12 but not independently confirmed; `remote-core` never sends this field as a result (also the safer default — it never transmits OS-level credentials).
- **`KeyboardMode::Auto`'s selection logic** and the exact rule for choosing `KeyEvent.unicode` vs `KeyEvent.seq` for composed/IME text — not found in the inspected source.
- **Exact timing of the first `PermissionInfo` push** relative to login (an initial full set vs. delta-only) — not confirmed; `remote-core`'s `PermissionManager` starts from whatever `RemoteProtocol.permissions` reports at connect time (currently empty by default, the safe choice) and waits for explicit `permission-change` events, never assuming a default-allow state.
- **Whether the free/OSS `rustdesk-server` needs anything beyond the WSS reverse-proxy requirement** to fully back "Web Client V2" — the announcement blog could only be read via a search-engine snippet (direct fetch returned 403), so this is not fully confirmed from a primary source.
