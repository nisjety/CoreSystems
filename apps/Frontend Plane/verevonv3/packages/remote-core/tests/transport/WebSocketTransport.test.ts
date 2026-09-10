import { describe, expect, it } from 'vitest';
import { WebSocketTransport } from '../../src/transport/WebSocketTransport.js';
import { MockWebSocket } from '../doubles/MockWebSocket.js';

function createTransport() {
  let socket: MockWebSocket | undefined;
  const transport = new WebSocketTransport({
    url: 'wss://example.test/ws',
    createWebSocket: (url) => {
      socket = new MockWebSocket(url);
      return socket;
    },
  });
  return { transport, getSocket: () => socket };
}

describe('WebSocketTransport', () => {
  it('resolves connect() once the socket opens', async () => {
    const { transport, getSocket } = createTransport();
    const connectPromise = transport.connect();
    getSocket()?.simulateOpen();
    await expect(connectPromise).resolves.toBeUndefined();
  });

  it('rejects connect() on a socket error', async () => {
    const { transport, getSocket } = createTransport();
    const connectPromise = transport.connect();
    getSocket()?.simulateError();
    await expect(connectPromise).rejects.toThrow();
  });

  it('delivers binary messages as Uint8Array', async () => {
    const { transport, getSocket } = createTransport();
    const connectPromise = transport.connect();
    getSocket()?.simulateOpen();
    await connectPromise;

    const received: Uint8Array[] = [];
    transport.onMessage((data) => received.push(data));

    getSocket()?.simulateMessage(new Uint8Array([1, 2, 3]).buffer);

    expect(received).toHaveLength(1);
    expect(Array.from(received[0] ?? [])).toEqual([1, 2, 3]);
  });

  it('throws when sending before the socket is open', () => {
    const { transport } = createTransport();
    expect(() => transport.send(new Uint8Array([1]))).toThrow();
  });

  it('notifies onClose handlers when the socket closes', async () => {
    const { transport, getSocket } = createTransport();
    const connectPromise = transport.connect();
    getSocket()?.simulateOpen();
    await connectPromise;

    let closeInfo: { code?: number } | undefined;
    transport.onClose((info) => (closeInfo = info));
    await transport.close();

    expect(closeInfo?.code).toBe(1000);
  });
});
