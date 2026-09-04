import { describe, expect, it } from 'vitest';
import { createRemoteClient } from '../../src/client/createRemoteClient.js';
import { AuthenticationError, RemoteConnectionError } from '../../src/errors/RemoteError.js';
import { MockProtocol } from '../doubles/MockProtocol.js';
import type { AuthenticationResponse, SessionAuthenticator } from '../../src/types/public.js';

// createRemoteClient({ protocol }) — a custom/test protocol — never wires the
// "auth.token" shorthand (that only exists once the default RustDesk branch
// exists, see createRemoteClient.ts), so tests exercise the generic
// "authenticator" path directly instead.
const stubAuthenticator: SessionAuthenticator = {
  async authenticate(): Promise<AuthenticationResponse> {
    return { passwordHash: new Uint8Array([1, 2, 3]) };
  },
};

describe('createRemoteClient + RemoteClient.connect (public entry point)', () => {
  it('connects successfully when the protocol reaches "connected" and resolves a usable RemoteSession', async () => {
    const protocol = new MockProtocol();
    protocol.setConnectBehavior(async () => undefined); // MockProtocol reports 'connected' via its own state events below

    const client = createRemoteClient({ protocol });

    const connectPromise = client.connect({ deviceId: '123456789', authenticator: stubAuthenticator });

    // A real protocol implementation would emit these as it progresses;
    // MockProtocol lets the test drive them explicitly.
    protocol.emit('state', { from: 'connecting', to: 'rendezvous' });
    protocol.emit('state', { from: 'rendezvous', to: 'authenticating' });
    protocol.emit('state', { from: 'authenticating', to: 'connected' });

    const session = await connectPromise;

    expect(session.state).toBe('connected');
    expect(protocol.connectCalls).toHaveLength(1);
    expect(protocol.connectCalls[0]?.deviceId).toBe('123456789');
  });

  it('rejects with AuthenticationError when neither authenticator nor auth.token is given', async () => {
    const client = createRemoteClient({ protocol: new MockProtocol() });
    await expect(client.connect({ deviceId: 'x' } as never)).rejects.toThrow(AuthenticationError);
  });

  it('rejects with a typed error when the protocol throws during connect()', async () => {
    const protocol = new MockProtocol();
    protocol.setConnectBehavior(async () => {
      throw new Error('network unreachable');
    });
    const client = createRemoteClient({ protocol });

    await expect(client.connect({ deviceId: '123', authenticator: stubAuthenticator })).rejects.toThrow(
      RemoteConnectionError,
    );
  });

  it('rejects if the protocol resolves connect() without ever reaching "connected"', async () => {
    const protocol = new MockProtocol();
    protocol.setConnectBehavior(async () => undefined); // resolves immediately, no state events at all
    const client = createRemoteClient({ protocol });

    await expect(client.connect({ deviceId: '123', authenticator: stubAuthenticator })).rejects.toThrow(
      RemoteConnectionError,
    );
  });
});
