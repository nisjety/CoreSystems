import { zstdCompressSync } from 'node:zlib';
import { ProtoWriter } from '../../src/protocol/rustdesk/wire/ProtoWriter.js';

/**
 * Encoders for the messages the HOST sends — the direction remote-core only
 * ever decodes. They deliberately live here in the test tree rather than in
 * `src/`, for two reasons:
 *
 *  1. remote-core is always the connecting/controlling side, so shipping
 *     "encode a LoginResponse" in the library would be dead weight.
 *  2. Having the fake peer build its own messages makes the round-trip test
 *     meaningfully independent of the decode path it exercises.
 *
 * Field numbers are the verified ones from docs/rustdesk-protocol.md.
 */

export function encodeIdPk(id: string, pk: Uint8Array): Uint8Array {
  return new ProtoWriter().string(1, id).bytes(2, pk).finish();
}

export function encodeHashMessage(salt: string, challenge: string): Uint8Array {
  const hash = new ProtoWriter().string(1, salt).string(2, challenge).finish();
  return new ProtoWriter().message(9, hash).finish();
}

export function encodeSignedIdMessage(signed: Uint8Array): Uint8Array {
  const signedId = new ProtoWriter().bytes(1, signed).finish();
  return new ProtoWriter().message(3, signedId).finish();
}

export interface FakeDisplay {
  readonly name: string;
  readonly width: number;
  readonly height: number;
  readonly scale?: number;
  /** Origin in the host's global virtual-desktop coordinates. */
  readonly x?: number;
  readonly y?: number;
  readonly cursorEmbedded?: boolean;
}

export function encodeLoginResponseWithPeerInfo(options: {
  readonly hostname: string;
  readonly platform: string;
  readonly displays: readonly FakeDisplay[];
  readonly currentDisplay: number;
}): Uint8Array {
  const peerInfo = new ProtoWriter().string(1, 'host-user').string(2, options.hostname).string(3, options.platform);
  for (const display of options.displays) {
    const encoded = new ProtoWriter()
      .sint32(1, display.x ?? 0)
      .sint32(2, display.y ?? 0)
      .int32(3, display.width)
      .int32(4, display.height)
      .string(5, display.name)
      .bool(6, true)
      .bool(7, display.cursorEmbedded ?? false)
      .double(9, display.scale ?? 1)
      .finish();
    peerInfo.message(4, encoded);
  }
  peerInfo.int32(5, options.currentDisplay).bool(6, false).string(7, '1.4.9');

  const loginResponse = new ProtoWriter().message(2, peerInfo.finish()).bool(3, false).finish();
  return new ProtoWriter().message(8, loginResponse).finish();
}

export function encodeLoginResponseWithError(error: string): Uint8Array {
  const loginResponse = new ProtoWriter().string(1, error).finish();
  return new ProtoWriter().message(8, loginResponse).finish();
}

export function encodePermissionInfoMessage(permission: number, enabled: boolean): Uint8Array {
  const permissionInfo = new ProtoWriter().int32(1, permission).bool(2, enabled).finish();
  const misc = new ProtoWriter().message(6, permissionInfo).finish();
  return new ProtoWriter().message(19, misc).finish();
}

export function encodeClipboardMessage(text: string): Uint8Array {
  const clipboard = new ProtoWriter()
    .bool(1, false)
    .bytes(2, new TextEncoder().encode(text))
    .int32(5, 0) // ClipboardFormat.Text
    .finish();
  return new ProtoWriter().message(16, clipboard).finish();
}

/** VP9 is field 6 of the VideoFrame oneof. */
export function encodeVideoFrameMessage(options: {
  readonly data: Uint8Array;
  readonly key: boolean;
  readonly pts: bigint;
  readonly display: number;
}): Uint8Array {
  const encodedFrame = new ProtoWriter()
    .bytes(1, options.data)
    .bool(2, options.key)
    .int64(3, options.pts)
    .finish();
  const frames = new ProtoWriter().message(1, encodedFrame).finish();
  const videoFrame = new ProtoWriter().message(6, frames).int32(14, options.display).finish();
  return new ProtoWriter().message(6, videoFrame).finish();
}

export function encodePunchHoleResponse(options: {
  readonly relayServer: string;
  readonly pk: Uint8Array;
}): Uint8Array {
  const response = new ProtoWriter().bytes(2, options.pk).string(4, options.relayServer).finish();
  return new ProtoWriter().message(11, response).finish();
}

export function encodePunchHoleFailure(failure: number, otherFailure = ''): Uint8Array {
  const response = new ProtoWriter().int32(3, failure).string(7, otherFailure).finish();
  return new ProtoWriter().message(11, response).finish();
}

export function encodeRelayResponse(options: {
  readonly uuid: string;
  readonly relayServer: string;
  readonly pk: Uint8Array;
}): Uint8Array {
  const response = new ProtoWriter()
    .string(2, options.uuid)
    .string(3, options.relayServer)
    .bytes(5, options.pk)
    .finish();
  return new ProtoWriter().message(19, response).finish();
}

export function encodeRelayRefusal(reason: string): Uint8Array {
  const response = new ProtoWriter().string(6, reason).finish();
  return new ProtoWriter().message(19, response).finish();
}

/** Host-originated probe (from_client=false); `lastDelay` is the host's previous RTT measurement. */
export function encodeTestDelayMessage(options: {
  readonly time: bigint;
  readonly fromClient: boolean;
  readonly lastDelay: number;
  /** kbps the host's encoder is currently targeting — host-set only. */
  readonly targetBitrate?: number;
}): Uint8Array {
  const testDelay = new ProtoWriter()
    .int64(1, options.time)
    .bool(2, options.fromClient)
    .uint32(3, options.lastDelay)
    .uint32(4, options.targetBitrate ?? 0)
    .finish();
  return new ProtoWriter().message(5, testDelay).finish();
}

/** Host confirmation of a display switch: Misc.switch_display (5) carrying the new geometry. */
export function encodeSwitchDisplayMessage(options: {
  readonly display: number;
  readonly x?: number;
  readonly y?: number;
  readonly width: number;
  readonly height: number;
}): Uint8Array {
  const switchDisplay = new ProtoWriter()
    .int32(1, options.display)
    .sint32(2, options.x ?? 0)
    .sint32(3, options.y ?? 0)
    .int32(4, options.width)
    .int32(5, options.height)
    .finish();
  const misc = new ProtoWriter().message(5, switchDisplay).finish();
  return new ProtoWriter().message(19, misc).finish();
}

/**
 * A full cursor shape. `colors` is zstd-compressed exactly as the host does
 * it (no flag, no negotiation) — Node's built-in zstd stands in for the host's
 * compressor here so the client's fzstd path is exercised for real.
 */
export function encodeCursorDataMessage(options: {
  readonly id: bigint;
  readonly hotx: number;
  readonly hoty: number;
  readonly width: number;
  readonly height: number;
  readonly rgba: Uint8Array;
  /** Test hook: send these bytes verbatim instead of compressing `rgba`. */
  readonly rawColors?: Uint8Array;
}): Uint8Array {
  const colors = options.rawColors ?? new Uint8Array(zstdCompressSync(options.rgba));
  const cursorData = new ProtoWriter()
    .uint64(1, options.id)
    .sint32(2, options.hotx)
    .sint32(3, options.hoty)
    .int32(4, options.width)
    .int32(5, options.height)
    .bytes(6, colors)
    .finish();
  return new ProtoWriter().message(12, cursorData).finish();
}

export function encodeCursorPositionMessage(x: number, y: number): Uint8Array {
  const position = new ProtoWriter().sint32(1, x).sint32(2, y).finish();
  return new ProtoWriter().message(13, position).finish();
}

/** `cursor_id` is a bare uint64 scalar in the Message oneof (field 14). */
export function encodeCursorIdMessage(id: bigint): Uint8Array {
  return new ProtoWriter().uint64(14, id).finish();
}
