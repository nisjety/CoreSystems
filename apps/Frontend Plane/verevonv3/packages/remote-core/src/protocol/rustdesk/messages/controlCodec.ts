import { ProtoWriter } from '../wire/ProtoWriter.js';
import { ProtoReader } from '../wire/ProtoReader.js';
import type { RdClipboard, RdKeyEvent, RdMouseEvent, RdPermissionInfo } from './types.js';

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

export type RdMiscPayload = { readonly kind: 'permissionInfo'; readonly info: RdPermissionInfo } | { readonly kind: 'other' };

/** `Misc` (message.proto) er selv et oneof med ~30 varianter; vi dekoder kun permission_info (felt 6) og hopper over resten. */
export function decodeMisc(data: Uint8Array): RdMiscPayload {
  const reader = new ProtoReader(data);
  while (!reader.eof()) {
    const tag = reader.readTag();
    if (tag.fieldNumber === 6) {
      return { kind: 'permissionInfo', info: decodePermissionInfo(reader.readLengthDelimited()) };
    }
    reader.skip(tag.wireType);
  }
  return { kind: 'other' };
}
