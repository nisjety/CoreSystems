import { ProtoWriter } from '../wire/ProtoWriter.js';
import { encodeOptionMessage } from './controlCodec.js';
import { ProtoReader } from '../wire/ProtoReader.js';
import type {
  RdDisplayInfo,
  RdHash,
  RdIdPk,
  RdLoginRequest,
  RdLoginResponse,
  RdPeerInfo,
  RdPublicKey,
  RdResolution,
  RdSignedId,
} from './types.js';

/**
 * Vi er alltid den tilkoblende (kontrollerende) siden i denne arkitekturen —
 * en nettleser kan ikke registrere seg som en dialbar RustDesk-peer (kun
 * UDP-basert, se docs/rustdesk-protocol.md). Derfor koder vi kun det vi
 * faktisk SENDER (LoginRequest, PublicKey, SignedId) og dekoder kun det vi
 * faktisk MOTTAR (Hash, LoginResponse, PeerInfo). PublicKey/SignedId trengs
 * begge veier siden håndtrykket er gjensidig.
 */

// ---------- Hash (mottas) ----------

export function decodeHash(data: Uint8Array): RdHash {
  const reader = new ProtoReader(data);
  let salt = '';
  let challenge = '';
  while (!reader.eof()) {
    const tag = reader.readTag();
    switch (tag.fieldNumber) {
      case 1:
        salt = reader.readString();
        break;
      case 2:
        challenge = reader.readString();
        break;
      default:
        reader.skip(tag.wireType);
    }
  }
  return { salt, challenge };
}

// ---------- PublicKey (sendes og mottas) ----------

export function encodePublicKey(value: RdPublicKey): Uint8Array {
  return new ProtoWriter().bytes(1, value.asymmetricValue).bytes(2, value.symmetricValue).finish();
}

export function decodePublicKey(data: Uint8Array): RdPublicKey {
  const reader = new ProtoReader(data);
  let asymmetricValue: Uint8Array = new Uint8Array(0);
  let symmetricValue: Uint8Array = new Uint8Array(0);
  while (!reader.eof()) {
    const tag = reader.readTag();
    switch (tag.fieldNumber) {
      case 1:
        asymmetricValue = reader.readLengthDelimited();
        break;
      case 2:
        symmetricValue = reader.readLengthDelimited();
        break;
      default:
        reader.skip(tag.wireType);
    }
  }
  return { asymmetricValue, symmetricValue };
}

// ---------- SignedId / IdPk (sendes og mottas) ----------

export function encodeSignedId(value: RdSignedId): Uint8Array {
  return new ProtoWriter().bytes(1, value.id).finish();
}

export function decodeSignedId(data: Uint8Array): RdSignedId {
  const reader = new ProtoReader(data);
  let id: Uint8Array = new Uint8Array(0);
  while (!reader.eof()) {
    const tag = reader.readTag();
    if (tag.fieldNumber === 1) id = reader.readLengthDelimited();
    else reader.skip(tag.wireType);
  }
  return { id };
}

export function decodeIdPk(data: Uint8Array): RdIdPk {
  const reader = new ProtoReader(data);
  let id = '';
  let pk: Uint8Array = new Uint8Array(0);
  while (!reader.eof()) {
    const tag = reader.readTag();
    switch (tag.fieldNumber) {
      case 1:
        id = reader.readString();
        break;
      case 2:
        pk = reader.readLengthDelimited();
        break;
      default:
        reader.skip(tag.wireType);
    }
  }
  return { id, pk };
}

// ---------- LoginRequest (sendes) ----------

export function encodeLoginRequest(value: RdLoginRequest): Uint8Array {
  // os_login (felt 12) utelates alltid: RustDesk sitt passord-håndtrykk
  // (felt 2) er det som faktisk brukes her, og feltnumrene for OSLogin sine
  // egne underfelt er ikke uavhengig bekreftet — se docs/rustdesk-protocol.md.
  const writer = new ProtoWriter()
    .string(1, value.username)
    .bytes(2, value.password)
    .string(4, value.myId)
    .string(5, value.myName);

  // Felt 6 er OptionMessage. Utelates den, registrerer verten oss som
  // «bare VP9» — trygt, men da kan vi heller ikke skru av lyd vi ikke bruker.
  if (value.option) writer.message(6, encodeOptionMessage(value.option));

  return writer
    .bool(9, value.videoAckRequired)
    .uint64(10, value.sessionId)
    .string(11, value.version)
    .string(13, value.myPlatform)
    .bytes(14, value.hwid)
    .string(17, value.avatar)
    .finish();
}

// ---------- Resolution / DisplayInfo / PeerInfo (mottas) ----------

function decodeResolution(data: Uint8Array): RdResolution {
  const reader = new ProtoReader(data);
  let width = 0;
  let height = 0;
  while (!reader.eof()) {
    const tag = reader.readTag();
    switch (tag.fieldNumber) {
      case 1:
        width = reader.readInt32();
        break;
      case 2:
        height = reader.readInt32();
        break;
      default:
        reader.skip(tag.wireType);
    }
  }
  return { width, height };
}

function decodeDisplayInfo(data: Uint8Array): RdDisplayInfo {
  const reader = new ProtoReader(data);
  let x = 0;
  let y = 0;
  let width = 0;
  let height = 0;
  let name = '';
  let online = false;
  let cursorEmbedded = false;
  let originalResolution: RdResolution | undefined;
  let scale = 0;
  while (!reader.eof()) {
    const tag = reader.readTag();
    switch (tag.fieldNumber) {
      case 1:
        x = reader.readSint32();
        break;
      case 2:
        y = reader.readSint32();
        break;
      case 3:
        width = reader.readInt32();
        break;
      case 4:
        height = reader.readInt32();
        break;
      case 5:
        name = reader.readString();
        break;
      case 6:
        online = reader.readBool();
        break;
      case 7:
        cursorEmbedded = reader.readBool();
        break;
      case 8:
        originalResolution = decodeResolution(reader.readLengthDelimited());
        break;
      case 9:
        scale = reader.readDouble();
        break;
      default:
        reader.skip(tag.wireType);
    }
  }
  return { x, y, width, height, name, online, cursorEmbedded, originalResolution, scale };
}

function decodePeerInfo(data: Uint8Array): RdPeerInfo {
  const reader = new ProtoReader(data);
  let username = '';
  let hostname = '';
  let platform = '';
  const displays: RdDisplayInfo[] = [];
  let currentDisplay = 0;
  let sasEnabled = false;
  let version = '';
  while (!reader.eof()) {
    const tag = reader.readTag();
    switch (tag.fieldNumber) {
      case 1:
        username = reader.readString();
        break;
      case 2:
        hostname = reader.readString();
        break;
      case 3:
        platform = reader.readString();
        break;
      case 4:
        displays.push(decodeDisplayInfo(reader.readLengthDelimited()));
        break;
      case 5:
        currentDisplay = reader.readInt32();
        break;
      case 6:
        sasEnabled = reader.readBool();
        break;
      case 7:
        version = reader.readString();
        break;
      default:
        reader.skip(tag.wireType);
    }
  }
  return { username, hostname, platform, displays, currentDisplay, sasEnabled, version };
}

// ---------- LoginResponse (mottas) ----------

export function decodeLoginResponse(data: Uint8Array): RdLoginResponse {
  const reader = new ProtoReader(data);
  let result: RdLoginResponse['result'] = { kind: 'error', error: '' };
  let enableTrustedDevices = false;
  while (!reader.eof()) {
    const tag = reader.readTag();
    switch (tag.fieldNumber) {
      case 1:
        result = { kind: 'error', error: reader.readString() };
        break;
      case 2:
        result = { kind: 'peerInfo', peerInfo: decodePeerInfo(reader.readLengthDelimited()) };
        break;
      case 3:
        enableTrustedDevices = reader.readBool();
        break;
      default:
        reader.skip(tag.wireType);
    }
  }
  return { result, enableTrustedDevices };
}

/** Feltnummer for `Message`-konvoluttens `oneof union` (message.proto). Kun det vi faktisk sender/mottar. */
export const MessageField = {
  SignedId: 3,
  PublicKey: 4,
  TestDelay: 5,
  VideoFrame: 6,
  LoginRequest: 7,
  LoginResponse: 8,
  Hash: 9,
  MouseEvent: 10,
  CursorData: 12,
  CursorPosition: 13,
  /** Bar uint64 i oneof-en: «bytt til denne tidligere sendte markørformen». */
  CursorId: 14,
  KeyEvent: 15,
  Clipboard: 16,
  Misc: 19,
  PeerInfo: 25,
  Auth2FA: 27,
} as const;
