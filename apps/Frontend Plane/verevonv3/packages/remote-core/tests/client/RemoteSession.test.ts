import { describe, expect, it, vi } from 'vitest';
import { RemoteSessionImpl } from '../../src/client/RemoteSession.js';
import { SessionStateMachine } from '../../src/client/SessionStateMachine.js';
import { MockProtocol } from '../doubles/MockProtocol.js';

function connectedSession(protocol: MockProtocol): RemoteSessionImpl {
  const stateMachine = new SessionStateMachine();
  stateMachine.transition('connecting');
  stateMachine.transition('rendezvous');
  stateMachine.transition('authenticating');
  stateMachine.transition('connected');
  return new RemoteSessionImpl({ id: 'session-1', protocol, stateMachine });
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

  it('a transient-network disconnect event moves to reconnecting, not disconnected', () => {
    const protocol = new MockProtocol();
    const session = connectedSession(protocol);

    protocol.emit('disconnect', { reason: 'transient-network' });

    expect(session.state).toBe('reconnecting');
  });
});
