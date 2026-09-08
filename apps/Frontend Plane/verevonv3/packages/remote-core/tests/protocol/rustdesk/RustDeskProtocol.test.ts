import { describe, expect, it, vi } from 'vitest';
import { RustDeskProtocol } from '../../../src/protocol/rustdesk/RustDeskProtocol.js';
import { RustDeskPasswordAuthenticator } from '../../../src/protocol/rustdesk/RustDeskPasswordAuthenticator.js';
import { AuthenticationError, EncryptionError, RemoteConnectionError } from '../../../src/errors/RemoteError.js';
import { MockTransport } from '../../doubles/MockTransport.js';
import { FakeRustDeskPeer } from '../../doubles/FakeRustDeskPeer.js';
import {
  encodeClipboardMessage,
  encodePermissionInfoMessage,
  encodePunchHoleFailure,
  encodePunchHoleResponse,
  encodeRelayRefusal,
  encodeRelayResponse,
  encodeVideoFrameMessage,
  encodeSwitchDisplayMessage,
  encodeTestDelayMessage,
  encodeCursorDataMessage,
  encodeCursorIdMessage,
  encodeCursorPositionMessage,
  type FakeDisplay,
} from '../../doubles/peerEncoders.js';
import { decodePeerRendezvousMessage } from '../../doubles/peerDecoders.js';
import { Permission, PunchHoleFailure } from '../../../src/protocol/rustdesk/messages/types.js';

const DEVICE_ID = '123456789';
const SECRET = 'verevon-session-token';
const RENDEZVOUS_URL = 'wss://rendezvous.test';
const RELAY_URL = 'wss://relay.test';

interface Harness {
  readonly peer: FakeRustDeskPeer;
  readonly protocol: RustDeskProtocol;
  readonly rendezvous: MockTransport;
  readonly relay: MockTransport;
}

type RendezvousBehavior = 'punchHole' | 'relayResponse' | 'offline' | 'refuseRelay';

async function harness(
  options: {
    readonly behavior?: RendezvousBehavior;
    readonly serverPublicKey?: string | null;
    readonly allowUnverifiedPeer?: boolean;
    readonly assumeInputPermittedOnLogin?: boolean;
    readonly vouch?: Uint8Array;
    readonly require2FACode?: string;
    readonly displays?: readonly FakeDisplay[];
    /** Defaults to 0 here so ordinary tests never leave a probe timer running. */
    readonly latencyProbeIntervalMs?: number;
    readonly secondFactorTimeoutMs?: number;
    readonly videoCodecs?: readonly ('vp8' | 'vp9' | 'av1' | 'h264' | 'h265')[];
    readonly preferredCodec?: 'auto' | 'vp9' | 'h264';
  } = {},
): Promise<Harness> {
  const peer = await FakeRustDeskPeer.create({
    deviceId: DEVICE_ID,
    secret: SECRET,
    require2FACode: options.require2FACode,
    displays: options.displays,
  });
  const rendezvous = new MockTransport();
  const relay = new MockTransport();
  const behavior = options.behavior ?? 'punchHole';
  const vouch = options.vouch ?? peer.vouch;

  rendezvous.onSend = (data) => {
    const message = decodePeerRendezvousMessage(data);
    if (message.kind !== 'punchHoleRequest') return;
    switch (behavior) {
      case 'punchHole':
        rendezvous.deliver(encodePunchHoleResponse({ relayServer: RELAY_URL, pk: vouch }));
        return;
      case 'relayResponse':
        rendezvous.deliver(encodeRelayResponse({ uuid: 'server-chosen-uuid', relayServer: RELAY_URL, pk: vouch }));
        return;
      case 'offline':
        rendezvous.deliver(encodePunchHoleFailure(PunchHoleFailure.Offline));
        return;
      case 'refuseRelay':
        rendezvous.deliver(encodeRelayRefusal('license expired'));
        return;
    }
  };

  peer.attachRelay(relay);

  const serverPublicKey =
    options.serverPublicKey === null ? undefined : (options.serverPublicKey ?? peer.serverPublicKeyBase64);

  const protocol = new RustDeskProtocol({
    rendezvousUrl: RENDEZVOUS_URL,
    relayUrl: RELAY_URL,
    serverPublicKey,
    allowUnverifiedPeer: options.allowUnverifiedPeer,
    assumeInputPermittedOnLogin: options.assumeInputPermittedOnLogin,
    latencyProbeIntervalMs: options.latencyProbeIntervalMs ?? 0,
    secondFactorTimeoutMs: options.secondFactorTimeoutMs,
    preferredCodec: options.preferredCodec,
    // Node has no WebCodecs, so the browser probe would report nothing.
    mediaCapabilities: {
      webCodecsAvailable: true,
      videoCodecs: options.videoCodecs ?? ['vp8', 'vp9', 'av1'],
    },
    createTransport: (url) => (url === RENDEZVOUS_URL ? rendezvous : relay),
  });

  return { peer, protocol, rendezvous, relay };
}

function connect(
  protocol: RustDeskProtocol,
  secret = SECRET,
  secondFactor?: () => Promise<string>,
): Promise<void> {
  return protocol.connect({
    deviceId: DEVICE_ID,
    authenticator: new RustDeskPasswordAuthenticator(secret, { secondFactor }),
  });
}

describe('RustDeskProtocol — full handshake against a fake peer', () => {
  it('completes rendezvous, relay pairing, key exchange and login', async () => {
    const { peer, protocol, rendezvous, relay } = await harness();

    await connect(protocol);
    await peer.settle();

    expect(peer.loginPasswordMatched).toBe(true);
    expect(protocol.displays).toEqual([
      {
        id: '0',
        label: 'Built-in Display',
        width: 1920,
        height: 1080,
        isPrimary: true,
        scaleFactor: 1,
        cursorEmbedded: false,
      },
    ]);
    // Login proves we can see the screen; control permissions must arrive
    // separately, never be inferred from it.
    expect(protocol.permissions).toEqual(['screen.view']);

    // The rendezvous request must ask for relay explicitly — a browser can
    // never do UDP hole punching.
    const punchHole = decodePeerRendezvousMessage(rendezvous.sent[0] ?? new Uint8Array());
    expect(punchHole).toMatchObject({ kind: 'punchHoleRequest', id: DEVICE_ID, forceRelay: true });

    // First thing on the relay must be the pairing token.
    const pairing = decodePeerRendezvousMessage(relay.sent[0] ?? new Uint8Array());
    expect(pairing).toMatchObject({ kind: 'requestRelay', id: DEVICE_ID });

    expect(rendezvous.closeCalls).toBe(1);
  });

  it('works when the server answers with a RelayResponse instead of a PunchHoleResponse', async () => {
    const { peer, protocol, relay } = await harness({ behavior: 'relayResponse' });

    await connect(protocol);
    await peer.settle();

    expect(peer.loginPasswordMatched).toBe(true);
    // The server-chosen uuid must be the one we present to hbbr.
    const pairing = decodePeerRendezvousMessage(relay.sent[0] ?? new Uint8Array());
    expect(pairing).toMatchObject({ kind: 'requestRelay', uuid: 'server-chosen-uuid' });
  });

  it('rejects a wrong session token with AuthenticationError', async () => {
    const { peer, protocol } = await harness();

    await expect(connect(protocol, 'not-the-right-token')).rejects.toThrow(AuthenticationError);
    await peer.settle();
    expect(peer.loginPasswordMatched).toBe(false);
  });

  it('completes two-factor login when the authenticator can supply a code', async () => {
    const { peer, protocol } = await harness({ require2FACode: '123456' });
    const secondFactor = vi.fn(async () => '123456');

    await connect(protocol, SECRET, secondFactor);
    await peer.settle();

    // The code is only requested AFTER the password has been accepted.
    expect(peer.loginPasswordMatched).toBe(true);
    expect(secondFactor).toHaveBeenCalledTimes(1);
    expect(peer.received.find((message) => message.kind === 'auth2fa')).toMatchObject({ code: '123456' });
    expect(protocol.displays).toHaveLength(1);
  });

  it('rejects a wrong second-factor code with a typed authentication error', async () => {
    const { protocol } = await harness({ require2FACode: '123456' });

    // One attempt only: the fake peer's encrypted channel is single-use.
    const error = await connect(protocol, SECRET, async () => '000000').catch((caught: unknown) => caught);
    expect(error).toBeInstanceOf(AuthenticationError);
    expect((error as Error).message).toMatch(/two-factor code was rejected/i);
  });

  it('fails clearly when the host requires 2FA and no second-factor provider was configured', async () => {
    const { peer, protocol } = await harness({ require2FACode: '123456' });

    await expect(connect(protocol)).rejects.toThrow(AuthenticationError);
    await peer.settle();
    // Nothing was sent that pretends to be a code.
    expect(peer.received.some((message) => message.kind === 'auth2fa')).toBe(false);
  });

  it('refuses a peer whose vouch was signed by the wrong key', async () => {
    const forged = await FakeRustDeskPeer.forgedVouch(DEVICE_ID);
    const { protocol } = await harness({ vouch: forged });

    await expect(connect(protocol)).rejects.toThrow(EncryptionError);
  });

  it('fails closed when no server key is configured to verify the peer', async () => {
    const { protocol } = await harness({ serverPublicKey: null });

    // RustDesk's own client would silently continue unauthenticated here;
    // docs/security.md requires us to refuse instead.
    await expect(connect(protocol)).rejects.toThrow(/unauthenticated handshake/i);
  });

  it('allows an unverified peer only behind the explicit development flag', async () => {
    const { peer, protocol } = await harness({ serverPublicKey: null, allowUnverifiedPeer: true });

    await connect(protocol);
    await peer.settle();

    expect(peer.loginPasswordMatched).toBe(true);
  });

  it('treats a completely EMPTY PunchHoleResponse as "unknown device ID"', async () => {
    // Observed from a real hbbs: an unknown ID gets literally `5a 00` — an
    // empty PunchHoleResponse, relying on proto3's default failure=0
    // (ID_NOT_EXIST). Nothing distinguishes it from "field absent", which is
    // why RendezvousClient keys off empty relayServer+pk instead of `failure`.
    const peer = await FakeRustDeskPeer.create({ deviceId: DEVICE_ID, secret: SECRET });
    const rendezvous = new MockTransport();
    const relay = new MockTransport();
    rendezvous.onSend = () => rendezvous.deliver(new Uint8Array([0x5a, 0x00]));

    const protocol = new RustDeskProtocol({
      rendezvousUrl: RENDEZVOUS_URL,
      relayUrl: RELAY_URL,
      serverPublicKey: peer.serverPublicKeyBase64,
      createTransport: (url) => (url === RENDEZVOUS_URL ? rendezvous : relay),
    });

    await expect(connect(protocol)).rejects.toThrow(/does not know that device ID/i);
  });

  it('falls back to serverPublicKey as the licence key when none is given', async () => {
    // A real hbbs answers failure=3 (LICENSE_MISMATCH) when the access key is
    // missing, and that key is the same string as the server public key — so
    // omitting licenceKey must not silently produce a mismatch.
    const peer = await FakeRustDeskPeer.create({ deviceId: DEVICE_ID, secret: SECRET });
    const rendezvous = new MockTransport();
    const relay = new MockTransport();
    let sentLicenceKey: string | undefined;

    rendezvous.onSend = (data) => {
      const message = decodePeerRendezvousMessage(data);
      if (message.kind !== 'punchHoleRequest') return;
      sentLicenceKey = message.licenceKey;
      rendezvous.deliver(encodePunchHoleResponse({ relayServer: RELAY_URL, pk: peer.vouch }));
    };
    peer.attachRelay(relay);

    const protocol = new RustDeskProtocol({
      rendezvousUrl: RENDEZVOUS_URL,
      relayUrl: RELAY_URL,
      serverPublicKey: peer.serverPublicKeyBase64,
      // licenceKey deliberately omitted
      createTransport: (url) => (url === RENDEZVOUS_URL ? rendezvous : relay),
    });

    await connect(protocol);
    expect(sentLicenceKey).toBe(peer.serverPublicKeyBase64);
  });

  it('reports an offline device as a connection error, not an auth error', async () => {
    const { protocol } = await harness({ behavior: 'offline' });

    await expect(connect(protocol)).rejects.toThrow(RemoteConnectionError);
    await expect(connect(protocol)).rejects.toThrow(/offline/i);
  });

  it('reports a refused relay with the server reason', async () => {
    const { protocol } = await harness({ behavior: 'refuseRelay' });

    await expect(connect(protocol)).rejects.toThrow(/license expired/i);
  });

  it('grants input permissions up front only when explicitly configured', async () => {
    const { peer, protocol } = await harness({ assumeInputPermittedOnLogin: true });

    await connect(protocol);
    await peer.settle();

    expect([...protocol.permissions].sort()).toEqual(['input.keyboard', 'input.pointer', 'screen.view']);
  });
});

describe('RustDeskProtocol — session traffic', () => {
  it('encodes pointer actions the peer can decode, in order', async () => {
    const { peer, protocol } = await harness();
    await connect(protocol);

    await protocol.sendAction({ type: 'pointer.move', actor: 'human', x: 100, y: 200 });
    await protocol.sendAction({ type: 'pointer.click', actor: 'human', x: 100, y: 200, button: 'left' });
    await peer.settle();

    const mouseEvents = peer.received.filter((message) => message.kind === 'mouseEvent');
    // move, then click = down + up
    expect(mouseEvents).toHaveLength(3);
    expect(mouseEvents[0]).toMatchObject({ mask: 0, x: 100, y: 200 }); // Move
    expect(mouseEvents[1]).toMatchObject({ mask: (0x01 << 3) | 1, x: 100, y: 200 }); // Left down
    expect(mouseEvents[2]).toMatchObject({ mask: (0x01 << 3) | 2, x: 100, y: 200 }); // Left up
  });

  it('expresses a drag as down → move → up so the host can move windows and select text', async () => {
    const { peer, protocol } = await harness();
    await connect(protocol);

    await protocol.sendAction({ type: 'pointer.down', actor: 'human', x: 10, y: 10, button: 'left' });
    await protocol.sendAction({ type: 'pointer.move', actor: 'human', x: 50, y: 60 });
    await protocol.sendAction({ type: 'pointer.up', actor: 'human', x: 50, y: 60, button: 'left' });
    await peer.settle();

    const mouseEvents = peer.received.filter((message) => message.kind === 'mouseEvent');
    expect(mouseEvents).toHaveLength(3);
    expect(mouseEvents[0]).toMatchObject({ mask: (0x01 << 3) | 1, x: 10, y: 10 }); // Left down
    expect(mouseEvents[1]).toMatchObject({ mask: 0, x: 50, y: 60 }); // Move while held
    expect(mouseEvents[2]).toMatchObject({ mask: (0x01 << 3) | 2, x: 50, y: 60 }); // Left up
  });

  it('encodes named keys as ControlKey and printable keys as unicode', async () => {
    const { peer, protocol } = await harness();
    await connect(protocol);

    await protocol.sendAction({ type: 'keyboard.keyDown', actor: 'human', key: 'Enter' });
    await protocol.sendAction({ type: 'keyboard.keyDown', actor: 'human', key: 'a' });
    await protocol.sendAction({ type: 'keyboard.type', actor: 'ai', text: 'hei verden' });
    await peer.settle();

    const keyEvents = peer.received.filter((message) => message.kind === 'keyEvent');
    expect(keyEvents).toHaveLength(3);
    expect(keyEvents[0]).toMatchObject({ down: true, controlKey: 27 }); // ControlKey.Return
    expect(keyEvents[1]).toMatchObject({ down: true, unicode: 'a'.codePointAt(0) });
    expect(keyEvents[2]).toMatchObject({ seq: 'hei verden', press: true });
  });

  it('rejects an unmappable key name instead of sending nothing', async () => {
    const { protocol } = await harness();
    await connect(protocol);

    await expect(
      protocol.sendAction({ type: 'keyboard.keyDown', actor: 'human', key: 'NotARealKey' }),
    ).resolves.toMatchObject({ ok: false });
  });

  it('sends clipboard text the peer can read back', async () => {
    const { peer, protocol } = await harness();
    await connect(protocol);

    await protocol.sendAction({ type: 'clipboard.write', actor: 'human', text: 'kopiert tekst' });
    await peer.settle();

    const clipboard = peer.received.find((message) => message.kind === 'clipboard');
    expect(clipboard).toMatchObject({ text: 'kopiert tekst', format: 0 });
  });

  it('display.select asks the host to switch and applies the host\'s confirmation, not its own guess', async () => {
    const displays: FakeDisplay[] = [
      { name: 'Left', width: 1920, height: 1080 },
      { name: 'Right', width: 2560, height: 1440 },
    ];
    const { peer, protocol } = await harness({ displays });
    const changes: Array<readonly string[]> = [];
    protocol.on('display-change', ({ displays: next }) =>
      changes.push(next.filter((display) => display.isPrimary).map((display) => display.id)),
    );
    await connect(protocol);
    expect(protocol.displays.find((display) => display.isPrimary)?.id).toBe('0');

    const result = await protocol.sendAction({ type: 'display.select', actor: 'human', displayId: '1' });
    expect(result.ok).toBe(true);
    // Not switched yet — the host has not confirmed.
    expect(protocol.displays.find((display) => display.isPrimary)?.id).toBe('0');

    await peer.settle();
    await flush();

    expect(peer.received.find((message) => message.kind === 'switchDisplay')).toMatchObject({ display: 1 });
    expect(peer.currentDisplay).toBe(1);
    expect(protocol.displays.find((display) => display.isPrimary)).toMatchObject({
      id: '1',
      label: 'Right',
      width: 2560,
      height: 1440,
    });
    expect(changes.at(-1)).toEqual(['1']);
  });

  it('display.select rejects an unknown displayId without sending anything', async () => {
    const { peer, protocol } = await harness();
    await connect(protocol);

    const result = await protocol.sendAction({ type: 'display.select', actor: 'human', displayId: '7' });
    await peer.settle();

    expect(result.ok).toBe(false);
    expect(result.error?.message).toMatch(/unknown displayId/i);
    expect(peer.received.some((message) => message.kind === 'switchDisplay')).toBe(false);
  });

  it('echoes the host\'s TestDelay probe and reports its last measured RTT as latency', async () => {
    const { peer, protocol } = await harness();
    const latencies: number[] = [];
    protocol.on('latency', ({ latencyMs }) => latencies.push(latencyMs));
    await connect(protocol);

    await peer.sendTestDelay(37, 1_700_000_000_000n);
    await peer.settle();
    await flush();
    await peer.settle();

    // Verbatim echo: the host correlates on `time`, so it must come back untouched.
    const echo = peer.received.find((message) => message.kind === 'testDelay');
    expect(echo).toMatchObject({ time: 1_700_000_000_000n, fromClient: false, lastDelay: 37 });
    expect(latencies).toEqual([37]);
  });

  it('a host probe with no previous measurement is echoed but reports no latency', async () => {
    const { peer, protocol } = await harness();
    const latencies: number[] = [];
    protocol.on('latency', ({ latencyMs }) => latencies.push(latencyMs));
    await connect(protocol);

    await peer.sendTestDelay(0);
    await peer.settle();
    await flush();
    await peer.settle();

    expect(peer.received.some((message) => message.kind === 'testDelay')).toBe(true);
    expect(latencies).toEqual([]);
  });

  it('measures its own RTT from client-originated probes the host echoes back', async () => {
    const { peer, protocol } = await harness({ latencyProbeIntervalMs: 5 });
    const latencies: number[] = [];
    protocol.on('latency', ({ latencyMs }) => latencies.push(latencyMs));
    await connect(protocol);

    await new Promise((resolve) => setTimeout(resolve, 40));
    await peer.settle();
    await flush();
    await protocol.disconnect();

    const probes = peer.received.filter((message) => message.kind === 'testDelay' && message.fromClient);
    expect(probes.length).toBeGreaterThan(0);
    expect(latencies.length).toBeGreaterThan(0);
    for (const latency of latencies) {
      expect(latency).toBeGreaterThanOrEqual(0);
      expect(latency).toBeLessThan(1_000);
    }
  });

  it('maps a host permission grant onto the public permission set', async () => {
    const { peer, protocol } = await harness();
    const changes: string[][] = [];
    protocol.on('permission-change', ({ permissions }) => changes.push([...permissions].sort()));

    await connect(protocol);
    await peer.sendEncrypted(encodePermissionInfoMessage(Permission.Keyboard, true));
    await peer.settle();
    await flush();

    // RustDesk gates mouse AND keyboard behind one Keyboard flag.
    expect(changes.at(-1)).toEqual(['input.keyboard', 'input.pointer', 'screen.view']);
  });

  it('revokes permissions mid-session when the host turns them off', async () => {
    const { peer, protocol } = await harness();
    await connect(protocol);

    await peer.sendEncrypted(encodePermissionInfoMessage(Permission.Clipboard, true));
    await peer.settle();
    await flush();
    expect([...protocol.permissions].sort()).toContain('clipboard.write');

    await peer.sendEncrypted(encodePermissionInfoMessage(Permission.Clipboard, false));
    await peer.settle();
    await flush();
    expect(protocol.permissions).not.toContain('clipboard.write');
  });

  it('emits clipboard updates pushed by the host', async () => {
    const { peer, protocol } = await harness();
    const received: Array<string | undefined> = [];
    protocol.on('clipboard', ({ text }) => received.push(text));

    await connect(protocol);
    await peer.sendEncrypted(encodeClipboardMessage('fra fjernmaskinen'));
    await peer.settle();
    await flush();

    expect(received).toEqual(['fra fjernmaskinen']);
  });

  it('survives a video frame arriving where WebCodecs is unavailable (Node)', async () => {
    const { peer, protocol } = await harness();
    await connect(protocol);

    // vitest runs in Node, which has no WebCodecs VideoDecoder. The frame
    // must be reported as a codec problem, not tear the session down.
    await peer.sendEncrypted(
      encodeVideoFrameMessage({ data: new Uint8Array([1, 2, 3]), key: true, pts: 0n, display: 0 }),
    );
    await peer.settle();
    await flush();

    expect(protocol.displays).toHaveLength(1);
    await expect(
      protocol.sendAction({ type: 'pointer.move', actor: 'human', x: 1, y: 1 }),
    ).resolves.toMatchObject({ ok: true });
  });

  it('reports relay loss as a retryable disconnect, then becomes reconnectable', async () => {
    const { peer, protocol, relay } = await harness();
    const reasons: string[] = [];
    const states: string[] = [];
    protocol.on('disconnect', ({ reason }) => reasons.push(reason));
    protocol.on('state', ({ to }) => states.push(to));

    await connect(protocol);
    relay.simulateClose({ wasClean: false, code: 1006 });

    expect(reasons).toEqual(['relay-failure']);
    // 'disconnect' (with the reason) is emitted BEFORE the terminal state.
    expect(states.at(-1)).toBe('disconnected');

    // The same instance must accept a fresh connect() — that is what the
    // session's reconnect loop relies on. The host side simply sees a new
    // incoming connection and runs the handshake again.
    peer.resetPairing();
    await connect(protocol);
    await peer.settle();
    expect(peer.loginPasswordMatched).toBe(true);
    expect(protocol.displays).toHaveLength(1);
  });

  it('closes the relay transport on disconnect()', async () => {
    const { protocol, relay } = await harness();
    await connect(protocol);

    await protocol.disconnect();

    expect(relay.closeCalls).toBe(1);
  });

  it('refuses a second connect() on the same instance', async () => {
    const { protocol } = await harness();
    await connect(protocol);

    await expect(connect(protocol)).rejects.toThrow(/state "connected"/);
  });

  it('captureFrame fails honestly before any frame has decoded', async () => {
    const { protocol } = await harness();
    await connect(protocol);

    await expect(protocol.captureFrame()).rejects.toThrow(/no video frame/i);
  });
});

/** Lets the protocol's serialised inbound chain drain. */
function flush(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

describe('RustDeskProtocol — connect cancellation (AbortSignal)', () => {
  it('refuses to start when the signal is already aborted', async () => {
    const { protocol, rendezvous } = await harness();
    const controller = new AbortController();
    controller.abort();

    await expect(
      protocol.connect({
        deviceId: DEVICE_ID,
        authenticator: new RustDeskPasswordAuthenticator(SECRET),
        signal: controller.signal,
      }),
    ).rejects.toThrow(/aborted by the caller/i);

    // Nothing was put on the wire at all.
    expect(rendezvous.sent).toHaveLength(0);
  });

  it('aborts a connect that is waiting on the peer, and closes the relay behind it', async () => {
    // A rendezvous that never answers leaves connect() parked on its first expect().
    const peer = await FakeRustDeskPeer.create({ deviceId: DEVICE_ID, secret: SECRET });
    const rendezvous = new MockTransport();
    const relay = new MockTransport();
    rendezvous.onSend = (data) => {
      const message = decodePeerRendezvousMessage(data);
      if (message.kind !== 'punchHoleRequest') return;
      rendezvous.deliver(encodePunchHoleResponse({ relayServer: RELAY_URL, pk: peer.vouch }));
    };
    // Deliberately do NOT attach the peer: no SignedId will ever arrive.

    const protocol = new RustDeskProtocol({
      rendezvousUrl: RENDEZVOUS_URL,
      relayUrl: RELAY_URL,
      serverPublicKey: peer.serverPublicKeyBase64,
      latencyProbeIntervalMs: 0,
      createTransport: (url) => (url === RENDEZVOUS_URL ? rendezvous : relay),
    });

    const controller = new AbortController();
    const connecting = protocol.connect({
      deviceId: DEVICE_ID,
      authenticator: new RustDeskPasswordAuthenticator(SECRET),
      signal: controller.signal,
    });

    await flush();
    controller.abort();

    await expect(connecting).rejects.toThrow(/aborted by the caller/i);
    // abandonConnect() must have run: the relay socket is not left open, and
    // the protocol landed in 'failed' rather than staying pinned in
    // 'authenticating' (which ACTIVE_STATES would refuse a reconnect from).
    // Reusability itself is covered by the relay-drop suite below.
    expect(relay.closeCalls).toBe(1);
  });
});

describe('RustDeskProtocol — two-factor edge cases', () => {
  it('gives up on its own deadline instead of hanging when nobody supplies a code', async () => {
    const { protocol, relay } = await harness({ require2FACode: '123456', secondFactorTimeoutMs: 20 });

    await expect(
      protocol.connect({
        deviceId: DEVICE_ID,
        // A provider that never resolves — the operator walked away.
        authenticator: new RustDeskPasswordAuthenticator(SECRET, {
          secondFactor: () => new Promise<string>(() => undefined),
        }),
      }),
    ).rejects.toThrow(/timed out waiting for the two-factor code/i);

    // The half-authenticated relay session must not be left allocated.
    expect(relay.closeCalls).toBe(1);
  });

  it('does not prompt for a code during an automatic reconnect attempt', async () => {
    const { peer, protocol } = await harness({ require2FACode: '123456' });
    const secondFactor = vi.fn(async () => '123456');

    await expect(
      protocol.connect({
        deviceId: DEVICE_ID,
        authenticator: new RustDeskPasswordAuthenticator(SECRET, { secondFactor }),
        isAutomaticRetry: true,
      }),
    ).rejects.toThrow(/reconnect manually/i);

    await peer.settle();
    // The whole point: no dialog the operator never asked for.
    expect(secondFactor).not.toHaveBeenCalled();
    expect(peer.received.some((message) => message.kind === 'auth2fa')).toBe(false);
  });

  it('distinguishes a re-issued challenge from "no code was ever requested"', async () => {
    const { peer, protocol } = await harness({ require2FACode: '123456' });
    // The host answers our code with another challenge (an expired TOTP window)
    // rather than "Wrong 2FA Code".
    peer.reissueChallengeOnAuth2FA = true;

    await expect(connect(protocol, SECRET, async () => '123456')).rejects.toThrow(/re-issued/i);
  });
});

describe('RustDeskProtocol — input validation and host surprises', () => {
  it('rejects a displayId that is not one we published, instead of coercing it to a number', async () => {
    const displays: FakeDisplay[] = [
      { name: 'Left', width: 1920, height: 1080 },
      { name: 'Right', width: 2560, height: 1440 },
    ];
    const { peer, protocol } = await harness({ displays });
    await connect(protocol);

    // Number('') === 0 would have passed a naive numeric guard and switched to
    // display 0 while reporting success.
    for (const displayId of ['', ' 1 ', '1.0', '0x1', '1e0']) {
      const result = await protocol.sendAction({ type: 'display.select', actor: 'human', displayId });
      expect(result.ok, `displayId ${JSON.stringify(displayId)} must be refused`).toBe(false);
      expect(result.error?.message).toMatch(/unknown displayId/i);
    }

    await peer.settle();
    expect(peer.received.some((message) => message.kind === 'switchDisplay')).toBe(false);
    // The real id still works.
    await expect(
      protocol.sendAction({ type: 'display.select', actor: 'human', displayId: '1' }),
    ).resolves.toMatchObject({ ok: true });
  });

  it('leaves the display list untouched when the host confirms a display we never enumerated', async () => {
    const { peer, protocol } = await harness();
    await connect(protocol);
    const before = protocol.displays;
    const changes: unknown[] = [];
    protocol.on('display-change', (payload) => changes.push(payload));

    // Host starts streaming a third monitor we never saw in PeerInfo.
    await peer.sendEncrypted(encodeSwitchDisplayMessage({ display: 2, width: 800, height: 600 }));
    await peer.settle();
    await flush();

    // Silently clearing every isPrimary flag would leave the UI pointing at a
    // display that is not being streamed.
    expect(protocol.displays).toEqual(before);
    expect(protocol.displays.some((display) => display.isPrimary)).toBe(true);
    expect(changes).toHaveLength(0);
  });

  it('ignores a TestDelay echo with no timestamp rather than reporting a 55-year latency', async () => {
    const { peer, protocol } = await harness();
    const latencies: number[] = [];
    protocol.on('latency', ({ latencyMs }) => latencies.push(latencyMs));
    await connect(protocol);

    // proto3 elides zeroes, so "no time field" and "time = 0" are the same bytes.
    await peer.sendEncrypted(encodeTestDelayMessage({ time: 0n, fromClient: true, lastDelay: 0 }));
    await peer.settle();
    await flush();

    expect(latencies).toEqual([]);
  });

  it('ignores an implausible round trip', async () => {
    const { peer, protocol } = await harness();
    const latencies: number[] = [];
    protocol.on('latency', ({ latencyMs }) => latencies.push(latencyMs));
    await connect(protocol);

    // A real measurement cannot exceed the host's own 30 s inactivity cutoff.
    await peer.sendEncrypted(encodeTestDelayMessage({ time: 1_000n, fromClient: true, lastDelay: 0 }));
    await peer.settle();
    await flush();

    expect(latencies).toEqual([]);
  });
});

describe('RustDeskProtocol — reconnectable after a relay drop (the real seam)', () => {
  it('a relay loss leaves the instance in a state that accepts a fresh connect()', async () => {
    const { peer, protocol, relay } = await harness();
    await connect(protocol);
    expect(protocol.displays).toHaveLength(1);

    relay.simulateClose({ wasClean: false, code: 1006 });

    // This is what Reconnector does — and what ACTIVE_STATES would have blocked
    // if handleRelayLoss had left us in 'connected' or 'authenticating'.
    peer.resetPairing();
    await connect(protocol);
    await peer.settle();

    expect(peer.loginPasswordMatched).toBe(true);
    expect(protocol.displays).toHaveLength(1);
    // Permissions were rebuilt from the new login, not carried over stale.
    expect(protocol.permissions).toEqual(['screen.view']);
  });

  it('a failed attempt still leaves the instance reconnectable', async () => {
    const { peer, protocol, relay } = await harness();
    await connect(protocol);
    relay.simulateClose({ wasClean: false, code: 1006 });

    // Attempt 1: the peer refuses the login.
    peer.resetPairing();
    peer.rejectLoginWith = 'Wrong Password';
    await expect(connect(protocol)).rejects.toThrow(AuthenticationError);

    // Attempt 2 must still be allowed (abandonConnect lands in 'failed', which
    // is outside ACTIVE_STATES).
    peer.resetPairing();
    peer.rejectLoginWith = undefined;
    await connect(protocol);
    await peer.settle();
    expect(protocol.displays).toHaveLength(1);
  });
});

describe('RustDeskProtocol — declaring what the browser can decode', () => {
  function loginOption(peer: FakeRustDeskPeer) {
    const login = peer.received.find((message) => message.kind === 'loginRequest');
    if (login?.kind !== 'loginRequest') throw new Error('no loginRequest was received');
    return login.option;
  }

  it('advertises exactly the codecs WebCodecs reports, as 0/1 int32 flags', async () => {
    // A typical Chromium without hardware H265.
    const { peer, protocol } = await harness({ videoCodecs: ['vp8', 'vp9', 'av1'] });
    await connect(protocol);
    await peer.settle();

    const decoding = loginOption(peer)?.supportedDecoding;
    expect(decoding).toBeDefined();
    // The host tests `> 0`; we send literal 1 to stay bit-identical with the
    // reference client, and 0 (elided) for what we cannot decode.
    expect(decoding).toMatchObject({
      abilityVp8: 1,
      abilityVp9: 1,
      abilityAv1: 1,
      abilityH264: 0,
      abilityH265: 0,
    });
  });

  it('never sends the i444 / prefer_chroma fields', async () => {
    const { peer, protocol } = await harness();
    await connect(protocol);
    await peer.settle();

    // Copying the reference client's `i444 { vp9: true }` would let a host with
    // only a browser viewer emit VP9 profile 1, which most WebCodecs VP9
    // decoders reject — with nothing on the wire to explain the black screen.
    expect(loginOption(peer)?.supportedDecoding?.sentChromaFields).toBe(false);
  });

  it('asks for VP9 by default, since it is the only codec the protocol guarantees', async () => {
    const { peer, protocol } = await harness();
    await connect(protocol);
    await peer.settle();

    expect(loginOption(peer)?.supportedDecoding?.prefer).toBe(1); // PreferCodec.VP9
  });

  it('asks for H264 only when the browser can actually decode it', async () => {
    const withH264 = await harness({ videoCodecs: ['vp9', 'h264'], preferredCodec: 'h264' });
    await connect(withH264.protocol);
    await withH264.peer.settle();
    expect(loginOption(withH264.peer)?.supportedDecoding).toMatchObject({ prefer: 2, abilityH264: 1 });

    // Asking for H264 on a browser that cannot decode it must not advertise it.
    const withoutH264 = await harness({ videoCodecs: ['vp9'], preferredCodec: 'h264' });
    await connect(withoutH264.protocol);
    await withoutH264.peer.settle();
    expect(loginOption(withoutH264.peer)?.supportedDecoding).toMatchObject({ prefer: 1, abilityH264: 0 });
  });

  it('turns off host audio with BoolOption.Yes (2) — not the intuitive 1, which means No', async () => {
    const { peer, protocol } = await harness();
    await connect(protocol);
    await peer.settle();

    const option = loginOption(peer);
    // 1 would be an explicit "No", i.e. audio stays ON. This is the single
    // easiest field in the protocol to get backwards.
    expect(option?.disableAudio).toBe(2);
    // Clipboard IS implemented, so we must not disable it.
    expect(option?.disableClipboard).toBeUndefined();
    // The host never sends CursorPosition unless this is explicitly Yes — a
    // drawn cursor would otherwise never move.
    expect(option?.showRemoteCursor).toBe(2);
  });

  it('warns rather than pretending when the browser cannot decode VP9 at all', async () => {
    const warnings: string[] = [];
    const peer = await FakeRustDeskPeer.create({ deviceId: DEVICE_ID, secret: SECRET });
    const rendezvous = new MockTransport();
    const relay = new MockTransport();
    rendezvous.onSend = (data) => {
      if (decodePeerRendezvousMessage(data).kind !== 'punchHoleRequest') return;
      rendezvous.deliver(encodePunchHoleResponse({ relayServer: RELAY_URL, pk: peer.vouch }));
    };
    peer.attachRelay(relay);

    const protocol = new RustDeskProtocol({
      rendezvousUrl: RENDEZVOUS_URL,
      relayUrl: RELAY_URL,
      serverPublicKey: peer.serverPublicKeyBase64,
      latencyProbeIntervalMs: 0,
      mediaCapabilities: { webCodecsAvailable: true, videoCodecs: ['h264'] },
      logger: { debug: () => undefined, info: () => undefined, error: () => undefined, warn: (m) => warnings.push(m) },
      createTransport: (url) => (url === RENDEZVOUS_URL ? rendezvous : relay),
    });

    await connect(protocol);
    await peer.settle();

    // The host never reads ability_vp9 and always falls back to VP9, so there
    // is no protocol escape — the only honest thing is to say so.
    expect(warnings.some((message) => /VP9/.test(message))).toBe(true);
  });
});

describe('RustDeskProtocol — quality control', () => {
  it('requests a quality level as a Misc.option carrying only image_quality', async () => {
    const { peer, protocol } = await harness();
    await connect(protocol);

    await protocol.sendAction({ type: 'quality.set', actor: 'human', level: 'low' });
    await peer.settle();

    const option = peer.received.find((message) => message.kind === 'option');
    if (option?.kind !== 'option') throw new Error('no Misc.option was received');
    expect(option.option.imageQuality).toBe(2); // ImageQuality.Low
    // The host's compatibility check requires a supported_decoding update to
    // travel ALONE, so a quality change must not carry one.
    expect(option.option.supportedDecoding).toBeUndefined();
  });

  it('maps every level onto the values the protocol actually defines', async () => {
    const { peer, protocol } = await harness();
    await connect(protocol);

    for (const [level, expected] of [
      ['low', 2],
      ['medium', 3],
      ['high', 4],
    ] as const) {
      await protocol.sendAction({ type: 'quality.set', actor: 'human', level });
      await peer.settle();
      const options = peer.received.filter((message) => message.kind === 'option');
      expect(options.at(-1)).toMatchObject({ option: { imageQuality: expected } });
    }

    // 'auto' is NotSet (0): it hands control back to the host's own ABR loop.
    // proto3 elides zero, so the option message is legitimately empty.
    await protocol.sendAction({ type: 'quality.set', actor: 'human', level: 'auto' });
    await peer.settle();
    const options = peer.received.filter((message) => message.kind === 'option');
    expect(options.at(-1)).toMatchObject({ option: { imageQuality: undefined } });
  });

  it('reports the host\'s live target bitrate rather than guessing a level from it', async () => {
    const { peer, protocol } = await harness();
    const events: Array<{ level: string; targetBitrateKbps?: number }> = [];
    protocol.on('quality', (payload) => events.push({ ...payload }));
    await connect(protocol);

    await peer.sendTestDelay(12, 1n + BigInt(Date.now()));
    await peer.settle();
    await flush();

    // Before we ask for anything, the level in effect really is 'auto'.
    expect(events).toEqual([]);

    // The host's own probe carries target_bitrate; that is the only quality
    // number the protocol reports.
    await peer.sendEncrypted(encodeTestDelayMessage({ time: BigInt(Date.now()), fromClient: false, lastDelay: 12, targetBitrate: 2073 }));
    await peer.settle();
    await flush();

    expect(events.at(-1)).toEqual({ level: 'auto', targetBitrateKbps: 2073 });

    // Asking for a level changes the reported level, keeping the host's number.
    await protocol.sendAction({ type: 'quality.set', actor: 'human', level: 'low' });
    expect(events.at(-1)).toEqual({ level: 'low', targetBitrateKbps: 2073 });
  });

  it('does not re-emit an unchanged bitrate', async () => {
    const { peer, protocol } = await harness();
    const events: unknown[] = [];
    protocol.on('quality', (payload) => events.push(payload));
    await connect(protocol);

    for (let index = 0; index < 3; index += 1) {
      await peer.sendEncrypted(
        encodeTestDelayMessage({ time: BigInt(Date.now()), fromClient: false, lastDelay: 5, targetBitrate: 1000 }),
      );
      await peer.settle();
      await flush();
    }

    expect(events).toHaveLength(1);
  });
});

describe('RustDeskProtocol — real media statistics', () => {
  it('reports the bytes actually received, before decoding', async () => {
    const { peer, protocol } = await harness();
    const samples: Array<{ encodedBytes: number; droppedFramesTotal: number }> = [];
    protocol.on('media-stats', (payload) => samples.push({ ...payload }));
    await connect(protocol);

    await peer.sendEncrypted(
      encodeVideoFrameMessage({ data: new Uint8Array(1234), key: true, pts: 0n, display: 0 }),
    );
    await peer.settle();
    await flush();

    expect(samples).toHaveLength(1);
    expect(samples[0]?.encodedBytes).toBe(1234);
  });
});

describe('RustDeskProtocol — remote cursor', () => {
  function solidCursor(width: number, height: number, byte = 0xab): Uint8Array {
    return new Uint8Array(width * height * 4).fill(byte);
  }

  it('decompresses a CursorData shape and exposes it as non-premultiplied RGBA', async () => {
    const { peer, protocol } = await harness();
    const shapes: Array<{ id: string; width: number; height: number; hotspotX: number; hotspotY: number; rgba: Uint8Array }> = [];
    protocol.on('cursor-shape', (shape) => shapes.push(shape));
    await connect(protocol);

    const rgba = solidCursor(16, 24);
    // colors travels zstd-compressed with NO flag — the client must always decompress.
    await peer.sendEncrypted(encodeCursorDataMessage({ id: 42n, hotx: 3, hoty: 5, width: 16, height: 24, rgba }));
    await peer.settle();
    await flush();

    expect(shapes).toHaveLength(1);
    expect(shapes[0]).toMatchObject({ id: '42', width: 16, height: 24, hotspotX: 3, hotspotY: 5 });
    expect(shapes[0]?.rgba.length).toBe(16 * 24 * 4);
    expect([...(shapes[0]?.rgba.subarray(0, 8) ?? [])]).toEqual([0xab, 0xab, 0xab, 0xab, 0xab, 0xab, 0xab, 0xab]);
  });

  it('re-selects a cached shape on cursor_id and keeps the current one for an unknown id', async () => {
    const { peer, protocol } = await harness();
    const shapes: string[] = [];
    protocol.on('cursor-shape', (shape) => shapes.push(shape.id));
    await connect(protocol);

    await peer.sendEncrypted(encodeCursorDataMessage({ id: 1n, hotx: 0, hoty: 0, width: 4, height: 4, rgba: solidCursor(4, 4, 1) }));
    await peer.sendEncrypted(encodeCursorDataMessage({ id: 2n, hotx: 0, hoty: 0, width: 4, height: 4, rgba: solidCursor(4, 4, 2) }));
    await peer.sendEncrypted(encodeCursorIdMessage(1n)); // host switches back to shape 1
    await peer.sendEncrypted(encodeCursorIdMessage(999n)); // never sent — must not blank the cursor
    await peer.settle();
    await flush();

    expect(shapes).toEqual(['1', '2', '1']);
  });

  it('converts host-global cursor positions into the streamed display\'s local coordinates', async () => {
    // Two monitors: the right one starts at x=1920 in the host's virtual desktop.
    const displays: FakeDisplay[] = [
      { name: 'Left', width: 1920, height: 1080, x: 0, y: 0 },
      { name: 'Right', width: 2560, height: 1440, x: 1920, y: -200 },
    ];
    const { peer, protocol } = await harness({ displays });
    const positions: Array<{ x: number; y: number; displayId: string }> = [];
    protocol.on('cursor-position', (position) => positions.push({ ...position }));
    await connect(protocol);

    // Streaming display 0: global == local.
    await peer.sendEncrypted(encodeCursorPositionMessage(100, 50));
    await peer.settle();
    await flush();
    expect(positions.at(-1)).toEqual({ x: 100, y: 50, displayId: '0' });

    // Switch to display 1; the host confirms with that display's origin.
    await protocol.sendAction({ type: 'display.select', actor: 'human', displayId: '1' });
    await peer.settle();
    await flush();
    await peer.sendEncrypted(encodeSwitchDisplayMessage({ display: 1, x: 1920, y: -200, width: 2560, height: 1440 }));
    await peer.settle();
    await flush();

    await peer.sendEncrypted(encodeCursorPositionMessage(2000, 100));
    await peer.settle();
    await flush();
    expect(positions.at(-1)).toEqual({ x: 80, y: 300, displayId: '1' });
  });

  it('ignores a cursor whose pixel data is not valid zstd, without ending the session', async () => {
    const { peer, protocol } = await harness();
    const shapes: unknown[] = [];
    protocol.on('cursor-shape', (shape) => shapes.push(shape));
    await connect(protocol);

    await peer.sendEncrypted(
      encodeCursorDataMessage({ id: 7n, hotx: 0, hoty: 0, width: 8, height: 8, rgba: solidCursor(8, 8), rawColors: new Uint8Array([1, 2, 3, 4]) }),
    );
    await peer.settle();
    await flush();

    expect(shapes).toEqual([]);
    // The inbound chain must still be alive: a later valid message is processed.
    await peer.sendEncrypted(encodeClipboardMessage('fortsatt i live'));
    await peer.settle();
    await flush();
    await expect(protocol.sendAction({ type: 'pointer.move', actor: 'human', x: 1, y: 1 })).resolves.toMatchObject({ ok: true });
  });

  it('rejects a cursor whose decompressed size does not match its declared dimensions', async () => {
    const { peer, protocol } = await harness();
    const shapes: unknown[] = [];
    protocol.on('cursor-shape', (shape) => shapes.push(shape));
    await connect(protocol);

    // Declares 8x8 but ships 4x4 worth of pixels.
    await peer.sendEncrypted(encodeCursorDataMessage({ id: 8n, hotx: 0, hoty: 0, width: 8, height: 8, rgba: solidCursor(4, 4) }));
    await peer.settle();
    await flush();

    expect(shapes).toEqual([]);
  });

  it('rejects implausible cursor dimensions before allocating anything', async () => {
    const { peer, protocol } = await harness();
    const shapes: unknown[] = [];
    protocol.on('cursor-shape', (shape) => shapes.push(shape));
    await connect(protocol);

    // A 20000x20000 cursor would be 1.6 GB of RGBA; the dimensions are checked first.
    await peer.sendEncrypted(
      encodeCursorDataMessage({ id: 9n, hotx: 0, hoty: 0, width: 20_000, height: 20_000, rgba: new Uint8Array(0), rawColors: new Uint8Array([0]) }),
    );
    await peer.settle();
    await flush();

    expect(shapes).toEqual([]);
  });

  it('surfaces cursorEmbedded per display so a renderer can skip its own overlay', async () => {
    const { protocol } = await harness({ displays: [{ name: 'Embedded', width: 800, height: 600, cursorEmbedded: true }] });
    await connect(protocol);
    expect(protocol.displays[0]?.cursorEmbedded).toBe(true);
  });
});
