import { describe, expect, it } from 'vitest';
import { ProtoWriter } from '../../../../src/protocol/rustdesk/wire/ProtoWriter.js';
import { ProtoReader } from '../../../../src/protocol/rustdesk/wire/ProtoReader.js';

describe('ProtoWriter / ProtoReader', () => {
  it('matches the canonical protobuf example: int32 field 1 = 150 -> 08 96 01', () => {
    // From Google's own protobuf encoding documentation (an open, public wire
    // format spec, not RustDesk-specific) — a golden check independent of our
    // own round-trip logic.
    const encoded = new ProtoWriter().int32(1, 150).finish();
    expect([...encoded]).toEqual([0x08, 0x96, 0x01]);
  });

  it('omits default-valued fields entirely, per proto3 semantics', () => {
    const encoded = new ProtoWriter().int32(1, 0).string(2, '').bool(3, false).finish();
    expect(encoded.length).toBe(0);
  });

  it('round-trips int32 including negative values (10-byte twos-complement form)', () => {
    for (const value of [0, 1, -1, 127, 128, -128, 2147483647, -2147483648]) {
      const encoded = new ProtoWriter().int32(1, value).finish();
      const reader = new ProtoReader(encoded);
      if (value === 0) {
        expect(reader.eof()).toBe(true);
        continue;
      }
      const tag = reader.readTag();
      expect(tag.fieldNumber).toBe(1);
      expect(reader.readInt32()).toBe(value);
    }
  });

  it('round-trips sint32 (zigzag) for negative and positive values', () => {
    for (const value of [0, -1, 1, -2, 2, -2147483648, 2147483647]) {
      const encoded = new ProtoWriter().sint32(1, value).finish();
      if (value === 0) continue;
      const reader = new ProtoReader(encoded);
      reader.readTag();
      expect(reader.readSint32()).toBe(value);
    }
  });

  it('round-trips uint64/int64 including values beyond 2^53', () => {
    const big = 9_007_199_254_740_993n; // 2^53 + 1, beyond safe JS number precision
    const encoded = new ProtoWriter().uint64(5, big).finish();
    const reader = new ProtoReader(encoded);
    reader.readTag();
    expect(reader.readUint64()).toBe(big);
  });

  it('round-trips negative int64', () => {
    const encoded = new ProtoWriter().int64(1, -12345n).finish();
    const reader = new ProtoReader(encoded);
    reader.readTag();
    expect(reader.readInt64()).toBe(-12345n);
  });

  it('round-trips bytes and string fields', () => {
    const payload = new Uint8Array([1, 2, 3, 255, 0, 128]);
    const encoded = new ProtoWriter().bytes(1, payload).string(2, 'Æøå — Verevon').finish();
    const reader = new ProtoReader(encoded);

    const tag1 = reader.readTag();
    expect(tag1).toEqual({ fieldNumber: 1, wireType: 2 });
    expect([...reader.readLengthDelimited()]).toEqual([...payload]);

    const tag2 = reader.readTag();
    expect(tag2).toEqual({ fieldNumber: 2, wireType: 2 });
    expect(reader.readString()).toBe('Æøå — Verevon');
  });

  it('round-trips a large byte payload without exceeding the call stack (spread-argument regression guard)', () => {
    const large = new Uint8Array(500_000).fill(7);
    const encoded = new ProtoWriter().bytes(9, large).finish();
    const reader = new ProtoReader(encoded);
    reader.readTag();
    const decoded = reader.readLengthDelimited();
    expect(decoded.length).toBe(large.length);
    expect(decoded[0]).toBe(7);
    expect(decoded[decoded.length - 1]).toBe(7);
  });

  it('round-trips a nested message field', () => {
    const inner = new ProtoWriter().string(1, 'salt-value').string(2, 'challenge-value').finish();
    const outer = new ProtoWriter().message(9, inner).finish();

    const reader = new ProtoReader(outer);
    const tag = reader.readTag();
    expect(tag.fieldNumber).toBe(9);
    const innerReader = new ProtoReader(reader.readLengthDelimited());

    innerReader.readTag();
    expect(innerReader.readString()).toBe('salt-value');
    innerReader.readTag();
    expect(innerReader.readString()).toBe('challenge-value');
  });

  it('round-trips a packed repeated varint field', () => {
    const values = [1, 4, 29, 100];
    const encoded = new ProtoWriter().packedVarint(4, values).finish();
    const reader = new ProtoReader(encoded);
    reader.readTag();
    expect(reader.readPackedVarints()).toEqual(values);
  });

  it('skip() advances past unknown fields of every wire type', () => {
    const encoded = new ProtoWriter()
      .int32(1, 42) // varint
      .double(2, 3.5) // 64-bit
      .string(3, 'ignored') // length-delimited
      .finish();

    const reader = new ProtoReader(encoded);
    let seenField1 = false;
    while (!reader.eof()) {
      const tag = reader.readTag();
      if (tag.fieldNumber === 1) {
        seenField1 = true;
        expect(reader.readInt32()).toBe(42);
      } else {
        reader.skip(tag.wireType);
      }
    }
    expect(seenField1).toBe(true);
  });
});
