import { ProtoReader } from '../wire/ProtoReader.js';
import type { RdEncodedVideoFrame, RdVideoCodec, RdVideoFrame } from './types.js';

const CODEC_FIELD: Readonly<Record<number, RdVideoCodec>> = {
  6: 'vp9',
  10: 'h264',
  11: 'h265',
  12: 'vp8',
  13: 'av1',
};

function decodeEncodedVideoFrame(data: Uint8Array): RdEncodedVideoFrame {
  const reader = new ProtoReader(data);
  let frameData: Uint8Array = new Uint8Array(0);
  let key = false;
  let pts = 0n;
  while (!reader.eof()) {
    const tag = reader.readTag();
    switch (tag.fieldNumber) {
      case 1:
        frameData = reader.readLengthDelimited();
        break;
      case 2:
        key = reader.readBool();
        break;
      case 3:
        pts = reader.readInt64();
        break;
      default:
        reader.skip(tag.wireType);
    }
  }
  return { data: frameData, key, pts };
}

function decodeEncodedVideoFrames(data: Uint8Array): RdEncodedVideoFrame[] {
  const reader = new ProtoReader(data);
  const frames: RdEncodedVideoFrame[] = [];
  while (!reader.eof()) {
    const tag = reader.readTag();
    if (tag.fieldNumber === 1) frames.push(decodeEncodedVideoFrame(reader.readLengthDelimited()));
    else reader.skip(tag.wireType);
  }
  return frames;
}

/**
 * Returnerer undefined for rgb/yuv-variantene (felt 7/8): RustDesk sender rå
 * pikseldata for disse på en måte forskningsgjennomgangen ikke fikk verifisert
 * nøyaktig (se docs/rustdesk-protocol.md, "Ukjente"). Vi støtter kun de kodede
 * kodek-variantene (vp9/vp8/h264/h265/av1) i denne versjonen — det er det
 * WebCodecs-veien vi bygger mot faktisk trenger.
 */
export function decodeVideoFrame(data: Uint8Array): RdVideoFrame | undefined {
  const reader = new ProtoReader(data);
  let codec: RdVideoCodec | undefined;
  let frames: RdEncodedVideoFrame[] = [];
  let display = 0;
  while (!reader.eof()) {
    const tag = reader.readTag();
    const mappedCodec = CODEC_FIELD[tag.fieldNumber];
    if (mappedCodec) {
      codec = mappedCodec;
      frames = decodeEncodedVideoFrames(reader.readLengthDelimited());
    } else if (tag.fieldNumber === 14) {
      display = reader.readInt32();
    } else {
      reader.skip(tag.wireType);
    }
  }
  if (!codec) return undefined;
  return { codec, frames, display };
}
