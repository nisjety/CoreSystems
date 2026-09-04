import { decodeVarint, toSignedInt32, toSignedInt64, toUnsignedInt32, zigzagDecode32 } from './varint.js';

const WIRE_VARINT = 0;
const WIRE_64BIT = 1;
const WIRE_LENGTH_DELIMITED = 2;
const WIRE_32BIT = 5;

export interface FieldTag {
  readonly fieldNumber: number;
  readonly wireType: number;
}

/**
 * Leser proto3 wire-format uten et skjema i seg selv — kalleren dispatcher på
 * feltnummer i en switch og velger riktig lese-metode selv. Ukjente
 * feltnumre må hoppes over med skip(wireType), slik proto3 krever for
 * fremover-kompatibilitet (nyere avsendere kan legge til felt vi ikke kjenner).
 */
export class ProtoReader {
  private offset = 0;

  constructor(private readonly data: Uint8Array) {}

  eof(): boolean {
    return this.offset >= this.data.length;
  }

  readTag(): FieldTag {
    const tag = Number(this.readVarintRaw());
    return { fieldNumber: tag >>> 3, wireType: tag & 0x7 };
  }

  readVarintRaw(): bigint {
    const { value, next } = decodeVarint(this.data, this.offset);
    this.offset = next;
    return value;
  }

  readInt32(): number {
    return toSignedInt32(this.readVarintRaw());
  }

  readUint32(): number {
    return toUnsignedInt32(this.readVarintRaw());
  }

  readSint32(): number {
    return zigzagDecode32(toUnsignedInt32(this.readVarintRaw()));
  }

  readBool(): boolean {
    return this.readVarintRaw() !== 0n;
  }

  readInt64(): bigint {
    return toSignedInt64(this.readVarintRaw());
  }

  readUint64(): bigint {
    return this.readVarintRaw();
  }

  readDouble(): number {
    const view = new DataView(this.data.buffer, this.data.byteOffset + this.offset, 8);
    this.offset += 8;
    return view.getFloat64(0, true);
  }

  readLengthDelimited(): Uint8Array {
    const length = Number(this.readVarintRaw());
    const slice = this.data.subarray(this.offset, this.offset + length);
    this.offset += length;
    return slice;
  }

  readString(): string {
    return new TextDecoder().decode(this.readLengthDelimited());
  }

  readPackedVarints(): number[] {
    const bytes = this.readLengthDelimited();
    const values: number[] = [];
    let pos = 0;
    while (pos < bytes.length) {
      const { value, next } = decodeVarint(bytes, pos);
      values.push(Number(value));
      pos = next;
    }
    return values;
  }

  skip(wireType: number): void {
    switch (wireType) {
      case WIRE_VARINT:
        this.readVarintRaw();
        return;
      case WIRE_64BIT:
        this.offset += 8;
        return;
      case WIRE_LENGTH_DELIMITED:
        this.readLengthDelimited();
        return;
      case WIRE_32BIT:
        this.offset += 4;
        return;
      default:
        throw new Error(`Unknown protobuf wire type: ${wireType}`);
    }
  }
}
