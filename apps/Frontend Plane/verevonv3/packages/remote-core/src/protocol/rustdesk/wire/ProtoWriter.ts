import { encodeVarint, zigzagEncode32 } from './varint.js';

const WIRE_VARINT = 0;
const WIRE_64BIT = 1;
const WIRE_LENGTH_DELIMITED = 2;

/**
 * Håndkodet proto3 wire-writer for akkurat de feltypene RustDesk sin
 * protokoll bruker. Dette er en uavhengig implementasjon av den åpne,
 * offentlige protobuf-spesifikasjonen — IKKE en kopi av RustDesk sin
 * .proto-fil eller generert kode. Feltnumre/-typer for hver melding er
 * verifisert mot rustdesk/hbb_common og dokumentert i
 * docs/rustdesk-protocol.md.
 *
 * Chunks holdes som egne Uint8Array-biter i stedet for én lang
 * number[]-buffer: et enkelt EncodedVideoFrame kan være mange kilobyte, og
 * `array.push(...stortByteArray)` sprer argumenter og kan sprenge kallstakken.
 */
export class ProtoWriter {
  private readonly chunks: Uint8Array[] = [];

  private pushVarint(value: bigint): void {
    const scratch: number[] = [];
    encodeVarint(value, scratch);
    this.chunks.push(Uint8Array.from(scratch));
  }

  private tag(fieldNumber: number, wireType: number): void {
    this.pushVarint(BigInt((fieldNumber << 3) | wireType));
  }

  /** proto3: feltverdier lik standardverdien (0/false/"") skrives aldri på ledningen. */
  int32(fieldNumber: number, value: number): this {
    if (value === 0) return this;
    this.tag(fieldNumber, WIRE_VARINT);
    this.pushVarint(BigInt(value));
    return this;
  }

  uint32(fieldNumber: number, value: number): this {
    if (value === 0) return this;
    this.tag(fieldNumber, WIRE_VARINT);
    this.pushVarint(BigInt(value >>> 0));
    return this;
  }

  sint32(fieldNumber: number, value: number): this {
    if (value === 0) return this;
    this.tag(fieldNumber, WIRE_VARINT);
    this.pushVarint(BigInt(zigzagEncode32(value)));
    return this;
  }

  bool(fieldNumber: number, value: boolean): this {
    if (!value) return this;
    this.tag(fieldNumber, WIRE_VARINT);
    this.chunks.push(Uint8Array.of(1));
    return this;
  }

  enum(fieldNumber: number, value: number): this {
    return this.int32(fieldNumber, value);
  }

  int64(fieldNumber: number, value: bigint | number): this {
    const v = BigInt(value);
    if (v === 0n) return this;
    this.tag(fieldNumber, WIRE_VARINT);
    this.pushVarint(v);
    return this;
  }

  uint64(fieldNumber: number, value: bigint | number): this {
    return this.int64(fieldNumber, value);
  }

  double(fieldNumber: number, value: number): this {
    if (value === 0) return this;
    this.tag(fieldNumber, WIRE_64BIT);
    const buffer = new ArrayBuffer(8);
    new DataView(buffer).setFloat64(0, value, true);
    this.chunks.push(new Uint8Array(buffer));
    return this;
  }

  bytes(fieldNumber: number, value: Uint8Array): this {
    if (value.length === 0) return this;
    this.tag(fieldNumber, WIRE_LENGTH_DELIMITED);
    this.pushVarint(BigInt(value.length));
    this.chunks.push(value);
    return this;
  }

  string(fieldNumber: number, value: string): this {
    if (value.length === 0) return this;
    return this.bytes(fieldNumber, new TextEncoder().encode(value));
  }

  /** Skriver alltid feltet, selv om submeldingen koder til 0 bytes — proto3 skiller "fraværende" fra "satt til en tom melding". */
  message(fieldNumber: number, encoded: Uint8Array): this {
    this.tag(fieldNumber, WIRE_LENGTH_DELIMITED);
    this.pushVarint(BigInt(encoded.length));
    this.chunks.push(encoded);
    return this;
  }

  /** proto3 pakker repeterte skalare/enum-felt som standard (ikke repeterte meldinger). */
  packedVarint(fieldNumber: number, values: readonly number[]): this {
    if (values.length === 0) return this;
    const inner: number[] = [];
    for (const value of values) encodeVarint(BigInt(value), inner);
    this.tag(fieldNumber, WIRE_LENGTH_DELIMITED);
    this.pushVarint(BigInt(inner.length));
    this.chunks.push(Uint8Array.from(inner));
    return this;
  }

  finish(): Uint8Array {
    const total = this.chunks.reduce((sum, chunk) => sum + chunk.length, 0);
    const result = new Uint8Array(total);
    let offset = 0;
    for (const chunk of this.chunks) {
      result.set(chunk, offset);
      offset += chunk.length;
    }
    return result;
  }
}
