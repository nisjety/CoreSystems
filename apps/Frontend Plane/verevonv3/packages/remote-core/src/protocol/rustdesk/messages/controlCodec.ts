import { ProtoWriter } from '../wire/ProtoWriter.js';
import { ProtoReader } from '../wire/ProtoReader.js';
import type {
  RdAuth2FA,
  RdClipboard,
  RdOptionMessage,
  RdSupportedDecoding,
  RdKeyEvent,
  RdMouseEvent,
  RdPermissionInfo,
  RdResolution,
  RdSwitchDisplay,
  RdTestDelay,
} from './types.js';

// ---------- MouseEvent (sendes) ----------

export function encodeMouseEvent(value: RdMouseEvent): Uint8Array {
  const mask = ((value.buttonFlags << 3) | value.kind) >>> 0;
  const writer = new ProtoWriter().int32(1, mask).sint32(2, value.x).sint32(3, value.y);
  if (value.modifiers.length > 0) writer.packedVarint(4, value.modifiers);
  return writer.finish();
}

// ---------- KeyEvent (sendes) ----------

export function encodeKeyEvent(value: RdKeyEvent): Uint8Array {
  const writer = new ProtoWriter().bool(1, value.down).bool(2, value.press);
  switch (value.identification.kind) {
    case 'controlKey':
      writer.enum(3, value.identification.controlKey);
      break;
    case 'chr':
      writer.uint32(4, value.identification.code);
      break;
    case 'unicode':
      writer.uint32(5, value.identification.codepoint);
      break;
    case 'seq':
      writer.string(6, value.identification.text);
      break;
  }
  if (value.modifiers.length > 0) writer.packedVarint(8, value.modifiers);
  writer.enum(9, value.mode);
  return writer.finish();
}

// ---------- Clipboard (sendes og mottas) ----------

export function encodeClipboard(value: RdClipboard): Uint8Array {
  return new ProtoWriter()
    .bool(1, value.compress)
    .bytes(2, value.content)
    .int32(3, value.width)
    .int32(4, value.height)
    .enum(5, value.format)
    .string(6, value.specialName)
    .finish();
}

export function decodeClipboard(data: Uint8Array): RdClipboard {
  const reader = new ProtoReader(data);
  let compress = false;
  let content: Uint8Array = new Uint8Array(0);
  let width = 0;
  let height = 0;
  let format = 0;
  let specialName = '';
  while (!reader.eof()) {
    const tag = reader.readTag();
    switch (tag.fieldNumber) {
      case 1:
        compress = reader.readBool();
        break;
      case 2:
        content = reader.readLengthDelimited();
        break;
      case 3:
        width = reader.readInt32();
        break;
      case 4:
        height = reader.readInt32();
        break;
      case 5:
        format = reader.readInt32();
        break;
      case 6:
        specialName = reader.readString();
        break;
      default:
        reader.skip(tag.wireType);
    }
  }
  return { compress, content, width, height, format, specialName };
}

// ---------- Misc / PermissionInfo (mottas) ----------

function decodePermissionInfo(data: Uint8Array): RdPermissionInfo {
  const reader = new ProtoReader(data);
  let permission = 0;
  let enabled = false;
  while (!reader.eof()) {
    const tag = reader.readTag();
    switch (tag.fieldNumber) {
      case 1:
        permission = reader.readInt32();
        break;
      case 2:
        enabled = reader.readBool();
        break;
      default:
        reader.skip(tag.wireType);
    }
  }
  return { permission, enabled };
}

// ---------- SwitchDisplay (sendes og mottas via Misc felt 5) ----------

const MISC_SWITCH_DISPLAY = 5;
const MISC_PERMISSION_INFO = 6;

function decodeResolution(data: Uint8Array): RdResolution {
  const reader = new ProtoReader(data);
  let width = 0;
  let height = 0;
  while (!reader.eof()) {
    const tag = reader.readTag();
    if (tag.fieldNumber === 1) width = reader.readInt32();
    else if (tag.fieldNumber === 2) height = reader.readInt32();
    else reader.skip(tag.wireType);
  }
  return { width, height };
}

/** Klientens forespørsel trenger bare `display`; øvrige felt er vertens svar-geometri. */
export function encodeSwitchDisplayRequest(display: number): Uint8Array {
  const switchDisplay = new ProtoWriter().int32(1, display).finish();
  return new ProtoWriter().message(MISC_SWITCH_DISPLAY, switchDisplay).finish();
}

function decodeSwitchDisplay(data: Uint8Array): RdSwitchDisplay {
  const reader = new ProtoReader(data);
  let display = 0;
  let x = 0;
  let y = 0;
  let width = 0;
  let height = 0;
  let cursorEmbedded = false;
  let originalResolution: RdResolution | undefined;
  while (!reader.eof()) {
    const tag = reader.readTag();
    switch (tag.fieldNumber) {
      case 1:
        display = reader.readInt32();
        break;
      case 2:
        x = reader.readSint32();
        break;
      case 3:
        y = reader.readSint32();
        break;
      case 4:
        width = reader.readInt32();
        break;
      case 5:
        height = reader.readInt32();
        break;
      case 6:
        cursorEmbedded = reader.readBool();
        break;
      case 8:
        originalResolution = decodeResolution(reader.readLengthDelimited());
        break;
      default:
        reader.skip(tag.wireType);
    }
  }
  return { display, x, y, width, height, cursorEmbedded, originalResolution };
}

export type RdMiscPayload =
  | { readonly kind: 'permissionInfo'; readonly info: RdPermissionInfo }
  | { readonly kind: 'switchDisplay'; readonly value: RdSwitchDisplay }
  | { readonly kind: 'other' };

/**
 * `Misc` (message.proto) er selv et oneof med ~30 varianter; vi dekoder
 * permission_info (felt 6) og switch_display (felt 5) og hopper over resten.
 */
export function decodeMisc(data: Uint8Array): RdMiscPayload {
  const reader = new ProtoReader(data);
  while (!reader.eof()) {
    const tag = reader.readTag();
    if (tag.fieldNumber === MISC_PERMISSION_INFO) {
      return { kind: 'permissionInfo', info: decodePermissionInfo(reader.readLengthDelimited()) };
    }
    if (tag.fieldNumber === MISC_SWITCH_DISPLAY) {
      return { kind: 'switchDisplay', value: decodeSwitchDisplay(reader.readLengthDelimited()) };
    }
    reader.skip(tag.wireType);
  }
  return { kind: 'other' };
}

// ---------- OptionMessage ----------

const MISC_OPTION = 7;

function encodeSupportedDecoding(value: RdSupportedDecoding): Uint8Array {
  // Rekkefølgen følger feltnumrene, ikke deklarasjonsrekkefølgen i vår type.
  // `i444` (7) og `prefer_chroma` (8) utelates bevisst — se RdSupportedDecoding.
  return new ProtoWriter()
    .int32(1, value.abilityVp9 ? 1 : 0)
    .int32(2, value.abilityH264 ? 1 : 0)
    .int32(3, value.abilityH265 ? 1 : 0)
    .enum(4, value.prefer)
    .int32(5, value.abilityVp8 ? 1 : 0)
    .int32(6, value.abilityAv1 ? 1 : 0)
    .finish();
}

export function encodeOptionMessage(value: RdOptionMessage): Uint8Array {
  const writer = new ProtoWriter();
  if (value.imageQuality !== undefined) writer.enum(1, value.imageQuality);
  if (value.showRemoteCursor !== undefined) writer.enum(3, value.showRemoteCursor);
  if (value.disableAudio !== undefined) writer.enum(7, value.disableAudio);
  if (value.disableClipboard !== undefined) writer.enum(8, value.disableClipboard);
  if (value.supportedDecoding !== undefined) {
    writer.message(10, encodeSupportedDecoding(value.supportedDecoding));
  }
  return writer.finish();
}

/** Endringer midt i økten går som `Misc.option` (felt 7). */
export function encodeMiscOption(value: RdOptionMessage): Uint8Array {
  return new ProtoWriter().message(MISC_OPTION, encodeOptionMessage(value)).finish();
}

// ---------- TestDelay (sendes og mottas) ----------

export function encodeTestDelay(value: RdTestDelay): Uint8Array {
  return new ProtoWriter()
    .int64(1, value.time)
    .bool(2, value.fromClient)
    .uint32(3, value.lastDelay)
    .uint32(4, value.targetBitrate)
    .finish();
}

export function decodeTestDelay(data: Uint8Array): RdTestDelay {
  const reader = new ProtoReader(data);
  let time = 0n;
  let fromClient = false;
  let lastDelay = 0;
  let targetBitrate = 0;
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
      case 4:
        targetBitrate = reader.readUint32();
        break;
      default:
        reader.skip(tag.wireType);
    }
  }
  return { time, fromClient, lastDelay, targetBitrate };
}

// ---------- Auth2FA (sendes) ----------

export function encodeAuth2FA(value: RdAuth2FA): Uint8Array {
  return new ProtoWriter().string(1, value.code).bytes(2, value.hwid).finish();
}
