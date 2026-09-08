import type { Transport, TransportCloseInfo, Unsubscribe } from '../../src/transport/Transport.js';

/**
 * In-memory Transport. Lets a test stand in for the far end of the wire
 * without a real WebSocket: `sent` records what the protocol wrote, and
 * `deliver()` pushes bytes back as if they arrived from the network.
 */
export class MockTransport implements Transport {
  readonly sent: Uint8Array[] = [];
  connectCalls = 0;
  closeCalls = 0;

  /** Invoked (async) whenever the protocol sends — how a fake peer reacts. */
  onSend: ((data: Uint8Array) => void) | undefined;

  private readonly messageHandlers = new Set<(data: Uint8Array) => void>();
  private readonly closeHandlers = new Set<(info: TransportCloseInfo) => void>();
  private readonly errorHandlers = new Set<(error: Error) => void>();

  async connect(): Promise<void> {
    this.connectCalls += 1;
  }

  send(data: Uint8Array): void {
    this.sent.push(data);
    const handler = this.onSend;
    if (handler) {
      // Deferred so the peer reacts after the protocol's own send returns,
      // the way a real network round-trip would.
      queueMicrotask(() => handler(data));
    }
  }

  async close(): Promise<void> {
    this.closeCalls += 1;
  }

  onMessage(handler: (data: Uint8Array) => void): Unsubscribe {
    this.messageHandlers.add(handler);
    return () => this.messageHandlers.delete(handler);
  }

  onClose(handler: (info: TransportCloseInfo) => void): Unsubscribe {
    this.closeHandlers.add(handler);
    return () => this.closeHandlers.delete(handler);
  }

  onError(handler: (error: Error) => void): Unsubscribe {
    this.errorHandlers.add(handler);
    return () => this.errorHandlers.delete(handler);
  }

  /** Test hook: deliver bytes to the protocol as an inbound message. */
  deliver(data: Uint8Array): void {
    for (const handler of [...this.messageHandlers]) handler(data);
  }

  simulateClose(info: TransportCloseInfo = { wasClean: true, code: 1000 }): void {
    for (const handler of [...this.closeHandlers]) handler(info);
  }

  simulateError(error = new Error('mock transport error')): void {
    for (const handler of [...this.errorHandlers]) handler(error);
  }
}
