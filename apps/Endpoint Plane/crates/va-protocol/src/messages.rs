//! `RustDesk` wire message catalog: field numbers, message shapes, and
//! encode/decode functions for exactly the messages the HOST side needs.
//!
//! Field numbers and message shapes are protocol FACTS (verified against a
//! real `rustdesk-server` and reverse-engineered from `@verevon/remote-core`,
//! which independently verified them from the controller side) — this file
//! contains no `.proto` schema text and no code copied from any `RustDesk`
//! repository. See `apps/Frontend Plane/verevonv3/packages/remote-core/docs/
//! rustdesk-protocol.md` for the underlying research.

use va_wire::{ProtoReader, ProtoWriter, WireError};

/// `Message` envelope field numbers (the peer-to-peer session protocol,
/// post-handshake) — only the ones the host actually sends or receives.
pub mod message_field {
    pub const SIGNED_ID: u32 = 3;
    pub const PUBLIC_KEY: u32 = 4;
    pub const TEST_DELAY: u32 = 5;
    pub const VIDEO_FRAME: u32 = 6;
    pub const LOGIN_REQUEST: u32 = 7;
    pub const LOGIN_RESPONSE: u32 = 8;
    pub const HASH: u32 = 9;
    pub const MISC: u32 = 19;
    pub const AUTH_2FA: u32 = 27;
}

/// `RendezvousMessage` envelope field numbers (hbbs/hbbr registration and
/// relay negotiation) — only the ones the host actually sends or receives.
/// All four field numbers here are citation-backed against `hbb_common`'s
/// `protos/rendezvous.proto` at commit
/// `69cea8dafee147848ae88702029f4bf7df7224c3` (the exact submodule pin of
/// `rustdesk-server` commit `a7736be5e40f85bfc141120dce587e836e5d4b80`, the
/// same commit already cited elsewhere in this file and in `remote-core`'s
/// own research). `REGISTER_PK_RESPONSE = 16` additionally cross-checks the
/// live wire capture already on record in `remote-core`'s
/// `docs/rustdesk-protocol.md` ("Verified against a live hbbs"): an empty
/// reply `82 01 00` decodes to exactly this field, wire-type 2, length 0
/// (proto3 default `OK`).
///
/// `REGISTER_PEER`/`REGISTER_PEER_RESPONSE` were added after a LIVE
/// discovery, not just a source read: an M2a host that only ever sent
/// `RegisterPk` got `OK` on every attempt, yet a real controller's
/// `PunchHoleRequest` for it still came back "offline". Reading
/// `rendezvous_server.rs`'s `update_addr` (only ever invoked from the
/// `RegisterPeer` handler, never from `RegisterPk`'s) showed why: it is
/// `RegisterPeer`, not `RegisterPk`, that refreshes a peer's `last_reg_time`
/// — the exact value `handle_punch_hole_request` checks against its 30s
/// offline threshold. `RegisterPk` alone establishes the id/pk association;
/// `RegisterPeer` is the actual online keepalive a real client resends
/// periodically, with `RegisterPeerResponse.request_pk` telling it to
/// (re)send `RegisterPk` when the server doesn't have one on file yet.
pub mod rendezvous_field {
    pub const REGISTER_PEER: u32 = 6;
    pub const REGISTER_PEER_RESPONSE: u32 = 7;
    /// Live-captured, not just source-cited: a real hbbs (with
    /// `ALWAYS_USE_RELAY=Y`, Support Plane's own setting) forwarded exactly
    /// this field number to a registered host once a controller sent
    /// `PunchHoleRequest` for it — `4a0e0a0aa9f64a3684a5a26d84a71802` decodes
    /// to tag `0x4a` = field 9, wire-type 2, matching `hbb_common`'s schema.
    pub const PUNCH_HOLE: u32 = 9;
    pub const REGISTER_PK: u32 = 15;
    pub const REGISTER_PK_RESPONSE: u32 = 16;
    pub const REQUEST_RELAY: u32 = 18;
    pub const RELAY_RESPONSE: u32 = 19;
}

/// Wraps `payload` (an already-encoded sub-message) as one field of the
/// `Message` oneof envelope — e.g. `wrap_message(message_field::SIGNED_ID,
/// &encode_signed_id(...))` produces the full `Message{signed_id: ...}`
/// envelope bytes ready to send (still needing encryption, for every field
/// except `SIGNED_ID` itself and the reply `PUBLIC_KEY`, which are plaintext).
pub fn wrap_message(field_number: u32, payload: &[u8]) -> Vec<u8> {
    let mut w = ProtoWriter::new();
    w.message(field_number, payload);
    w.finish()
}

/// Reads exactly one top-level field from a `Message` (or `RendezvousMessage`)
/// envelope — `RustDesk`'s `oneof` means exactly one is ever set, so callers
/// `match` on `field_number` and pass `payload` to the matching decoder.
/// Returns `None` for a genuinely empty envelope (0 bytes).
pub fn read_envelope_field(data: &[u8]) -> Result<Option<(u32, &[u8])>, WireError> {
    let mut r = ProtoReader::new(data);
    if r.eof() {
        return Ok(None);
    }
    let tag = r.read_tag()?;
    let payload = r.read_length_delimited()?;
    Ok(Some((tag.field_number, payload)))
}

// =====================================================================
// SignedId / IdPk — the host sends SignedId first, in plaintext.
// =====================================================================

/// `IdPk{id: string, pk: bytes}` — the payload the host signs with its
/// identity secret key to produce `SignedId.id`.
pub struct IdPk {
    pub id: String,
    pub pk: Vec<u8>,
}

pub fn encode_id_pk(value: &IdPk) -> Vec<u8> {
    let mut w = ProtoWriter::new();
    w.string(1, &value.id).bytes(2, &value.pk);
    w.finish()
}

/// Decodes an `IdPk` (needed by `va-testkit`'s `FakeController`, which
/// plays the controller role and must recover the host's ephemeral public
/// key from the signed payload it receives).
pub fn decode_id_pk(data: &[u8]) -> Result<IdPk, WireError> {
    let mut r = ProtoReader::new(data);
    let mut value = IdPk { id: String::new(), pk: Vec::new() };
    while !r.eof() {
        let tag = r.read_tag()?;
        match tag.field_number {
            1 => value.id = r.read_string()?,
            2 => value.pk = r.read_length_delimited()?.to_vec(),
            _ => r.skip(tag.wire_type)?,
        }
    }
    Ok(value)
}

/// `SignedId{id: bytes}` where `id` is `signature(64) || IdPk-encoded-bytes`.
pub fn encode_signed_id(attached_signature: &[u8]) -> Vec<u8> {
    let mut w = ProtoWriter::new();
    w.bytes(1, attached_signature);
    w.finish()
}

pub fn decode_signed_id(data: &[u8]) -> Result<Vec<u8>, WireError> {
    let mut r = ProtoReader::new(data);
    let mut id = Vec::new();
    while !r.eof() {
        let tag = r.read_tag()?;
        if tag.field_number == 1 {
            id = r.read_length_delimited()?.to_vec();
        } else {
            r.skip(tag.wire_type)?;
        }
    }
    Ok(id)
}

// =====================================================================
// PublicKey — sent and received both directions during the handshake.
// =====================================================================

pub struct PublicKey {
    pub asymmetric_value: Vec<u8>,
    pub symmetric_value: Vec<u8>,
}

pub fn encode_public_key(value: &PublicKey) -> Vec<u8> {
    let mut w = ProtoWriter::new();
    w.bytes(1, &value.asymmetric_value)
        .bytes(2, &value.symmetric_value);
    w.finish()
}

pub fn decode_public_key(data: &[u8]) -> Result<PublicKey, WireError> {
    let mut r = ProtoReader::new(data);
    let mut asymmetric_value = Vec::new();
    let mut symmetric_value = Vec::new();
    while !r.eof() {
        let tag = r.read_tag()?;
        match tag.field_number {
            1 => asymmetric_value = r.read_length_delimited()?.to_vec(),
            2 => symmetric_value = r.read_length_delimited()?.to_vec(),
            _ => r.skip(tag.wire_type)?,
        }
    }
    Ok(PublicKey {
        asymmetric_value,
        symmetric_value,
    })
}

// =====================================================================
// Hash — the host sends this unprompted, immediately after the handshake.
// =====================================================================

pub struct Hash {
    pub salt: String,
    pub challenge: String,
}

pub fn encode_hash(value: &Hash) -> Vec<u8> {
    let mut w = ProtoWriter::new();
    w.string(1, &value.salt).string(2, &value.challenge);
    w.finish()
}

// =====================================================================
// LoginRequest — the host receives this. `encode_login_request` exists only
// for `va-testkit`'s `FakeController`, which plays the controller role and
// so must SEND one — the reverse of what `HostSession` itself ever does.
// =====================================================================

pub struct LoginRequest {
    pub username: String,
    pub password: Vec<u8>,
    pub my_id: String,
    pub my_name: String,
    pub video_ack_required: bool,
    pub session_id: u64,
    pub version: String,
    pub my_platform: String,
    pub hwid: Vec<u8>,
}

pub fn encode_login_request(value: &LoginRequest) -> Vec<u8> {
    let mut w = ProtoWriter::new();
    w.string(1, &value.username)
        .bytes(2, &value.password)
        .string(4, &value.my_id)
        .string(5, &value.my_name)
        .bool(9, value.video_ack_required)
        .uint64(10, value.session_id)
        .string(11, &value.version)
        .string(13, &value.my_platform)
        .bytes(14, &value.hwid);
    w.finish()
}

pub fn decode_login_request(data: &[u8]) -> Result<LoginRequest, WireError> {
    let mut r = ProtoReader::new(data);
    let mut value = LoginRequest {
        username: String::new(),
        password: Vec::new(),
        my_id: String::new(),
        my_name: String::new(),
        video_ack_required: false,
        session_id: 0,
        version: String::new(),
        my_platform: String::new(),
        hwid: Vec::new(),
    };
    while !r.eof() {
        let tag = r.read_tag()?;
        match tag.field_number {
            1 => value.username = r.read_string()?,
            2 => value.password = r.read_length_delimited()?.to_vec(),
            4 => value.my_id = r.read_string()?,
            5 => value.my_name = r.read_string()?,
            9 => value.video_ack_required = r.read_bool()?,
            10 => value.session_id = r.read_uint64()?,
            11 => value.version = r.read_string()?,
            13 => value.my_platform = r.read_string()?,
            14 => value.hwid = r.read_length_delimited()?.to_vec(),
            _ => r.skip(tag.wire_type)?,
        }
    }
    Ok(value)
}

// =====================================================================
// Resolution / DisplayInfo / PeerInfo / LoginResponse — the host sends
// LoginResponse (containing PeerInfo on success, or a fixed-vocabulary
// error string).
// =====================================================================

#[derive(Clone, Copy)]
pub struct Resolution {
    pub width: i32,
    pub height: i32,
}

fn encode_resolution(value: Resolution) -> Vec<u8> {
    let mut w = ProtoWriter::new();
    w.int32(1, value.width).int32(2, value.height);
    w.finish()
}

pub struct DisplayInfo {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub name: String,
    pub online: bool,
    pub cursor_embedded: bool,
    pub original_resolution: Option<Resolution>,
    pub scale: f64,
}

fn encode_display_info(value: &DisplayInfo) -> Vec<u8> {
    let mut w = ProtoWriter::new();
    w.sint32(1, value.x)
        .sint32(2, value.y)
        .int32(3, value.width)
        .int32(4, value.height)
        .string(5, &value.name)
        .bool(6, value.online)
        .bool(7, value.cursor_embedded);
    if let Some(res) = value.original_resolution {
        w.message(8, &encode_resolution(res));
    }
    w.double(9, value.scale);
    w.finish()
}

pub struct PeerInfo {
    pub username: String,
    pub hostname: String,
    pub platform: String,
    pub displays: Vec<DisplayInfo>,
    pub current_display: i32,
    pub sas_enabled: bool,
    pub version: String,
}

fn encode_peer_info(value: &PeerInfo) -> Vec<u8> {
    let mut w = ProtoWriter::new();
    w.string(1, &value.username)
        .string(2, &value.hostname)
        .string(3, &value.platform);
    for display in &value.displays {
        w.message(4, &encode_display_info(display));
    }
    w.int32(5, value.current_display)
        .bool(6, value.sas_enabled)
        .string(7, &value.version);
    w.finish()
}

pub enum LoginResponse {
    /// Fixed vocabulary on the wire: "Empty Password", "Wrong Password",
    /// "Wrong 2FA Code", "2FA Required", "No Password Access", "Offline".
    Error(String),
    Success(PeerInfo),
}

pub fn encode_login_response(value: &LoginResponse) -> Vec<u8> {
    let mut w = ProtoWriter::new();
    match value {
        LoginResponse::Error(message) => {
            w.string(1, message);
        }
        LoginResponse::Success(peer_info) => {
            w.message(2, &encode_peer_info(peer_info));
        }
    }
    w.finish()
}

// The following decode_* functions mirror the encode_* ones above but are
// needed only by `va-testkit`'s `FakeController`, which plays the
// CONTROLLER role in tests and so must read what a real host sends
// (`LoginResponse`/`PeerInfo`/`DisplayInfo`/`Resolution`), the reverse of
// what `HostSession` itself ever needs to decode.

fn decode_resolution(data: &[u8]) -> Result<Resolution, WireError> {
    let mut r = ProtoReader::new(data);
    let mut value = Resolution { width: 0, height: 0 };
    while !r.eof() {
        let tag = r.read_tag()?;
        match tag.field_number {
            1 => value.width = r.read_int32()?,
            2 => value.height = r.read_int32()?,
            _ => r.skip(tag.wire_type)?,
        }
    }
    Ok(value)
}

fn decode_display_info(data: &[u8]) -> Result<DisplayInfo, WireError> {
    let mut r = ProtoReader::new(data);
    let mut value = DisplayInfo {
        x: 0,
        y: 0,
        width: 0,
        height: 0,
        name: String::new(),
        online: false,
        cursor_embedded: false,
        original_resolution: None,
        scale: 0.0,
    };
    while !r.eof() {
        let tag = r.read_tag()?;
        match tag.field_number {
            1 => value.x = r.read_sint32()?,
            2 => value.y = r.read_sint32()?,
            3 => value.width = r.read_int32()?,
            4 => value.height = r.read_int32()?,
            5 => value.name = r.read_string()?,
            6 => value.online = r.read_bool()?,
            7 => value.cursor_embedded = r.read_bool()?,
            8 => value.original_resolution = Some(decode_resolution(r.read_length_delimited()?)?),
            9 => value.scale = r.read_double()?,
            _ => r.skip(tag.wire_type)?,
        }
    }
    Ok(value)
}

fn decode_peer_info(data: &[u8]) -> Result<PeerInfo, WireError> {
    let mut r = ProtoReader::new(data);
    let mut value = PeerInfo {
        username: String::new(),
        hostname: String::new(),
        platform: String::new(),
        displays: Vec::new(),
        current_display: 0,
        sas_enabled: false,
        version: String::new(),
    };
    while !r.eof() {
        let tag = r.read_tag()?;
        match tag.field_number {
            1 => value.username = r.read_string()?,
            2 => value.hostname = r.read_string()?,
            3 => value.platform = r.read_string()?,
            4 => value.displays.push(decode_display_info(r.read_length_delimited()?)?),
            5 => value.current_display = r.read_int32()?,
            6 => value.sas_enabled = r.read_bool()?,
            7 => value.version = r.read_string()?,
            _ => r.skip(tag.wire_type)?,
        }
    }
    Ok(value)
}

pub fn decode_login_response(data: &[u8]) -> Result<LoginResponse, WireError> {
    let mut r = ProtoReader::new(data);
    let mut result = LoginResponse::Error(String::new());
    while !r.eof() {
        let tag = r.read_tag()?;
        match tag.field_number {
            1 => result = LoginResponse::Error(r.read_string()?),
            2 => result = LoginResponse::Success(decode_peer_info(r.read_length_delimited()?)?),
            _ => r.skip(tag.wire_type)?,
        }
    }
    Ok(result)
}

// =====================================================================
// TestDelay — the host sends a probe roughly every second and echoes any
// client-originated probe back verbatim.
// =====================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TestDelay {
    pub time: i64,
    pub from_client: bool,
    pub last_delay: u32,
    pub target_bitrate: u32,
}

pub fn encode_test_delay(value: TestDelay) -> Vec<u8> {
    let mut w = ProtoWriter::new();
    w.int64(1, value.time)
        .bool(2, value.from_client)
        .uint32(3, value.last_delay)
        .uint32(4, value.target_bitrate);
    w.finish()
}

pub fn decode_test_delay(data: &[u8]) -> Result<TestDelay, WireError> {
    let mut r = ProtoReader::new(data);
    let mut value = TestDelay {
        time: 0,
        from_client: false,
        last_delay: 0,
        target_bitrate: 0,
    };
    while !r.eof() {
        let tag = r.read_tag()?;
        match tag.field_number {
            1 => value.time = r.read_int64()?,
            2 => value.from_client = r.read_bool()?,
            3 => value.last_delay = r.read_uint32()?,
            4 => value.target_bitrate = r.read_uint32()?,
            _ => r.skip(tag.wire_type)?,
        }
    }
    Ok(value)
}

// =====================================================================
// VideoFrame — the host sends encoded video chunks, batched. `av1s` (field
// 13) is what this host actually sends (see `va-encode`'s crate docs for
// why AV1 over VP9); `vp9s` (field 6) is kept as a citation-backed,
// tested-but-currently-unused alternative in case a future build ever
// wants it, matching this file's file-level policy of only ever encoding
// wire shapes it has verified, not guessing at ones it doesn't use yet.
// Both field numbers and `EncodedVideoFrame`/`EncodedVideoFrames`'s shape
// are citation-backed against `hbb_common`'s `protos/message.proto` at
// commit `69cea8dafee147848ae88702029f4bf7df7224c3`.
// =====================================================================

pub struct EncodedVideoFrame {
    pub data: Vec<u8>,
    pub key: bool,
    pub pts: i64,
}

fn encode_encoded_video_frame(value: &EncodedVideoFrame) -> Vec<u8> {
    let mut w = ProtoWriter::new();
    w.bytes(1, &value.data).bool(2, value.key).int64(3, value.pts);
    w.finish()
}

fn encode_encoded_video_frames(frames: &[EncodedVideoFrame]) -> Vec<u8> {
    let mut inner = ProtoWriter::new();
    for frame in frames {
        inner.message(1, &encode_encoded_video_frame(frame));
    }
    inner.finish()
}

/// `VideoFrame.vp9s` (oneof field 6 within `VideoFrame` — VP9 is the one
/// codec every client is guaranteed to accept). Batches one or more encoded
/// chunks into a single `EncodedVideoFrames` wire message; a single encoded
/// frame is never itself split across multiple wire messages.
pub fn encode_vp9_video_frame(frames: &[EncodedVideoFrame], display: i32) -> Vec<u8> {
    let mut w = ProtoWriter::new();
    w.message(6, &encode_encoded_video_frames(frames));
    w.int32(14, display);
    w.finish()
}

/// `VideoFrame.av1s` (oneof field 13 within `VideoFrame`) — what this host
/// actually sends. Same `EncodedVideoFrames` wrapper shape as `vp9s`; only
/// the oneof field number differs.
pub fn encode_av1_video_frame(frames: &[EncodedVideoFrame], display: i32) -> Vec<u8> {
    let mut w = ProtoWriter::new();
    w.message(13, &encode_encoded_video_frames(frames));
    w.int32(14, display);
    w.finish()
}

// =====================================================================
// Misc / PermissionInfo — RustDesk gates BOTH mouse and keyboard behind a
// single Keyboard permission flag; there is no separate mouse permission
// on the wire.
// =====================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Permission {
    Keyboard,
    Clipboard,
    Audio,
    File,
}

impl Permission {
    fn wire_value(self) -> i32 {
        match self {
            Permission::Keyboard => 0,
            Permission::Clipboard => 2,
            Permission::Audio => 3,
            Permission::File => 4,
        }
    }
}

pub fn encode_permission_info(permission: Permission, enabled: bool) -> Vec<u8> {
    let mut inner = ProtoWriter::new();
    inner.int32(1, permission.wire_value()).bool(2, enabled);
    let mut w = ProtoWriter::new();
    w.message(6, &inner.finish());
    w.finish()
}

/// `Misc.switch_display` — the host's confirmation of a display switch, sent
/// with full geometry (unlike the client's request, which carries only the
/// target index).
pub struct SwitchDisplay {
    pub display: i32,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub cursor_embedded: bool,
}

pub fn encode_switch_display(value: &SwitchDisplay) -> Vec<u8> {
    let mut inner = ProtoWriter::new();
    inner
        .int32(1, value.display)
        .sint32(2, value.x)
        .sint32(3, value.y)
        .int32(4, value.width)
        .int32(5, value.height)
        .bool(6, value.cursor_embedded);
    let mut w = ProtoWriter::new();
    w.message(5, &inner.finish());
    w.finish()
}

// =====================================================================
// RegisterPeer / RegisterPeerResponse — the actual online-keepalive hbbs
// checks a peer's `last_reg_time` against (see the module doc above for how
// this was discovered). `va-net` sends this every `REGISTRATION_INTERVAL`;
// `RegisterPk` alone does NOT keep a peer discoverable for punch-hole
// requests.
// =====================================================================

pub struct RegisterPeer {
    pub id: String,
    /// The rendezvous-server-list config version this device has already
    /// applied. `0` (never applied any) is the correct value for a device
    /// that ignores `ConfigUpdate` pushes entirely, which `va-net` does —
    /// Verevon's own Support Plane config is fixed, not server-pushed.
    pub serial: i32,
}

pub fn encode_register_peer(value: &RegisterPeer) -> Vec<u8> {
    let mut w = ProtoWriter::new();
    w.string(1, &value.id).int32(2, value.serial);
    w.finish()
}

pub struct RegisterPeerResponse {
    /// `true` means hbbs has no `pk` on file for this id yet (or this
    /// registration's source IP changed) — the caller must send
    /// `RegisterPk` again before this `RegisterPeer` keepalive actually
    /// refreshes `last_reg_time` server-side.
    pub request_pk: bool,
}

pub fn decode_register_peer_response(data: &[u8]) -> Result<RegisterPeerResponse, WireError> {
    let mut r = ProtoReader::new(data);
    let mut value = RegisterPeerResponse { request_pk: false };
    while !r.eof() {
        let tag = r.read_tag()?;
        if tag.field_number == 2 {
            value.request_pk = r.read_bool()?;
        } else {
            r.skip(tag.wire_type)?;
        }
    }
    Ok(value)
}

// =====================================================================
// RegisterPk / RegisterPkResponse — establishes the id<->pk association
// (UDP only; sending this over TCP/WS gets an unconditional NOT_SUPPORT
// from a real hbbs, per the same research that closed this field's
// numbers). Does NOT by itself keep a peer online for punch-hole routing —
// see `RegisterPeer` above.
// =====================================================================

/// `RegisterPk{id, uuid, pk, old_id, no_register_device}`. `pk` is the same
/// persistent Ed25519 public key later reused to sign `SignedId` — a real
/// client never uses a separate key for registration vs. the session
/// handshake, confirmed against its own source. `uuid` only needs to be a
/// stable, non-empty per-device value (a real client prefers the OS machine
/// ID, falling back to its own `pk` bytes) — hbbs never interprets it beyond
/// consistency-checking it against whatever was stored on a prior
/// registration of the same `id`.
pub struct RegisterPk {
    pub id: String,
    pub uuid: Vec<u8>,
    pub pk: Vec<u8>,
}

pub fn encode_register_pk(value: &RegisterPk) -> Vec<u8> {
    let mut w = ProtoWriter::new();
    w.string(1, &value.id).bytes(2, &value.uuid).bytes(3, &value.pk);
    // `old_id` (field 4) and `no_register_device` (field 5) are left at
    // their proto3 defaults (empty string / false) — this device never
    // renames itself and always wants to be discoverable.
    w.finish()
}

/// `RegisterPkResponse.result`'s wire values — the nested `Result` enum from
/// `rendezvous.proto`. `Ok` is proto3's zero default, so a real hbbs's
/// success reply omits the field entirely (the live-captured `82 01 00` has
/// no inner bytes at all); every other value is an explicit rejection this
/// device should surface, not silently retry forever on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegisterPkResult {
    Ok,
    UuidMismatch,
    IdExists,
    TooFrequent,
    InvalidIdFormat,
    NotSupport,
    ServerError,
    NotDeployed,
    /// A value this catalog doesn't recognize — forward-compatible with a
    /// future server build adding new variants, rather than erroring.
    Unknown(i32),
}

impl RegisterPkResult {
    fn from_wire(value: i32) -> Self {
        match value {
            0 => Self::Ok,
            2 => Self::UuidMismatch,
            3 => Self::IdExists,
            4 => Self::TooFrequent,
            5 => Self::InvalidIdFormat,
            6 => Self::NotSupport,
            7 => Self::ServerError,
            8 => Self::NotDeployed,
            other => Self::Unknown(other),
        }
    }
}

pub struct RegisterPkResponse {
    pub result: RegisterPkResult,
    /// Always 0 in the reference server build this was verified against —
    /// never populated by any reply path it constructs. Kept for
    /// forward-compatibility, not currently acted on by `va-net`.
    pub keep_alive: i32,
}

pub fn decode_register_pk_response(data: &[u8]) -> Result<RegisterPkResponse, WireError> {
    let mut r = ProtoReader::new(data);
    let mut value = RegisterPkResponse { result: RegisterPkResult::Ok, keep_alive: 0 };
    while !r.eof() {
        let tag = r.read_tag()?;
        match tag.field_number {
            1 => value.result = RegisterPkResult::from_wire(r.read_int32()?),
            2 => value.keep_alive = r.read_int32()?,
            _ => r.skip(tag.wire_type)?,
        }
    }
    Ok(value)
}

// =====================================================================
// PunchHole / RelayResponse / RequestRelay — the relay-pairing sequence
// hbbs/hbbr use to route a controller's connection to this host once
// `PunchHoleRequest` finds it. Field numbers and shapes are citation-backed
// against `hbb_common`'s `protos/rendezvous.proto` at commit
// `69cea8dafee147848ae88702029f4bf7df7224c3`; the sequence itself (which
// message goes to which server, in which order, and that the second TCP
// connection's stream becomes the raw peer-session channel afterward) is
// read directly from `rustdesk`'s own `src/rendezvous_mediator.rs`
// (`handle_punch_hole`/`create_relay`) and `src/server.rs`
// (`create_relay_connection_`) at commit
// `1ec1b9e7e3808b24bfc923ee1e6800ab9694e9a5`. See `docs/architecture.md`
// for the full walkthrough and the live capture that triggered this
// research.
// =====================================================================

/// What hbbs forwards to a registered host once a controller's
/// `PunchHoleRequest` finds it — only the fields Verevon's own
/// `ALWAYS_USE_RELAY=Y` deployment ever needs. `socket_addr` is an
/// `AddrMangle`-obfuscated encoding of the controller's address; this host
/// never decodes it, only passes it straight through into `RelayResponse`.
pub struct PunchHole {
    pub socket_addr: Vec<u8>,
    /// `2` (`SYMMETRIC`) is the only value observed from a real hbbs with
    /// `ALWAYS_USE_RELAY=Y` set — the server forces it specifically to
    /// signal "go straight to relay, don't attempt a direct hole-punch".
    /// Every other value is a real, distinct `NatType`, kept as the raw
    /// wire int rather than modeled as an enum since this host only ever
    /// acts on the relay case.
    pub nat_type: i32,
}

pub fn decode_punch_hole(data: &[u8]) -> Result<PunchHole, WireError> {
    let mut r = ProtoReader::new(data);
    let mut value = PunchHole { socket_addr: Vec::new(), nat_type: 0 };
    while !r.eof() {
        let tag = r.read_tag()?;
        match tag.field_number {
            1 => value.socket_addr = r.read_length_delimited()?.to_vec(),
            3 => value.nat_type = r.read_int32()?,
            _ => r.skip(tag.wire_type)?,
        }
    }
    Ok(value)
}

/// The wire value of `PunchHole.nat_type` that means "go straight to
/// relay" — see [`PunchHole::nat_type`]'s doc for why this is kept as a raw
/// int rather than a full enum.
pub const NAT_TYPE_SYMMETRIC: i32 = 2;

/// `RelayResponse{socket_addr, uuid, relay_server, id, version}` — sent to
/// hbbs (NOT hbbr) over a fresh, short-lived TCP connection in reply to a
/// `PunchHole` whose `nat_type` was [`NAT_TYPE_SYMMETRIC`]. The real client
/// never reads a reply on this connection or reuses it afterward; hbbs
/// forwards this message's contents on to the original controller.
pub struct RelayResponse {
    /// Passed straight through from the triggering [`PunchHole`], unchanged
    /// — this is what lets hbbs correlate the reply back to the
    /// controller's original request.
    pub socket_addr: Vec<u8>,
    pub uuid: String,
    pub relay_server: String,
    pub id: String,
    pub version: String,
}

pub fn encode_relay_response(value: &RelayResponse) -> Vec<u8> {
    let mut w = ProtoWriter::new();
    w.bytes(1, &value.socket_addr)
        .string(2, &value.uuid)
        .string(3, &value.relay_server)
        .string(4, &value.id)
        .string(7, &value.version);
    w.finish()
}

/// `RequestRelay{uuid, licence_key}` — sent as the FIRST message on a
/// SEPARATE new TCP connection, this one to hbbr (not hbbs), identifying
/// which pending relay pairing this connection continues. Every field this
/// host doesn't set (`id`, `socket_addr`, `relay_server`, `secure`,
/// `conn_type`, `token`) is left at its proto3 default, matching what the
/// real client sends here (`RequestRelay{licence_key, uuid,
/// ..Default::default()}`) — those fields matter for the CONTROLLER's own
/// `RequestRelay` (a different call site sending the same message type),
/// not for a host answering a `PunchHole`.
pub struct RequestRelay {
    pub uuid: String,
    pub licence_key: String,
}

pub fn encode_request_relay(value: &RequestRelay) -> Vec<u8> {
    let mut w = ProtoWriter::new();
    w.string(2, &value.uuid).string(6, &value.licence_key);
    w.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_round_trips_through_wire_writer() {
        // Hash is host-send-only in this crate (no decode needed), so this
        // just pins the encode shape by decoding with a bare ProtoReader.
        let encoded = encode_hash(&Hash {
            salt: "device-salt".into(),
            challenge: "fresh-challenge".into(),
        });
        let mut r = ProtoReader::new(&encoded);
        let tag = r.read_tag().unwrap();
        assert_eq!(tag.field_number, 1);
        assert_eq!(r.read_string().unwrap(), "device-salt");
        let tag = r.read_tag().unwrap();
        assert_eq!(tag.field_number, 2);
        assert_eq!(r.read_string().unwrap(), "fresh-challenge");
    }

    #[test]
    fn public_key_round_trips() {
        let value = PublicKey {
            asymmetric_value: vec![1, 2, 3],
            symmetric_value: vec![4, 5, 6, 7],
        };
        let encoded = encode_public_key(&value);
        let decoded = decode_public_key(&encoded).unwrap();
        assert_eq!(decoded.asymmetric_value, value.asymmetric_value);
        assert_eq!(decoded.symmetric_value, value.symmetric_value);
    }

    #[test]
    fn signed_id_round_trips() {
        let sig = vec![9u8; 64 + 10];
        let encoded = encode_signed_id(&sig);
        let decoded = decode_signed_id(&encoded).unwrap();
        assert_eq!(decoded, sig);
    }

    #[test]
    fn envelope_wraps_and_reads_back_the_right_field() {
        let payload = encode_hash(&Hash { salt: "s".into(), challenge: "c".into() });
        let wrapped = wrap_message(message_field::HASH, &payload);
        let (field_number, read_payload) = read_envelope_field(&wrapped).unwrap().unwrap();
        assert_eq!(field_number, message_field::HASH);
        assert_eq!(read_payload, payload.as_slice());
    }

    #[test]
    fn envelope_read_of_empty_buffer_is_none() {
        assert!(read_envelope_field(&[]).unwrap().is_none());
    }

    #[test]
    fn id_pk_encodes_id_and_pk() {
        let encoded = encode_id_pk(&IdPk {
            id: "device-123".into(),
            pk: vec![1, 2, 3, 4],
        });
        let mut r = ProtoReader::new(&encoded);
        let tag = r.read_tag().unwrap();
        assert_eq!(tag.field_number, 1);
        assert_eq!(r.read_string().unwrap(), "device-123");
        let tag = r.read_tag().unwrap();
        assert_eq!(tag.field_number, 2);
        assert_eq!(r.read_length_delimited().unwrap(), &[1, 2, 3, 4]);
    }

    #[test]
    fn login_request_decodes_all_fields() {
        let mut w = ProtoWriter::new();
        w.string(1, "141589119")
            .bytes(2, &[0xAA; 32])
            .string(4, "controller-id")
            .string(5, "Verevon Support")
            .bool(9, true)
            .uint64(10, 42)
            .string(11, "1.4.9")
            .string(13, "Web")
            .bytes(14, &[]);
        let decoded = decode_login_request(&w.finish()).unwrap();
        assert_eq!(decoded.username, "141589119");
        assert_eq!(decoded.password, vec![0xAA; 32]);
        assert_eq!(decoded.my_id, "controller-id");
        assert_eq!(decoded.my_name, "Verevon Support");
        assert!(decoded.video_ack_required);
        assert_eq!(decoded.session_id, 42);
        assert_eq!(decoded.version, "1.4.9");
        assert_eq!(decoded.my_platform, "Web");
    }

    #[test]
    fn login_response_error_encodes_the_message_string() {
        let encoded = encode_login_response(&LoginResponse::Error("2FA Required".into()));
        let mut r = ProtoReader::new(&encoded);
        let tag = r.read_tag().unwrap();
        assert_eq!(tag.field_number, 1);
        assert_eq!(r.read_string().unwrap(), "2FA Required");
    }

    #[test]
    fn login_response_success_embeds_peer_info_with_displays() {
        let encoded = encode_login_response(&LoginResponse::Success(PeerInfo {
            username: "agent".into(),
            hostname: "DESKTOP-1".into(),
            platform: "Windows".into(),
            displays: vec![DisplayInfo {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
                name: "Display1".into(),
                online: true,
                cursor_embedded: false,
                original_resolution: Some(Resolution { width: 1920, height: 1080 }),
                scale: 1.0,
            }],
            current_display: 0,
            sas_enabled: false,
            version: "0.1.0".into(),
        }));
        let mut r = ProtoReader::new(&encoded);
        let tag = r.read_tag().unwrap();
        assert_eq!(tag.field_number, 2);
        let peer_info_bytes = r.read_length_delimited().unwrap();

        let mut pr = ProtoReader::new(peer_info_bytes);
        assert_eq!(pr.read_tag().unwrap().field_number, 1);
        assert_eq!(pr.read_string().unwrap(), "agent");
        assert_eq!(pr.read_tag().unwrap().field_number, 2);
        assert_eq!(pr.read_string().unwrap(), "DESKTOP-1");
        assert_eq!(pr.read_tag().unwrap().field_number, 3);
        assert_eq!(pr.read_string().unwrap(), "Windows");
        assert_eq!(pr.read_tag().unwrap().field_number, 4);
        let display_bytes = pr.read_length_delimited().unwrap();
        let mut dr = ProtoReader::new(display_bytes);
        assert_eq!(dr.read_tag().unwrap().field_number, 3);
        assert_eq!(dr.read_int32().unwrap(), 1920);
    }

    #[test]
    fn test_delay_round_trips() {
        let value = TestDelay {
            time: 123_456,
            from_client: false,
            last_delay: 42,
            target_bitrate: 2000,
        };
        let encoded = encode_test_delay(value);
        let decoded = decode_test_delay(&encoded).unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn vp9_video_frame_batches_multiple_chunks_under_one_wire_message() {
        let frames = vec![
            EncodedVideoFrame { data: vec![1, 2, 3], key: true, pts: 0 },
            EncodedVideoFrame { data: vec![4, 5], key: false, pts: 33 },
        ];
        let encoded = encode_vp9_video_frame(&frames, 0);
        let mut r = ProtoReader::new(&encoded);
        let tag = r.read_tag().unwrap();
        assert_eq!(tag.field_number, 6, "vp9s oneof field");
        let batch_bytes = r.read_length_delimited().unwrap();

        let mut br = ProtoReader::new(batch_bytes);
        let mut chunk_count = 0;
        while !br.eof() {
            let tag = br.read_tag().unwrap();
            assert_eq!(tag.field_number, 1);
            let frame_bytes = br.read_length_delimited().unwrap();
            let mut fr = ProtoReader::new(frame_bytes);
            let ftag = fr.read_tag().unwrap();
            assert_eq!(ftag.field_number, 1);
            let _ = fr.read_length_delimited().unwrap();
            chunk_count += 1;
        }
        assert_eq!(chunk_count, 2);
    }

    #[test]
    fn av1_video_frame_uses_field_thirteen_with_the_same_batch_shape_as_vp9() {
        let frames = vec![EncodedVideoFrame { data: vec![9, 8, 7], key: true, pts: 100 }];
        let encoded = encode_av1_video_frame(&frames, 2);
        let mut r = ProtoReader::new(&encoded);
        let tag = r.read_tag().unwrap();
        assert_eq!(tag.field_number, 13, "av1s oneof field");
        let batch_bytes = r.read_length_delimited().unwrap();

        let mut br = ProtoReader::new(batch_bytes);
        let tag = br.read_tag().unwrap();
        assert_eq!(tag.field_number, 1);
        let frame_bytes = br.read_length_delimited().unwrap();
        let mut fr = ProtoReader::new(frame_bytes);
        assert_eq!(fr.read_tag().unwrap().field_number, 1);
        assert_eq!(fr.read_length_delimited().unwrap(), &[9, 8, 7]);
        assert_eq!(fr.read_tag().unwrap().field_number, 2);
        assert!(fr.read_bool().unwrap());
        assert_eq!(fr.read_tag().unwrap().field_number, 3);
        assert_eq!(fr.read_int64().unwrap(), 100);

        let tag = r.read_tag().unwrap();
        assert_eq!(tag.field_number, 14, "display field");
        assert_eq!(r.read_int32().unwrap(), 2);
    }

    #[test]
    fn permission_info_encodes_keyboard_grant_gating_both_pointer_and_keyboard() {
        // Permission::Keyboard's wire value is 0 (proto3's default), so
        // ProtoWriter correctly OMITS field 1 entirely here — this is not a
        // bug, it's the same proto3 zero-value rule remote-core's own
        // ProtoWriter enforces. A real protobuf decoder treats "field
        // absent" as "field == default", so a receiver seeing no field 1
        // still resolves it as Keyboard. Covered by the Clipboard case
        // below, which has a non-zero wire value and so IS written.
        let encoded = encode_permission_info(Permission::Keyboard, true);
        let mut r = ProtoReader::new(&encoded);
        let tag = r.read_tag().unwrap();
        assert_eq!(tag.field_number, 6, "Misc field");
        let misc_bytes = r.read_length_delimited().unwrap();
        let mut mr = ProtoReader::new(misc_bytes);
        let tag = mr.read_tag().unwrap();
        assert_eq!(tag.field_number, 2, "field 1 (Keyboard=0) is correctly omitted");
        assert!(mr.read_bool().unwrap());
    }

    #[test]
    fn permission_info_writes_the_permission_field_when_its_wire_value_is_nonzero() {
        let encoded = encode_permission_info(Permission::Clipboard, false);
        let mut r = ProtoReader::new(&encoded);
        r.read_tag().unwrap();
        let misc_bytes = r.read_length_delimited().unwrap();
        let mut mr = ProtoReader::new(misc_bytes);
        let tag = mr.read_tag().unwrap();
        assert_eq!(tag.field_number, 1);
        assert_eq!(mr.read_int32().unwrap(), 2, "Clipboard enum value");
        // enabled=false is proto3's default too, so field 2 is also omitted.
        assert!(mr.eof());
    }

    #[test]
    fn switch_display_carries_full_geometry() {
        let encoded = encode_switch_display(&SwitchDisplay {
            display: 1,
            x: -100,
            y: 50,
            width: 2560,
            height: 1440,
            cursor_embedded: false,
        });
        let mut r = ProtoReader::new(&encoded);
        let tag = r.read_tag().unwrap();
        assert_eq!(tag.field_number, 5, "Misc field");
        let misc_bytes = r.read_length_delimited().unwrap();
        let mut mr = ProtoReader::new(misc_bytes);
        assert_eq!(mr.read_tag().unwrap().field_number, 1);
        assert_eq!(mr.read_int32().unwrap(), 1);
        assert_eq!(mr.read_tag().unwrap().field_number, 2);
        assert_eq!(mr.read_sint32().unwrap(), -100);
    }

    #[test]
    fn register_peer_encodes_id_and_serial() {
        let encoded = encode_register_peer(&RegisterPeer { id: "999888777".into(), serial: 0 });
        let mut r = ProtoReader::new(&encoded);
        let tag = r.read_tag().unwrap();
        assert_eq!(tag.field_number, 1);
        assert_eq!(r.read_string().unwrap(), "999888777");
        // serial=0 is proto3's default, so a correct writer omits it.
        assert!(r.eof());
    }

    #[test]
    fn register_peer_response_decodes_request_pk() {
        let mut w = ProtoWriter::new();
        w.bool(2, true);
        let decoded = decode_register_peer_response(&w.finish()).unwrap();
        assert!(decoded.request_pk);
    }

    #[test]
    fn register_peer_response_empty_payload_means_request_pk_is_false() {
        // proto3 default: hbbs already has a pk on file, no re-send needed.
        let decoded = decode_register_peer_response(&[]).unwrap();
        assert!(!decoded.request_pk);
    }

    #[test]
    fn register_pk_encodes_id_uuid_and_pk_and_omits_old_id_and_no_register_device() {
        let encoded = encode_register_pk(&RegisterPk {
            id: "141589119".into(),
            uuid: vec![0xAA; 16],
            pk: vec![0xBB; 32],
        });
        let mut r = ProtoReader::new(&encoded);
        let tag = r.read_tag().unwrap();
        assert_eq!(tag.field_number, 1);
        assert_eq!(r.read_string().unwrap(), "141589119");
        let tag = r.read_tag().unwrap();
        assert_eq!(tag.field_number, 2);
        assert_eq!(r.read_length_delimited().unwrap(), &[0xAA; 16]);
        let tag = r.read_tag().unwrap();
        assert_eq!(tag.field_number, 3);
        assert_eq!(r.read_length_delimited().unwrap(), &[0xBB; 32]);
        // old_id (4) and no_register_device (5) are proto3 defaults, so a
        // correct writer never emits them.
        assert!(r.eof());
    }

    #[test]
    fn register_pk_response_empty_payload_decodes_to_ok() {
        // The exact bytes independently captured from a real hbbs
        // (remote-core's docs/rustdesk-protocol.md, "Verified against a live
        // hbbs"): an empty RegisterPkResponse submessage means result=OK via
        // proto3's zero default, not "unknown"/"absent".
        let decoded = decode_register_pk_response(&[]).unwrap();
        assert_eq!(decoded.result, RegisterPkResult::Ok);
        assert_eq!(decoded.keep_alive, 0);
    }

    #[test]
    fn register_pk_response_decodes_nonzero_result_and_keep_alive() {
        let mut w = ProtoWriter::new();
        w.int32(1, 4).int32(2, 7); // TOO_FREQUENT, keep_alive=7
        let decoded = decode_register_pk_response(&w.finish()).unwrap();
        assert_eq!(decoded.result, RegisterPkResult::TooFrequent);
        assert_eq!(decoded.keep_alive, 7);
    }

    #[test]
    fn register_pk_response_unknown_result_value_is_forward_compatible() {
        let mut w = ProtoWriter::new();
        w.int32(1, 99);
        let decoded = decode_register_pk_response(&w.finish()).unwrap();
        assert_eq!(decoded.result, RegisterPkResult::Unknown(99));
    }

    #[test]
    fn punch_hole_decodes_the_exact_bytes_captured_live_from_a_real_hbbs() {
        // 4a0e0a0aa9f64a3684a5a26d84a71802, minus the envelope's own
        // `4a0e` (field 9, length 14) -- read_envelope_field's job, not
        // this function's -- leaving just the PunchHole submessage bytes.
        let payload = hex_decode("0a0aa9f64a3684a5a26d84a71802");
        let decoded = decode_punch_hole(&payload).unwrap();
        assert_eq!(decoded.socket_addr, hex_decode("a9f64a3684a5a26d84a7"));
        assert_eq!(decoded.nat_type, NAT_TYPE_SYMMETRIC);
    }

    #[test]
    fn envelope_around_the_live_captured_punch_hole_reads_as_field_nine() {
        let raw = hex_decode("4a0e0a0aa9f64a3684a5a26d84a71802");
        let (field_number, payload) = read_envelope_field(&raw).unwrap().unwrap();
        assert_eq!(field_number, rendezvous_field::PUNCH_HOLE);
        let decoded = decode_punch_hole(payload).unwrap();
        assert_eq!(decoded.nat_type, NAT_TYPE_SYMMETRIC);
    }

    #[test]
    fn relay_response_passes_socket_addr_through_unchanged() {
        let encoded = encode_relay_response(&RelayResponse {
            socket_addr: vec![0xaa, 0xbb, 0xcc],
            uuid: "test-uuid".into(),
            relay_server: "127.0.0.1:21117".into(),
            id: "313131313".into(),
            version: "0.1.0".into(),
        });
        let mut r = ProtoReader::new(&encoded);
        let tag = r.read_tag().unwrap();
        assert_eq!(tag.field_number, 1);
        assert_eq!(r.read_length_delimited().unwrap(), &[0xaa, 0xbb, 0xcc]);
        let tag = r.read_tag().unwrap();
        assert_eq!(tag.field_number, 2);
        assert_eq!(r.read_string().unwrap(), "test-uuid");
        let tag = r.read_tag().unwrap();
        assert_eq!(tag.field_number, 3);
        assert_eq!(r.read_string().unwrap(), "127.0.0.1:21117");
        let tag = r.read_tag().unwrap();
        assert_eq!(tag.field_number, 4);
        assert_eq!(r.read_string().unwrap(), "313131313");
        let tag = r.read_tag().unwrap();
        assert_eq!(tag.field_number, 7);
        assert_eq!(r.read_string().unwrap(), "0.1.0");
    }

    #[test]
    fn request_relay_only_sets_uuid_and_licence_key() {
        let encoded = encode_request_relay(&RequestRelay {
            uuid: "test-uuid".into(),
            licence_key: "not-a-real-key-just-a-test-fixture-string".into(),
        });
        let mut r = ProtoReader::new(&encoded);
        let tag = r.read_tag().unwrap();
        assert_eq!(tag.field_number, 2);
        assert_eq!(r.read_string().unwrap(), "test-uuid");
        let tag = r.read_tag().unwrap();
        assert_eq!(tag.field_number, 6);
        assert_eq!(r.read_string().unwrap(), "not-a-real-key-just-a-test-fixture-string");
        assert!(r.eof(), "id/socket_addr/relay_server/secure/conn_type/token must stay at proto3 defaults");
    }

    fn hex_decode(hex: &str) -> Vec<u8> {
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect()
    }
}
