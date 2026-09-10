import { ProtoWriter } from '../wire/ProtoWriter.js';
import { ProtoReader } from '../wire/ProtoReader.js';
import type { RdKeyExchange, RdPunchHoleRequest, RdPunchHoleResponse, RdRelayResponse, RdRequestRelay } from './types.js';

/** Feltnummer for `RendezvousMessage`-konvoluttens `oneof union` (rendezvous.proto). Kun det vi faktisk sender/mottar. */
export const RendezvousField = {
  PunchHoleRequest: 8,
  PunchHoleResponse: 11,
  RequestRelay: 18,
  RelayResponse: 19,
  KeyExchange: 25,
} as const;

// ---------- PunchHoleRequest (sendes) ----------

export function encodePunchHoleRequest(value: RdPunchHoleRequest): Uint8Array {
  return new ProtoWriter()
    .string(1, value.id)
    .enum(2, value.natType)
    .string(3, value.licenceKey)
    .enum(4, value.connType)
    .string(5, value.token)
    .string(6, value.version)
    .bool(8, value.forceRelay)
    .finish();
}

// ---------- PunchHoleResponse (mottas) ----------

export function decodePunchHoleResponse(data: Uint8Array): RdPunchHoleResponse {
  const reader = new ProtoReader(data);
  let relayServer = '';
  let pk: Uint8Array = new Uint8Array(0);
  let failure = 0;
  let otherFailure = '';
  while (!reader.eof()) {
    const tag = reader.readTag();
    switch (tag.fieldNumber) {
      case 2:
        pk = reader.readLengthDelimited();
        break;
      case 3:
        failure = reader.readInt32();
        break;
      case 4:
        relayServer = reader.readString();
        break;
      case 7:
        otherFailure = reader.readString();
        break;
      default:
        reader.skip(tag.wireType);
    }
  }
  return { relayServer, pk, failure, otherFailure };
}

// ---------- RequestRelay (sendes) ----------

export function encodeRequestRelay(value: RdRequestRelay): Uint8Array {
  return new ProtoWriter()
    .string(1, value.id)
    .string(2, value.uuid)
    .string(4, value.relayServer)
    .bool(5, value.secure)
    .string(6, value.licenceKey)
    .enum(7, value.connType)
    .string(8, value.token)
    .finish();
}

// ---------- RelayResponse (mottas) ----------

export function decodeRelayResponse(data: Uint8Array): RdRelayResponse {
  const reader = new ProtoReader(data);
  let uuid = '';
  let relayServer = '';
  let peerId = '';
  let pk: Uint8Array = new Uint8Array(0);
  let refuseReason = '';
  let version = '';
  while (!reader.eof()) {
    const tag = reader.readTag();
    switch (tag.fieldNumber) {
      case 2:
        uuid = reader.readString();
        break;
      case 3:
        relayServer = reader.readString();
        break;
      // Felt 4 (id) og 5 (pk) er to grener av samme oneof — bare én er satt.
      case 4:
        peerId = reader.readString();
        break;
      case 5:
        pk = reader.readLengthDelimited();
        break;
      case 6:
        refuseReason = reader.readString();
        break;
      case 7:
        version = reader.readString();
        break;
      default:
        reader.skip(tag.wireType);
    }
  }
  return { uuid, relayServer, peerId, pk, refuseReason, version };
}

// ---------- KeyExchange (sendes og mottas) ----------

export function encodeKeyExchange(value: RdKeyExchange): Uint8Array {
  const writer = new ProtoWriter();
  for (const key of value.keys) writer.bytes(1, key);
  return writer.finish();
}

export function decodeKeyExchange(data: Uint8Array): RdKeyExchange {
  const reader = new ProtoReader(data);
  const keys: Uint8Array[] = [];
  while (!reader.eof()) {
    const tag = reader.readTag();
    if (tag.fieldNumber === 1) keys.push(reader.readLengthDelimited());
    else reader.skip(tag.wireType);
  }
  return { keys };
}
