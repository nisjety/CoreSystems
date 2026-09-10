import { describe, expect, it, vi } from 'vitest';
import { RemoteSessionImpl, type RemoteSessionImplOptions } from '../../src/client/RemoteSession.js';
import { SessionStateMachine } from '../../src/client/SessionStateMachine.js';
import { AuthenticationError, RemoteConnectionError, CodecError } from '../../src/errors/RemoteError.js';
import { MockProtocol } from '../doubles/MockProtocol.js';

type SessionOverrides = Partial<Omit<RemoteSessionImplOptions, 'id' | 'protocol' | 'stateMachine'>>;

function connectedSession(protocol: MockProtocol, overrides: SessionOverrides = {}): RemoteSessionImpl {
  const stateMachine = new SessionStateMachine();
  stateMachine.transition('connecting');
  stateMachine.transition('rendezvous');
  stateMachine.transition('authenticating');
  stateMachine.transition('connected');
  return new RemoteSessionImpl({ id: 'session-1', protocol, stateMachine, ...overrides });
}

const noSleep = async (): Promise<void> => undefined;

/** Lets the async reconnect loop (attempt → sleep → attempt …) run to completion. */
async function settle(): Promise<void> {
  for (let index = 0; index < 20; index += 1) await Promise.resolve();
}

describe('RemoteSessionImpl', () => {
  it('starts in the connected public state', () => {
    const session = connectedSession(new MockProtocol());
    expect(session.state).toBe('connected');
  });

  it('re-publishes protocol frame events on the public bus and feeds stats', () => {
    const protocol = new MockProtocol();
    const session = connectedSession(protocol);
    const handler = vi.fn();
    session.on('frame', handler);

    protocol.emit('frame', { width: 100, height: 50, timestamp: 1 });

    expect(handler).toHaveBeenCalledWith({ width: 100, height: 50, timestamp: 1 });
    expect(session.stats.getSnapshot().decodedFrames).toBe(1);
  });

  it('pointer.move requires input.pointer permission', async () => {
    const protocol = new MockProtocol();
    const session = connectedSession(protocol);

    await expect(session.pointer.move({ x: 1, y: 1 })).rejects.toThrow();
  });

  it('pointer.click uses the last move() position when none is given', async () => {
    const protocol = new MockProtocol();
    protocol.permissions = ['input.pointer'];
    const session = connectedSession(protocol);

    await session.pointer.move({ x: 42, y: 7 });
    await session.pointer.click({ button: 'left' });

    expect(protocol.sentActions[1]).toMatchObject({ type: 'pointer.click', x: 42, y: 7, button: 'left' });
  });

  it('permission-change events update what actions the session allows mid-session', async () => {
    const protocol = new MockProtocol();
    const session = connectedSession(protocol);

    await expect(session.keyboard.keyDown('a')).rejects.toThrow();

    protocol.emit('permission-change', { permissions: ['input.keyboard'] });
    await expect(session.keyboard.keyDown('a')).resolves.toMatchObject({ ok: true });
  });

  it('an AI observer receives frames pushed through the protocol', () => {
    const protocol = new MockProtocol();
    const session = connectedSession(protocol);
    const observer = session.ai.createObserver({ maxFramesPerSecond: 1 });
    const received: unknown[] = [];
    observer.on('frame', (f) => received.push(f));

    protocol.emit('frame', { width: 1, height: 1, timestamp: 0 });

    expect(received).toHaveLength(1);
  });

  it('a clean disconnect transitions to disconnected and tears down subscriptions', async () => {
    const protocol = new MockProtocol();
    const session = connectedSession(protocol);
    const stateHandler = vi.fn();
    session.on('state', stateHandler);

    await session.disconnect();

    expect(session.state).toBe('disconnected');
    expect(protocol.disconnectCalls).toBe(1);
    expect(stateHandler).toHaveBeenCalledWith({ state: 'disconnected', previous: 'connected' });
  });

  it('a remote-shutdown disconnect event is classified as clean, not a failure', () => {
    const protocol = new MockProtocol();
    const session = connectedSession(protocol);

    protocol.emit('disconnect', { reason: 'remote-shutdown' });

    expect(session.state).toBe('disconnected');
  });

  it('a transient-network disconnect event moves to reconnecting when a reconnect is possible', () => {
    const protocol = new MockProtocol();
    // A reconnect that never resolves keeps the session visibly in 'reconnecting'.
    const session = connectedSession(protocol, { reconnect: () => new Promise(() => undefined), sleep: noSleep });

    protocol.emit('disconnect', { reason: 'transient-network' });

    expect(session.state).toBe('reconnecting');
  });

  it('a transient loss with no way to reconnect lands in disconnected instead of pretending', () => {
    const protocol = new MockProtocol();
    const session = connectedSession(protocol); // no `reconnect` supplied
    const states: string[] = [];
    session.on('state', ({ state }) => states.push(state));

    protocol.emit('disconnect', { reason: 'transient-network' });

    expect(session.state).toBe('disconnected');
    expect(states).toEqual(['disconnected']);
  });
});

describe('RemoteSessionImpl — automatic reconnection', () => {
  const policy = { maxAttempts: 3, baseDelayMs: 1, maxDelayMs: 1 };

  it('re-runs the connect and returns to connected when an attempt succeeds', async () => {
    const protocol = new MockProtocol();
    let attempts = 0;
    const reconnect = vi.fn(async () => {
      attempts += 1;
      if (attempts < 2) throw new Error('relay still down');
      // A real protocol reports its progress the same way during a reconnect.
      protocol.emit('state', { from: 'idle', to: 'connecting' });
      protocol.emit('state', { from: 'authenticating', to: 'connected' });
    });
    const session = connectedSession(protocol, { reconnect, reconnectPolicy: policy, sleep: noSleep });
    const states: string[] = [];
    const disconnects: string[] = [];
    session.on('state', ({ state }) => states.push(state));
    session.on('disconnect', ({ reason }) => disconnects.push(reason));

    protocol.emit('disconnect', { reason: 'relay-failure' });
    await settle();

    expect(reconnect).toHaveBeenCalledTimes(2);
    expect(session.state).toBe('connected');
    expect(states).toEqual(['reconnecting', 'connected']);
    expect(disconnects).toEqual(['relay-failure']);
    // Still a live session: events keep flowing.
    const frames = vi.fn();
    session.on('frame', frames);
    protocol.emit('frame', { width: 1, height: 1, timestamp: 0 });
    expect(frames).toHaveBeenCalledTimes(1);
  });

  it('ignores protocol disconnect events raised by failing attempts while reconnecting', async () => {
    const protocol = new MockProtocol();
    let attempts = 0;
    const reconnect = vi.fn(async () => {
      attempts += 1;
      if (attempts < 3) {
        // What RustDeskProtocol does when the relay drops mid-handshake.
        protocol.emit('disconnect', { reason: 'relay-failure' });
        throw new Error('handshake interrupted');
      }
      protocol.emit('state', { from: 'authenticating', to: 'connected' });
    });
    const session = connectedSession(protocol, { reconnect, reconnectPolicy: policy, sleep: noSleep });

    protocol.emit('disconnect', { reason: 'relay-failure' });
    await settle();

    expect(reconnect).toHaveBeenCalledTimes(3);
    expect(session.state).toBe('connected');
  });

  it('fails with a typed error event once the policy is exhausted', async () => {
    const protocol = new MockProtocol();
    const reconnect = vi.fn(async () => {
      throw new Error('relay still down');
    });
    const session = connectedSession(protocol, { reconnect, reconnectPolicy: policy, sleep: noSleep });
    const states: string[] = [];
    const errors: unknown[] = [];
    session.on('state', ({ state }) => states.push(state));
    session.on('error', (error) => errors.push(error));

    protocol.emit('disconnect', { reason: 'relay-failure' });
    await settle();

    expect(reconnect).toHaveBeenCalledTimes(policy.maxAttempts);
    expect(session.state).toBe('failed');
    expect(states).toEqual(['reconnecting', 'failed']);
    expect(errors).toHaveLength(1);
    expect(errors[0]).toBeInstanceOf(RemoteConnectionError);
    expect((errors[0] as Error).message).toMatch(/gave up after 3/);
  });

  it('a user disconnect during reconnection cancels further attempts', async () => {
    const protocol = new MockProtocol();
    let releaseSleep: (() => void) | undefined;
    const sleep = (): Promise<void> =>
      new Promise((resolve) => {
        releaseSleep = resolve;
      });
    const reconnect = vi.fn(async () => undefined);
    const session = connectedSession(protocol, { reconnect, reconnectPolicy: policy, sleep });

    protocol.emit('disconnect', { reason: 'transient-network' });
    expect(session.state).toBe('reconnecting');

    await session.disconnect();
    releaseSleep?.();
    await settle();

    expect(session.state).toBe('disconnected');
    expect(reconnect).not.toHaveBeenCalled();
    expect(protocol.disconnectCalls).toBe(1);
  });
});

describe('RemoteSessionImpl — a non-retryable loss arriving mid-reconnect', () => {
  const policy = { maxAttempts: 5, baseDelayMs: 1, maxDelayMs: 1, attemptTimeoutMs: 0 };

  it('stops the loop and reports the real reason instead of a fabricated "gave up"', async () => {
    const protocol = new MockProtocol();
    let attempts = 0;
    // Every attempt parks: without the reason-aware guard the session would sit
    // here burning all five attempts against a host that has shut down.
    const reconnect = vi.fn(() => {
      attempts += 1;
      return new Promise<void>(() => undefined);
    });
    const session = connectedSession(protocol, { reconnect, reconnectPolicy: policy, sleep: noSleep });
    const states: string[] = [];
    const disconnects: string[] = [];
    const errors: unknown[] = [];
    session.on('state', ({ state }) => states.push(state));
    session.on('disconnect', ({ reason }) => disconnects.push(reason));
    session.on('error', (error) => errors.push(error));

    protocol.emit('disconnect', { reason: 'relay-failure' });
    await settle();
    expect(session.state).toBe('reconnecting');
    expect(attempts).toBeGreaterThan(0);

    // The host is now genuinely gone — there is nothing to come back to.
    protocol.emit('disconnect', { reason: 'remote-shutdown' });
    await settle();

    expect(session.state).toBe('disconnected');
    expect(states).toEqual(['reconnecting', 'disconnected']);
    // The truthful reason reaches the consumer, and no invented error does.
    expect(disconnects).toEqual(['relay-failure', 'remote-shutdown']);
    expect(errors).toEqual([]);
  });

  it('still lets the Reconnector own a retryable loss that arrives mid-attempt', async () => {
    const protocol = new MockProtocol();
    let attempts = 0;
    const reconnect = vi.fn(async () => {
      attempts += 1;
      if (attempts < 2) {
        // A failing attempt reports its own relay failure; that must not end the session.
        protocol.emit('disconnect', { reason: 'relay-failure' });
        throw new Error('handshake interrupted');
      }
      protocol.emit('state', { from: 'authenticating', to: 'connected' });
    });
    const session = connectedSession(protocol, { reconnect, reconnectPolicy: policy, sleep: noSleep });

    protocol.emit('disconnect', { reason: 'relay-failure' });
    await settle();

    expect(session.state).toBe('connected');
    expect(attempts).toBe(2);
  });

  it('reports a refused reconnect as refused, not as exhaustion', async () => {
    const protocol = new MockProtocol();
    const reconnect = vi.fn(async () => {
      throw new AuthenticationError('Wrong Password');
    });
    const session = connectedSession(protocol, { reconnect, reconnectPolicy: policy, sleep: noSleep });
    const errors: Error[] = [];
    session.on('error', (error) => errors.push(error));

    protocol.emit('disconnect', { reason: 'transient-network' });
    await settle();

    expect(session.state).toBe('failed');
    // One attempt, not five — retrying a rejected credential is pointless.
    expect(reconnect).toHaveBeenCalledTimes(1);
    expect(errors).toHaveLength(1);
    expect(errors[0]?.message).toMatch(/refused and will not be retried/i);
    expect(errors[0]?.message).not.toMatch(/gave up after/i);
  });
});

describe('RemoteSessionImpl — non-fatal protocol errors', () => {
  it('forwards a protocol error to the public bus without ending the session', () => {
    const protocol = new MockProtocol();
    const session = connectedSession(protocol);
    const errors: Error[] = [];
    session.on('error', (error) => errors.push(error));

    // A dead video pipeline must be distinguishable from a healthy session.
    protocol.emit('error', new CodecError('Video decoder error: fatal'));

    expect(errors).toHaveLength(1);
    expect(errors[0]).toBeInstanceOf(CodecError);
    expect(session.state).toBe('connected');
  });
});
