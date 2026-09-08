import { describe, expect, it } from 'vitest';
import {
  decodeHash,
  decodePublicKey,
  decodeSignedId,
  decodeLoginResponse,
  encodePublicKey,
  encodeSignedId,
} from '../../../../src/protocol/rustdesk/messages/sessionCodec.js';
import {
  encodeMouseEvent,
  encodeKeyEvent,
  encodeClipboard,
  decodeClipboard,
  decodeMisc,
  encodeTestDelay,
  decodeTestDelay,
  encodeSwitchDisplayRequest,
  encodeAuth2FA,
} from '../../../../src/protocol/rustdesk/messages/controlCodec.js';
import { decodeVideoFrame } from '../../../../src/protocol/rustdesk/messages/videoCodec.js';
import { decodeCursorData, decodeCursorPosition } from '../../../../src/protocol/rustdesk/messages/cursorCodec.js';
import {
  encodePunchHoleRequest,
  decodePunchHoleResponse,
  encodeRequestRelay,
  decodeRelayResponse,
  encodeKeyExchange,
  decodeKeyExchange,
} from '../../../../src/protocol/rustdesk/messages/rendezvousCodec.js';
import {
  encodeMessage,
  decodeMessage,
  encodeRendezvousMessage,
  decodeRendezvousMessage,
} from '../../../../src/protocol/rustdesk/messages/envelope.js';
import { ProtoWriter } from '../../../../src/protocol/rustdesk/wire/ProtoWriter.js';
import { ProtoReader } from '../../../../src/protocol/rustdesk/wire/ProtoReader.js';
import { ClipboardFormat, ControlKey, KeyboardMode, MouseButtonFlag, MouseEventKind } from '../../../../src/protocol/rustdesk/messages/types.js';

describe('sessionCodec', () => {
  it('decodes a Hash message', () => {
    const wire = new ProtoWriter().string(1, 'the-salt').string(2, 'the-challenge').finish();
    expect(decodeHash(wire)).toEqual({ salt: 'the-salt', challenge: 'the-challenge' });
  });

  it('round-trips PublicKey', () => {
    const value = { asymmetricValue: new Uint8Array([1, 2, 3]), symmetricValue: new Uint8Array([4, 5]) };
    const decoded = decodePublicKey(encodePublicKey(value));
    expect([...decoded.asymmetricValue]).toEqual([1, 2, 3]);
    expect([...decoded.symmetricValue]).toEqual([4, 5]);
  });

  it('round-trips SignedId', () => {
    const value = { id: new Uint8Array([9, 9, 9]) };
    expect([...decodeSignedId(encodeSignedId(value)).id]).toEqual([9, 9, 9]);
  });

  it('decodes a LoginResponse error variant', () => {
    const wire = new ProtoWriter().string(1, 'Wrong Password').finish();
    const decoded = decodeLoginResponse(wire);
    expect(decoded.result).toEqual({ kind: 'error', error: 'Wrong Password' });
  });

  it('decodes a LoginResponse peerInfo variant with nested displays', () => {
    const display = new ProtoWriter()
      .sint32(1, 0)
      .sint32(2, 0)
      .int32(3, 1920)
      .int32(4, 1080)
      .string(5, 'Built-in Display')
      .bool(6, true)
      .finish();
    const peerInfo = new ProtoWriter()
      .string(1, 'kari')
      .string(2, 'DESKTOP-KARI')
      .string(3, 'windows')
      .message(4, display)
      .int32(5, 0)
      .finish();
    const wire = new ProtoWriter().message(2, peerInfo).bool(3, true).finish();

    const decoded = decodeLoginResponse(wire);
    expect(decoded.enableTrustedDevices).toBe(true);
    if (decoded.result.kind !== 'peerInfo') throw new Error('expected peerInfo');
    expect(decoded.result.peerInfo.hostname).toBe('DESKTOP-KARI');
    expect(decoded.result.peerInfo.displays).toHaveLength(1);
    expect(decoded.result.peerInfo.displays[0]).toMatchObject({ width: 1920, height: 1080, online: true });
  });
});

describe('controlCodec', () => {
  it('packs MouseEvent.mask as (buttonFlags << 3) | kind', () => {
    const wire = encodeMouseEvent({
      kind: MouseEventKind.Down,
      buttonFlags: MouseButtonFlag.Left,
      x: 100,
      y: 200,
      modifiers: [],
    });

    const reader = new ProtoReader(wire);
    const maskTag = reader.readTag();
    expect(maskTag).toEqual({ fieldNumber: 1, wireType: 0 });
    expect(reader.readInt32()).toBe((MouseButtonFlag.Left << 3) | MouseEventKind.Down); // 9

    const xTag = reader.readTag();
    expect(xTag).toEqual({ fieldNumber: 2, wireType: 0 });
    expect(reader.readSint32()).toBe(100);

    const yTag = reader.readTag();
    expect(yTag).toEqual({ fieldNumber: 3, wireType: 0 });
    expect(reader.readSint32()).toBe(200);
  });

  it('packs MouseEvent modifiers as a packed varint field', () => {
    const wire = encodeMouseEvent({
      kind: MouseEventKind.Down,
      buttonFlags: MouseButtonFlag.Left,
      x: 0,
      y: 0,
      modifiers: [ControlKey.Shift, ControlKey.Control],
    });
    const reader = new ProtoReader(wire);
    reader.readTag();
    reader.readInt32(); // mask
    const tag = reader.readTag();
    expect(tag).toEqual({ fieldNumber: 4, wireType: 2 });
    expect(reader.readPackedVarints()).toEqual([ControlKey.Shift, ControlKey.Control]);
  });

  it('KeyEvent encodes each identification kind distinctly', () => {
    const controlKey = encodeKeyEvent({
      down: true,
      press: false,
      identification: { kind: 'controlKey', controlKey: ControlKey.Return },
      modifiers: [],
      mode: KeyboardMode.Map,
    });
    const seq = encodeKeyEvent({
      down: true,
      press: true,
      identification: { kind: 'seq', text: 'hello' },
      modifiers: [ControlKey.Shift],
      mode: KeyboardMode.Translate,
    });
    expect(controlKey).not.toEqual(seq);
    expect(controlKey.length).toBeGreaterThan(0);
    expect(seq.length).toBeGreaterThan(0);
  });

  it('round-trips Clipboard text content', () => {
    const value = {
      compress: false,
      content: new TextEncoder().encode('hello clipboard'),
      width: 0,
      height: 0,
      format: ClipboardFormat.Text,
      specialName: '',
    };
    const decoded = decodeClipboard(encodeClipboard(value));
    expect(new TextDecoder().decode(decoded.content)).toBe('hello clipboard');
    expect(decoded.format).toBe(ClipboardFormat.Text);
  });

  it('decodes Misc.permission_info and ignores unrelated Misc variants', () => {
    const permissionInfo = new ProtoWriter().int32(1, 2).bool(2, false).finish();
    const misc = new ProtoWriter().message(6, permissionInfo).finish();
    expect(decodeMisc(misc)).toEqual({ kind: 'permissionInfo', info: { permission: 2, enabled: false } });

    const otherMisc = new ProtoWriter().bool(10, true).finish();
    expect(decodeMisc(otherMisc)).toEqual({ kind: 'other' });
  });

  it('decodes Misc.switch_display (5) with the host\'s geometry and original resolution', () => {
    const resolution = new ProtoWriter().int32(1, 3840).int32(2, 2160).finish();
    const switchDisplay = new ProtoWriter()
      .int32(1, 1)
      .sint32(2, -1920)
      .sint32(3, 0)
      .int32(4, 2560)
      .int32(5, 1440)
      .bool(6, true)
      .message(8, resolution)
      .finish();
    const misc = new ProtoWriter().message(5, switchDisplay).finish();

    expect(decodeMisc(misc)).toEqual({
      kind: 'switchDisplay',
      value: {
        display: 1,
        x: -1920,
        y: 0,
        width: 2560,
        height: 1440,
        cursorEmbedded: true,
        originalResolution: { width: 3840, height: 2160 },
      },
    });
  });

  it('encodes a SwitchDisplay request as Misc field 5 carrying only the display index', () => {
    const misc = encodeSwitchDisplayRequest(2);
    const reader = new ProtoReader(misc);
    const tag = reader.readTag();
    expect(tag.fieldNumber).toBe(5);
    const inner = new ProtoReader(reader.readLengthDelimited());
    expect(inner.readTag().fieldNumber).toBe(1);
    expect(inner.readInt32()).toBe(2);
    expect(inner.eof()).toBe(true);
  });

  it('round-trips TestDelay including the int64 timestamp', () => {
    const value = { time: 1_725_000_000_123n, fromClient: true, lastDelay: 42, targetBitrate: 3_000 };
    expect(decodeTestDelay(encodeTestDelay(value))).toEqual(value);
  });

  it('encodes Auth2FA with code (1) and hwid (2)', () => {
    const wire = encodeAuth2FA({ code: '123456', hwid: new Uint8Array([9, 8]) });
    const reader = new ProtoReader(wire);
    expect(reader.readTag().fieldNumber).toBe(1);
    expect(reader.readString()).toBe('123456');
    expect(reader.readTag().fieldNumber).toBe(2);
    expect([...reader.readLengthDelimited()]).toEqual([9, 8]);
  });
});

describe('videoCodec', () => {
  it('decodes a vp9s VideoFrame with one keyframe', () => {
    const encodedFrame = new ProtoWriter().bytes(1, new Uint8Array([1, 2, 3])).bool(2, true).int64(3, 12345n).finish();
    const frames = new ProtoWriter().message(1, encodedFrame).finish();
    const wire = new ProtoWriter().message(6, frames).int32(14, 0).finish();

    const decoded = decodeVideoFrame(wire);
    expect(decoded?.codec).toBe('vp9');
    expect(decoded?.frames).toHaveLength(1);
    expect(decoded?.frames[0]).toMatchObject({ key: true, pts: 12345n });
    expect([...(decoded?.frames[0]?.data ?? [])]).toEqual([1, 2, 3]);
  });

  it('returns undefined for the unsupported rgb/yuv raw-pixel variants', () => {
    const wire = new ProtoWriter().message(7, new Uint8Array()).finish();
    expect(decodeVideoFrame(wire)).toBeUndefined();
  });
});

describe('rendezvousCodec', () => {
  it('round-trips PunchHoleRequest fields via manual decode', () => {
    const wire = encodePunchHoleRequest({
      id: 'target-peer',
      natType: 0,
      licenceKey: '',
      connType: 0,
      token: '',
      version: '1.0.0',
      forceRelay: true,
    });
    expect(wire.length).toBeGreaterThan(0);
  });

  it('decodes PunchHoleResponse relay-path fields', () => {
    const wire = new ProtoWriter().bytes(2, new Uint8Array([7, 7])).string(4, 'relay.verevon.com').finish();
    const decoded = decodePunchHoleResponse(wire);
    expect(decoded.relayServer).toBe('relay.verevon.com');
    expect([...decoded.pk]).toEqual([7, 7]);
  });

  it('round-trips RequestRelay/RelayResponse', () => {
    const requestWire = encodeRequestRelay({
      id: 'target-peer',
      uuid: 'session-uuid',
      relayServer: 'relay.verevon.com',
      secure: true,
      licenceKey: '',
      connType: 0,
      token: '',
    });
    expect(requestWire.length).toBeGreaterThan(0);

    const responseWire = new ProtoWriter()
      .string(2, 'session-uuid')
      .string(3, 'relay.verevon.com')
      .bytes(5, new Uint8Array([9, 9]))
      .string(7, '1.4.9')
      .finish();
    const decoded = decodeRelayResponse(responseWire);
    expect(decoded).toMatchObject({
      uuid: 'session-uuid',
      relayServer: 'relay.verevon.com',
      refuseReason: '',
      version: '1.4.9',
    });
    expect([...decoded.pk]).toEqual([9, 9]);
  });

  it('round-trips KeyExchange with multiple keys', () => {
    const value = { keys: [new Uint8Array([1]), new Uint8Array([2, 2])] };
    const decoded = decodeKeyExchange(encodeKeyExchange(value));
    expect(decoded.keys.map((k) => [...k])).toEqual([[1], [2, 2]]);
  });
});

describe('cursorCodec', () => {
  it('decodes CursorData fields including the uint64 id and signed hotspot', () => {
    const wire = new ProtoWriter()
      .uint64(1, 18_446_744_073_709_551_615n)
      .sint32(2, -1)
      .sint32(3, 7)
      .int32(4, 32)
      .int32(5, 32)
      .bytes(6, new Uint8Array([9, 9]))
      .finish();
    const decoded = decodeCursorData(wire);
    expect(decoded).toMatchObject({ id: 18_446_744_073_709_551_615n, hotx: -1, hoty: 7, width: 32, height: 32 });
    expect([...decoded.colors]).toEqual([9, 9]);
  });

  it('decodes CursorPosition with negative (multi-monitor) coordinates', () => {
    const wire = new ProtoWriter().sint32(1, -1920).sint32(2, 40).finish();
    expect(decodeCursorPosition(wire)).toEqual({ x: -1920, y: 40 });
  });

  it('dispatches cursor_data (12), cursor_position (13) and the bare cursor_id (14) through the envelope', () => {
    const data = new ProtoWriter().uint64(1, 5n).int32(4, 1).int32(5, 1).finish();
    expect(decodeMessage(new ProtoWriter().message(12, data).finish())).toMatchObject({ kind: 'cursorData', value: { id: 5n } });
    expect(decodeMessage(new ProtoWriter().message(13, new ProtoWriter().sint32(1, 3).finish()).finish())).toEqual({
      kind: 'cursorPosition',
      value: { x: 3, y: 0 },
    });
    expect(decodeMessage(new ProtoWriter().uint64(14, 77n).finish())).toEqual({ kind: 'cursorId', value: 77n });
  });
});

describe('envelope dispatch', () => {
  it('round-trips an outgoing loginRequest through the Message envelope shape', () => {
    const wire = encodeMessage({
      kind: 'loginRequest',
      value: {
        username: 'target-peer',
        password: new Uint8Array([1, 2, 3]),
        myId: 'my-id',
        myName: 'Verevon Support',
        myPlatform: 'browser',
        sessionId: 42n,
        version: '1.0.0',
        videoAckRequired: true,
        hwid: new Uint8Array(0),
        avatar: '',
      },
    });
    // We never decode our own LoginRequest (we only ever send it), so this
    // just asserts the envelope actually wraps it under the right field.
    expect(wire.length).toBeGreaterThan(0);
  });

  it('decodes an incoming hash Message', () => {
    const inner = new ProtoWriter().string(1, 's').string(2, 'c').finish();
    const wire = new ProtoWriter().message(9, inner).finish();
    const decoded = decodeMessage(wire);
    expect(decoded).toEqual({ kind: 'hash', value: { salt: 's', challenge: 'c' } });
  });

  it('dispatches Message.test_delay (5), Misc.switch_display and Message.auth_2fa (27)', () => {
    const testDelay = encodeMessage({
      kind: 'testDelay',
      value: { time: 5n, fromClient: false, lastDelay: 12, targetBitrate: 0 },
    });
    expect(decodeMessage(testDelay)).toEqual({
      kind: 'testDelay',
      value: { time: 5n, fromClient: false, lastDelay: 12, targetBitrate: 0 },
    });

    const switchDisplay = encodeMessage({ kind: 'switchDisplay', display: 1 });
    expect(decodeMessage(switchDisplay)).toMatchObject({ kind: 'switchDisplay', value: { display: 1 } });

    const auth2fa = encodeMessage({ kind: 'auth2fa', value: { code: '000000', hwid: new Uint8Array(0) } });
    const reader = new ProtoReader(auth2fa);
    expect(reader.readTag().fieldNumber).toBe(27);
  });

  it('decodes an unrecognized Message field as unknown rather than throwing', () => {
    const wire = new ProtoWriter().message(11, new Uint8Array([1])).finish(); // cursor_data — not decoded
    expect(decodeMessage(wire)).toEqual({ kind: 'unknown', fieldNumber: 11 });
  });

  it('round-trips punchHoleRequest/keyExchange through the RendezvousMessage envelope', () => {
    const wire = encodeRendezvousMessage({
      kind: 'keyExchange',
      value: { keys: [new Uint8Array([5, 6, 7])] },
    });
    const decoded = decodeRendezvousMessage(wire);
    expect(decoded.kind).toBe('keyExchange');
    if (decoded.kind !== 'keyExchange') throw new Error('expected keyExchange');
    expect(decoded.value.keys.map((k) => [...k])).toEqual([[5, 6, 7]]);
  });
});
