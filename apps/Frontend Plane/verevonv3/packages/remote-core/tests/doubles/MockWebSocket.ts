import type { WebSocketLike } from '../../src/transport/WebSocketTransport.js';

/** Testdobbel for nettleserens WebSocket — se tests/transport/WebSocketTransport.test.ts. */
export class MockWebSocket implements WebSocketLike {
  readyState = 0;
  binaryType = '';
  onopen: ((event: unknown) => void) | null = null;
  onmessage: ((event: { data: unknown }) => void) | null = null;
  onerror: ((event: unknown) => void) | null = null;
  onclose: ((event: { code: number; reason: string; wasClean: boolean }) => void) | null = null;
  readonly sent: Array<ArrayBufferLike | ArrayBufferView> = [];

  constructor(public readonly url: string) {}

  send(data: ArrayBufferLike | ArrayBufferView): void {
    this.sent.push(data);
  }

  close(code = 1000, reason = ''): void {
    this.readyState = 3;
    this.onclose?.({ code, reason, wasClean: true });
  }

  simulateOpen(): void {
    this.readyState = 1;
    this.onopen?.(undefined);
  }

  simulateMessage(data: ArrayBuffer): void {
    this.onmessage?.({ data });
  }

  simulateError(): void {
    this.onerror?.(undefined);
  }
}
