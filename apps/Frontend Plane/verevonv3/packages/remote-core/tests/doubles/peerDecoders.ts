import { ProtoReader } from '../../src/protocol/rustdesk/wire/ProtoReader.js';

/**
 * Decoders for the client→host direction. remote-core only ever *encodes*
 * these (it is always the controlling side), so the host side of the wire
 * lives here in the test tree — see peerEncoders.ts for the same reasoning.
 */

export interface PeerSupportedDecoding {
  readonly abilityVp9: number;
  readonly abilityH264: number;
  readonly abilityH265: number;
  readonly prefer: number;
  readonly abilityVp8: number;
  readonly abilityAv1: number;
  /** True when the client sent i444 (7) or prefer_chroma (8) — it must not. */
  readonly sentChromaFields: boolean;
}

export interface PeerOptionMessage {
  readonly imageQuality?: number;
  readonly showRemoteCursor?: number;
  readonly disableAudio?: number;
  readonly disableClipboard?: number;
  readonly supportedDecoding?: PeerSupportedDecoding;
}

export interface PeerLoginRequest {
  readonly kind: 'loginRequest';
  readonly targetId: string;
  readonly password: Uint8Array;
  readonly myId: string;
  readonly myName: string;
  readonly myPlatform: string;
  readonly version: string;
  readonly sessionId: bigint;
  readonly option?: PeerOptionMessage;
}

export interface PeerPublicKey {
  readonly kind: 'publicKey';
  readonly asymmetricValue: Uint8Array;
  readonly symmetricValue: Uint8Array;
}

export interface PeerMouseEvent {
  readonly kind: 'mouseEvent';
  readonly mask: number;
  readonly x: number;
  readonly y: number;
}

export interface PeerKeyEvent {
  readonly kind: 'keyEvent';
  readonly down: boolean;
  readonly press: boolean;
  readonly controlKey?: number;
  readonly unicode?: number;
  readonly seq?: string;
  readonly mode: number;
}

export interface PeerClipboard {
  readonly kind: 'clipboard';
  readonly text: string;
  readonly format: number;
}

export interface PeerRequestRelay {
  readonly kind: 'requestRelay';
  readonly id: string;
  readonly uuid: string;
}

export interface PeerPunchHoleRequest {
  readonly kind: 'punchHoleRequest';
  readonly id: string;
  readonly licenceKey: string;
  readonly forceRelay: boolean;
  readonly version: string;
}

export interface PeerTestDelay {
  readonly kind: 'testDelay';
  readonly time: bigint;
  readonly fromClient: boolean;
  readonly lastDelay: number;
}

/** Client → host request: only `display` is meaningful in this direction. */
export interface PeerSwitchDisplay {
  readonly kind: 'switchDisplay';
  readonly display: number;
}

export interface PeerAuth2FA {
  readonly kind: 'auth2fa';
  readonly code: string;
  readonly hwid: Uint8Array;
}

/** Client -> host mid-session settings change (Misc.option, field 7). */
export interface PeerMiscOption {
  readonly kind: 'option';
  readonly option: PeerOptionMessage;
}

export type PeerMessage =
  | PeerLoginRequest
  | PeerPublicKey
  | PeerMouseEvent
  | PeerKeyEvent
  | PeerClipboard
  | PeerTestDelay
  | PeerSwitchDisplay
  | PeerAuth2FA
  | PeerMiscOption
  | { readonly kind: 'other'; readonly fieldNumber: number };

export type PeerRendezvousMessage =
  | PeerRequestRelay
  | PeerPunchHoleRequest
  | { readonly kind: 'other'; readonly fieldNumber: number };

export function decodePeerMessage(data: Uint8Array): PeerMessage {
  const reader = new ProtoReader(data);
  while (!reader.eof()) {
    const tag = reader.readTag();
    switch (tag.fieldNumber) {
      case 4:
        return decodePublicKey(reader.readLengthDelimited());
      case 5:
        return decodeTestDelay(reader.readLengthDelimited());
      case 7:
        return decodeLoginRequest(reader.readLengthDelimited());
      case 10:
        return decodeMouseEvent(reader.readLengthDelimited());
      case 15:
        return decodeKeyEvent(reader.readLengthDelimited());
      case 16:
        return decodeClipboard(reader.readLengthDelimited());
      case 19:
        return decodeMiscFromClient(reader.readLengthDelimited());
      case 27:
        return decodeAuth2FA(reader.readLengthDelimited());
      default:
        reader.skip(tag.wireType);
        return { kind: 'other', fieldNumber: tag.fieldNumber };
    }
  }
  return { kind: 'other', fieldNumber: -1 };
}

function decodeTestDelay(data: Uint8Array): PeerTestDelay {
  const reader = new ProtoReader(data);
  let time = 0n;
  let fromClient = false;
  let lastDelay = 0;
  while (!reader.eof()) {
    const tag = reader.readTag();
    switch (tag.fieldNumber) {
      case 1:
        time = reader.readInt64();
        break;
      case 2:
        fromClient = reader.readBool();
        break;
      case 3:
        lastDelay = reader.readUint32();
        break;
      default:
        reader.skip(tag.wireType);
    }
  }
  return { kind: 'testDelay', time, fromClient, lastDelay };
}

/** The only Misc variant a controller sends that the fake host cares about is switch_display (5). */
function decodeMiscFromClient(data: Uint8Array): PeerMessage {
  const reader = new ProtoReader(data);
  while (!reader.eof()) {
    const tag = reader.readTag();
    if (tag.fieldNumber === 5) {
      const inner = new ProtoReader(reader.readLengthDelimited());
      let display = 0;
      while (!inner.eof()) {
        const innerTag = inner.readTag();
        if (innerTag.fieldNumber === 1) display = inner.readInt32();
        else inner.skip(innerTag.wireType);
      }
      return { kind: 'switchDisplay', display };
    }
    if (tag.fieldNumber === 7) {
      return { kind: 'option', option: decodeOptionMessage(reader.readLengthDelimited()) };
    }
    reader.skip(tag.wireType);
    return { kind: 'other', fieldNumber: 19 };
  }
  return { kind: 'other', fieldNumber: 19 };
}

function decodeAuth2FA(data: Uint8Array): PeerAuth2FA {
  const reader = new ProtoReader(data);
  let code = '';
  let hwid: Uint8Array = new Uint8Array(0);
  while (!reader.eof()) {
    const tag = reader.readTag();
    if (tag.fieldNumber === 1) code = reader.readString();
    else if (tag.fieldNumber === 2) hwid = reader.readLengthDelimited();
    else reader.skip(tag.wireType);
  }
  return { kind: 'auth2fa', code, hwid };
}

export function decodePeerRendezvousMessage(data: Uint8Array): PeerRendezvousMessage {
  const reader = new ProtoReader(data);
  while (!reader.eof()) {
    const tag = reader.readTag();
    switch (tag.fieldNumber) {
      case 8:
        return decodePunchHoleRequest(reader.readLengthDelimited());
      case 18:
        return decodeRequestRelay(reader.readLengthDelimited());
      default:
        reader.skip(tag.wireType);
        return { kind: 'other', fieldNumber: tag.fieldNumber };
    }
  }
  return { kind: 'other', fieldNumber: -1 };
}

function decodePublicKey(data: Uint8Array): PeerPublicKey {
  const reader = new ProtoReader(data);
  let asymmetricValue: Uint8Array = new Uint8Array(0);
  let symmetricValue: Uint8Array = new Uint8Array(0);
  while (!reader.eof()) {
    const tag = reader.readTag();
    if (tag.fieldNumber === 1) asymmetricValue = reader.readLengthDelimited();
    else if (tag.fieldNumber === 2) symmetricValue = reader.readLengthDelimited();
    else reader.skip(tag.wireType);
  }
  return { kind: 'publicKey', asymmetricValue, symmetricValue };
}

function decodeLoginRequest(data: Uint8Array): PeerLoginRequest {
  const reader = new ProtoReader(data);
  let targetId = '';
  let password: Uint8Array = new Uint8Array(0);
  let myId = '';
  let myName = '';
  let myPlatform = '';
  let version = '';
  let sessionId = 0n;
  let option: PeerOptionMessage | undefined;
  while (!reader.eof()) {
    const tag = reader.readTag();
    switch (tag.fieldNumber) {
      case 1:
        targetId = reader.readString();
        break;
      case 2:
        password = reader.readLengthDelimited();
        break;
      case 4:
        myId = reader.readString();
        break;
      case 5:
        myName = reader.readString();
        break;
      case 6:
        option = decodeOptionMessage(reader.readLengthDelimited());
        break;
      case 10:
        sessionId = reader.readUint64();
        break;
      case 11:
        version = reader.readString();
        break;
      case 13:
        myPlatform = reader.readString();
        break;
      default:
        reader.skip(tag.wireType);
    }
  }
  return { kind: 'loginRequest', targetId, password, myId, myName, myPlatform, version, sessionId, option };
}

function decodeSupportedDecoding(data: Uint8Array): PeerSupportedDecoding {
  const reader = new ProtoReader(data);
  let abilityVp9 = 0;
  let abilityH264 = 0;
  let abilityH265 = 0;
  let prefer = 0;
  let abilityVp8 = 0;
  let abilityAv1 = 0;
  let sentChromaFields = false;
  while (!reader.eof()) {
    const tag = reader.readTag();
    switch (tag.fieldNumber) {
      case 1:
        abilityVp9 = reader.readInt32();
        break;
      case 2:
        abilityH264 = reader.readInt32();
        break;
      case 3:
        abilityH265 = reader.readInt32();
        break;
      case 4:
        prefer = reader.readInt32();
        break;
      case 5:
        abilityVp8 = reader.readInt32();
        break;
      case 6:
        abilityAv1 = reader.readInt32();
        break;
      case 7:
      case 8:
        sentChromaFields = true;
        reader.skip(tag.wireType);
        break;
      default:
        reader.skip(tag.wireType);
    }
  }
  return { abilityVp9, abilityH264, abilityH265, prefer, abilityVp8, abilityAv1, sentChromaFields };
}

function decodeOptionMessage(data: Uint8Array): PeerOptionMessage {
  const reader = new ProtoReader(data);
  let imageQuality: number | undefined;
  let showRemoteCursor: number | undefined;
  let disableAudio: number | undefined;
  let disableClipboard: number | undefined;
  let supportedDecoding: PeerSupportedDecoding | undefined;
  while (!reader.eof()) {
    const tag = reader.readTag();
    switch (tag.fieldNumber) {
      case 1:
        imageQuality = reader.readInt32();
        break;
      case 3:
        showRemoteCursor = reader.readInt32();
        break;
      case 7:
        disableAudio = reader.readInt32();
        break;
      case 8:
        disableClipboard = reader.readInt32();
        break;
      case 10:
        supportedDecoding = decodeSupportedDecoding(reader.readLengthDelimited());
        break;
      default:
        reader.skip(tag.wireType);
    }
  }
  return { imageQuality, showRemoteCursor, disableAudio, disableClipboard, supportedDecoding };
}

function decodeMouseEvent(data: Uint8Array): PeerMouseEvent {
  const reader = new ProtoReader(data);
  let mask = 0;
  let x = 0;
  let y = 0;
  while (!reader.eof()) {
    const tag = reader.readTag();
    switch (tag.fieldNumber) {
      case 1:
        mask = reader.readInt32();
        break;
      case 2:
        x = reader.readSint32();
        break;
      case 3:
        y = reader.readSint32();
        break;
      default:
        reader.skip(tag.wireType);
    }
  }
  return { kind: 'mouseEvent', mask, x, y };
}

function decodeKeyEvent(data: Uint8Array): PeerKeyEvent {
  const reader = new ProtoReader(data);
  let down = false;
  let press = false;
  let controlKey: number | undefined;
  let unicode: number | undefined;
  let seq: string | undefined;
  let mode = 0;
  while (!reader.eof()) {
    const tag = reader.readTag();
    switch (tag.fieldNumber) {
      case 1:
        down = reader.readBool();
        break;
      case 2:
        press = reader.readBool();
        break;
      case 3:
        controlKey = reader.readInt32();
        break;
      case 5:
        unicode = reader.readUint32();
        break;
      case 6:
        seq = reader.readString();
        break;
      case 9:
        mode = reader.readInt32();
        break;
      default:
        reader.skip(tag.wireType);
    }
  }
  return { kind: 'keyEvent', down, press, controlKey, unicode, seq, mode };
}

function decodeClipboard(data: Uint8Array): PeerClipboard {
  const reader = new ProtoReader(data);
  let text = '';
  let format = 0;
  while (!reader.eof()) {
    const tag = reader.readTag();
    switch (tag.fieldNumber) {
      case 2:
        text = new TextDecoder().decode(reader.readLengthDelimited());
        break;
      case 5:
        format = reader.readInt32();
        break;
      default:
        reader.skip(tag.wireType);
    }
  }
  return { kind: 'clipboard', text, format };
}

function decodeRequestRelay(data: Uint8Array): PeerRequestRelay {
  const reader = new ProtoReader(data);
  let id = '';
  let uuid = '';
  while (!reader.eof()) {
    const tag = reader.readTag();
    if (tag.fieldNumber === 1) id = reader.readString();
    else if (tag.fieldNumber === 2) uuid = reader.readString();
    else reader.skip(tag.wireType);
  }
  return { kind: 'requestRelay', id, uuid };
}

function decodePunchHoleRequest(data: Uint8Array): PeerPunchHoleRequest {
  const reader = new ProtoReader(data);
  let id = '';
  let licenceKey = '';
  let forceRelay = false;
  let version = '';
  while (!reader.eof()) {
    const tag = reader.readTag();
    switch (tag.fieldNumber) {
      case 1:
        id = reader.readString();
        break;
      case 3:
        licenceKey = reader.readString();
        break;
      case 6:
        version = reader.readString();
        break;
      case 8:
        forceRelay = reader.readBool();
        break;
      default:
        reader.skip(tag.wireType);
    }
  }
  return { kind: 'punchHoleRequest', id, licenceKey, forceRelay, version };
}
