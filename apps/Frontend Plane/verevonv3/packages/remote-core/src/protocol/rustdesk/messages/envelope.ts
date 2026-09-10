import { ProtoWriter } from '../wire/ProtoWriter.js';
import { ProtoReader } from '../wire/ProtoReader.js';
import type {
  RdAuth2FA,
  RdClipboard,
  RdCursorData,
  RdCursorPosition,
  RdOptionMessage,
  RdHash,
  RdKeyEvent,
  RdKeyExchange,
  RdLoginRequest,
  RdLoginResponse,
  RdMouseEvent,
  RdPermissionInfo,
  RdPublicKey,
  RdPunchHoleRequest,
  RdPunchHoleResponse,
  RdRelayResponse,
  RdRequestRelay,
  RdSignedId,
  RdSwitchDisplay,
  RdTestDelay,
  RdVideoFrame,
} from './types.js';
import { MessageField, decodeHash, decodeLoginResponse, encodeLoginRequest, encodePublicKey, decodePublicKey, encodeSignedId, decodeSignedId } from './sessionCodec.js';
import {
  encodeClipboard,
  decodeClipboard,
  encodeKeyEvent,
  encodeMouseEvent,
  decodeMisc,
  encodeSwitchDisplayRequest,
  encodeTestDelay,
  decodeTestDelay,
  encodeAuth2FA,
  encodeMiscOption,
} from './controlCodec.js';
import { decodeVideoFrame } from './videoCodec.js';
import { decodeCursorData, decodeCursorPosition } from './cursorCodec.js';
import {
  RendezvousField,
  encodeKeyExchange,
  decodeKeyExchange,
  encodePunchHoleRequest,
  decodePunchHoleResponse,
  encodeRequestRelay,
  decodeRelayResponse,
} from './rendezvousCodec.js';

// ---------- `Message` (peer-til-peer-økten) ----------

export type OutgoingMessage =
  | { readonly kind: 'signedId'; readonly value: RdSignedId }
  | { readonly kind: 'publicKey'; readonly value: RdPublicKey }
  | { readonly kind: 'loginRequest'; readonly value: RdLoginRequest }
  | { readonly kind: 'mouseEvent'; readonly value: RdMouseEvent }
  | { readonly kind: 'keyEvent'; readonly value: RdKeyEvent }
  | { readonly kind: 'clipboard'; readonly value: RdClipboard }
  | { readonly kind: 'testDelay'; readonly value: RdTestDelay }
  | { readonly kind: 'switchDisplay'; readonly display: number }
  | { readonly kind: 'auth2fa'; readonly value: RdAuth2FA }
  | { readonly kind: 'option'; readonly value: RdOptionMessage };

export function encodeMessage(message: OutgoingMessage): Uint8Array {
  const writer = new ProtoWriter();
  switch (message.kind) {
    case 'testDelay':
      writer.message(MessageField.TestDelay, encodeTestDelay(message.value));
      break;
    case 'switchDisplay':
      writer.message(MessageField.Misc, encodeSwitchDisplayRequest(message.display));
      break;
    case 'option':
      writer.message(MessageField.Misc, encodeMiscOption(message.value));
      break;
    case 'auth2fa':
      writer.message(MessageField.Auth2FA, encodeAuth2FA(message.value));
      break;
    case 'signedId':
      writer.message(MessageField.SignedId, encodeSignedId(message.value));
      break;
    case 'publicKey':
      writer.message(MessageField.PublicKey, encodePublicKey(message.value));
      break;
    case 'loginRequest':
      writer.message(MessageField.LoginRequest, encodeLoginRequest(message.value));
      break;
    case 'mouseEvent':
      writer.message(MessageField.MouseEvent, encodeMouseEvent(message.value));
      break;
    case 'keyEvent':
      writer.message(MessageField.KeyEvent, encodeKeyEvent(message.value));
      break;
    case 'clipboard':
      writer.message(MessageField.Clipboard, encodeClipboard(message.value));
      break;
  }
  return writer.finish();
}

export type IncomingMessage =
  | { readonly kind: 'signedId'; readonly value: RdSignedId }
  | { readonly kind: 'publicKey'; readonly value: RdPublicKey }
  | { readonly kind: 'hash'; readonly value: RdHash }
  | { readonly kind: 'loginResponse'; readonly value: RdLoginResponse }
  | { readonly kind: 'videoFrame'; readonly value: RdVideoFrame }
  | { readonly kind: 'clipboard'; readonly value: RdClipboard }
  | { readonly kind: 'permissionInfo'; readonly value: RdPermissionInfo }
  | { readonly kind: 'switchDisplay'; readonly value: RdSwitchDisplay }
  | { readonly kind: 'testDelay'; readonly value: RdTestDelay }
  | { readonly kind: 'cursorData'; readonly value: RdCursorData }
  | { readonly kind: 'cursorPosition'; readonly value: RdCursorPosition }
  | { readonly kind: 'cursorId'; readonly value: bigint }
  | { readonly kind: 'unknown'; readonly fieldNumber: number };

/**
 * `Message` er ETT oneof — kun ett felt er satt per melding. Vi leser den
 * første kjente taggen og returnerer umiddelbart; alt annet (audio, filer,
 * terminal, chat, ...) faller til 'unknown' i stedet for å late som vi
 * forstår det.
 */
export function decodeMessage(data: Uint8Array): IncomingMessage {
  const reader = new ProtoReader(data);
  while (!reader.eof()) {
    const tag = reader.readTag();
    switch (tag.fieldNumber) {
      case MessageField.SignedId:
        return { kind: 'signedId', value: decodeSignedId(reader.readLengthDelimited()) };
      case MessageField.PublicKey:
        return { kind: 'publicKey', value: decodePublicKey(reader.readLengthDelimited()) };
      case MessageField.Hash:
        return { kind: 'hash', value: decodeHash(reader.readLengthDelimited()) };
      case MessageField.LoginResponse:
        return { kind: 'loginResponse', value: decodeLoginResponse(reader.readLengthDelimited()) };
      case MessageField.VideoFrame: {
        const frame = decodeVideoFrame(reader.readLengthDelimited());
        return frame ? { kind: 'videoFrame', value: frame } : { kind: 'unknown', fieldNumber: tag.fieldNumber };
      }
      case MessageField.Clipboard:
        return { kind: 'clipboard', value: decodeClipboard(reader.readLengthDelimited()) };
      case MessageField.TestDelay:
        return { kind: 'testDelay', value: decodeTestDelay(reader.readLengthDelimited()) };
      case MessageField.CursorData:
        return { kind: 'cursorData', value: decodeCursorData(reader.readLengthDelimited()) };
      case MessageField.CursorPosition:
        return { kind: 'cursorPosition', value: decodeCursorPosition(reader.readLengthDelimited()) };
      case MessageField.CursorId:
        return { kind: 'cursorId', value: reader.readUint64() };
      case MessageField.Misc: {
        const misc = decodeMisc(reader.readLengthDelimited());
        if (misc.kind === 'permissionInfo') return { kind: 'permissionInfo', value: misc.info };
        if (misc.kind === 'switchDisplay') return { kind: 'switchDisplay', value: misc.value };
        return { kind: 'unknown', fieldNumber: tag.fieldNumber };
      }
      default:
        // `Message` is a pure oneof — exactly one field is ever populated —
        // so an unrecognized field IS the answer, not noise to skip past.
        reader.skip(tag.wireType);
        return { kind: 'unknown', fieldNumber: tag.fieldNumber };
    }
  }
  return { kind: 'unknown', fieldNumber: -1 };
}

// ---------- `RendezvousMessage` (hbbs/hbbr-kontrollplanet) ----------

export type OutgoingRendezvousMessage =
  | { readonly kind: 'punchHoleRequest'; readonly value: RdPunchHoleRequest }
  | { readonly kind: 'requestRelay'; readonly value: RdRequestRelay }
  | { readonly kind: 'keyExchange'; readonly value: RdKeyExchange };

export function encodeRendezvousMessage(message: OutgoingRendezvousMessage): Uint8Array {
  const writer = new ProtoWriter();
  switch (message.kind) {
    case 'punchHoleRequest':
      writer.message(RendezvousField.PunchHoleRequest, encodePunchHoleRequest(message.value));
      break;
    case 'requestRelay':
      writer.message(RendezvousField.RequestRelay, encodeRequestRelay(message.value));
      break;
    case 'keyExchange':
      writer.message(RendezvousField.KeyExchange, encodeKeyExchange(message.value));
      break;
  }
  return writer.finish();
}

export type IncomingRendezvousMessage =
  | { readonly kind: 'punchHoleResponse'; readonly value: RdPunchHoleResponse }
  | { readonly kind: 'relayResponse'; readonly value: RdRelayResponse }
  | { readonly kind: 'keyExchange'; readonly value: RdKeyExchange }
  | { readonly kind: 'unknown'; readonly fieldNumber: number };

export function decodeRendezvousMessage(data: Uint8Array): IncomingRendezvousMessage {
  const reader = new ProtoReader(data);
  while (!reader.eof()) {
    const tag = reader.readTag();
    switch (tag.fieldNumber) {
      case RendezvousField.PunchHoleResponse:
        return { kind: 'punchHoleResponse', value: decodePunchHoleResponse(reader.readLengthDelimited()) };
      case RendezvousField.RelayResponse:
        return { kind: 'relayResponse', value: decodeRelayResponse(reader.readLengthDelimited()) };
      case RendezvousField.KeyExchange:
        return { kind: 'keyExchange', value: decodeKeyExchange(reader.readLengthDelimited()) };
      default:
        // `RendezvousMessage` is likewise a pure oneof — see decodeMessage() above.
        reader.skip(tag.wireType);
        return { kind: 'unknown', fieldNumber: tag.fieldNumber };
    }
  }
  return { kind: 'unknown', fieldNumber: -1 };
}
