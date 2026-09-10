import { ProtoReader } from '../wire/ProtoReader.js';
import type { RdCursorData, RdCursorPosition } from './types.js';

/**
 * Markørmeldinger (mottas). Feltnumre verifisert mot message.proto 2026-09-07:
 *   CursorData { id(uint64)=1, hotx(sint32)=2, hoty(sint32)=3, width=4, height=5, colors(bytes)=6 }
 *   CursorPosition { x(sint32)=1, y(sint32)=2 }
 * `cursor_id` (Message-felt 14) er en bar uint64 i selve envelopen og har
 * ingen egen submelding — den dekodes i envelope.ts.
 *
 * MERK: `colors` leveres KOMPRIMERT (zstd) og pakkes ikke ut her — kodeken
 * gjør bare wire-dekoding. Se RustDeskProtocol for utpakking og validering.
 */
export function decodeCursorData(data: Uint8Array): RdCursorData {
  const reader = new ProtoReader(data);
  let id = 0n;
  let hotx = 0;
  let hoty = 0;
  let width = 0;
  let height = 0;
  let colors: Uint8Array = new Uint8Array(0);
  while (!reader.eof()) {
    const tag = reader.readTag();
    switch (tag.fieldNumber) {
      case 1:
        id = reader.readUint64();
        break;
      case 2:
        hotx = reader.readSint32();
        break;
      case 3:
        hoty = reader.readSint32();
        break;
      case 4:
        width = reader.readInt32();
        break;
      case 5:
        height = reader.readInt32();
        break;
      case 6:
        colors = reader.readLengthDelimited();
        break;
      default:
        reader.skip(tag.wireType);
    }
  }
  return { id, hotx, hoty, width, height, colors };
}

export function decodeCursorPosition(data: Uint8Array): RdCursorPosition {
  const reader = new ProtoReader(data);
  let x = 0;
  let y = 0;
  while (!reader.eof()) {
    const tag = reader.readTag();
    if (tag.fieldNumber === 1) x = reader.readSint32();
    else if (tag.fieldNumber === 2) y = reader.readSint32();
    else reader.skip(tag.wireType);
  }
  return { x, y };
}
