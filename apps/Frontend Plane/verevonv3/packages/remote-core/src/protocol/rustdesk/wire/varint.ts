/**
 * Lav-nivå varint/zigzag-primitiver for proto3 wire-format (Google sin åpne
 * spesifikasjon — ikke RustDesk-spesifikk). Brukt av ProtoWriter/ProtoReader.
 */

const U64_MASK = (1n << 64n) - 1n;

/** To-komplement-wrapper til 64 bit før varint-koding, slik proto3 krever for negative int32/int64. */
export function encodeVarint(valueIn: bigint, out: number[]): void {
  let value = valueIn & U64_MASK;
  do {
    let byte = Number(value & 0x7fn);
    value >>= 7n;
    if (value !== 0n) byte |= 0x80;
    out.push(byte);
  } while (value !== 0n);
}

export function decodeVarint(bytes: Uint8Array, offset: number): { value: bigint; next: number } {
  let result = 0n;
  let shift = 0n;
  let pos = offset;
  for (;;) {
    const byte = bytes[pos];
    if (byte === undefined) throw new RangeError('Truncated varint');
    result |= BigInt(byte & 0x7f) << shift;
    pos += 1;
    if ((byte & 0x80) === 0) break;
    shift += 7n;
    if (shift > 63n) throw new RangeError('Varint exceeds 64 bits');
  }
  return { value: result, next: pos };
}

export function zigzagEncode32(value: number): number {
  return ((value << 1) ^ (value >> 31)) >>> 0;
}

export function zigzagDecode32(value: number): number {
  return (value >>> 1) ^ -(value & 1);
}

export function toSignedInt32(raw: bigint): number {
  const truncated = Number(raw & 0xffffffffn);
  return truncated >= 0x80000000 ? truncated - 0x100000000 : truncated;
}

export function toUnsignedInt32(raw: bigint): number {
  return Number(raw & 0xffffffffn);
}

export function toSignedInt64(raw: bigint): bigint {
  const wrapped = raw & U64_MASK;
  return wrapped >= 1n << 63n ? wrapped - (1n << 64n) : wrapped;
}
