import type { Transport, TransportCloseInfo, Unsubscribe } from './Transport.js';
import { TransportError } from '../errors/RemoteError.js';

/**
 * Minimal struktur-type for det vi faktisk bruker av WebSocket. Gjør
 * WebSocketTransport testbar uten en ekte nettleser-socket (se
 * tests/doubles/MockWebSocket.ts).
 */
export interface WebSocketLike {
  readyState: number;
  binaryType: string;
  onopen: ((event: unknown) => void) | null;
  onmessage: ((event: { data: unknown }) => void) | null;
  onerror: ((event: unknown) => void) | null;
  onclose: ((event: { code: number; reason: string; wasClean: boolean }) => void) | null;
  send(data: ArrayBufferLike | ArrayBufferView): void;
  close(code?: number, reason?: string): void;
}

export type WebSocketFactory = (url: string, protocols?: string | string[]) => WebSocketLike;

const WEBSOCKET_OPEN_STATE = 1;

// Global `WebSocket` sine callback-signaturer er typet mot `Event`/`MessageEvent`,
// mens WebSocketLike bevisst bruker `unknown` for å holde grensesnittet minimalt
// og lett å mocke. Broen mellom de to krever en eksplisitt, lokalisert cast.
const defaultWebSocketFactory: WebSocketFactory = (url, protocols) =>
  new WebSocket(url, protocols) as unknown as WebSocketLike;

export interface WebSocketTransportOptions {
  readonly url: string;
  readonly protocols?: string | string[];
  readonly createWebSocket?: WebSocketFactory;
}

export class WebSocketTransport implements Transport {
  private readonly url: string;
  private readonly protocols: string | string[] | undefined;
  private readonly createWebSocket: WebSocketFactory;
  private socket: WebSocketLike | undefined;
  private readonly messageHandlers = new Set<(data: Uint8Array) => void>();
  private readonly closeHandlers = new Set<(info: TransportCloseInfo) => void>();
  private readonly errorHandlers = new Set<(error: Error) => void>();

  constructor(options: WebSocketTransportOptions) {
    this.url = options.url;
    this.protocols = options.protocols;
    this.createWebSocket = options.createWebSocket ?? defaultWebSocketFactory;
  }

  connect(): Promise<void> {
    return new Promise((resolve, reject) => {
      const socket = this.createWebSocket(this.url, this.protocols);
      socket.binaryType = 'arraybuffer';

      socket.onopen = () => resolve();

      socket.onerror = () => {
        const error = new TransportError('WebSocket transport error', { url: this.url });
        for (const handler of [...this.errorHandlers]) handler(error);
        reject(error);
      };

      socket.onmessage = (event) => {
        const data = toUint8Array(event.data);
        if (!data) return;
        for (const handler of [...this.messageHandlers]) handler(data);
      };

      socket.onclose = (event) => {
        const info: TransportCloseInfo = {
          code: event.code,
          reason: event.reason,
          wasClean: event.wasClean,
        };
        for (const handler of [...this.closeHandlers]) handler(info);
      };

      this.socket = socket;
    });
  }

  send(data: Uint8Array): void {
    if (!this.socket || this.socket.readyState !== WEBSOCKET_OPEN_STATE) {
      throw new TransportError('Cannot send: transport is not open', { url: this.url });
    }
    this.socket.send(data);
  }

  async close(): Promise<void> {
    this.socket?.close(1000, 'client-disconnect');
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
}

function toUint8Array(data: unknown): Uint8Array | undefined {
  if (data instanceof ArrayBuffer) return new Uint8Array(data);
  if (ArrayBuffer.isView(data)) {
    return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  }
  // RustDesk sin wire-protokoll er binær (protobuf). En tekst-/Blob-frame er
  // uventet og stilles i bero i stedet for å krasje transportlaget.
  return undefined;
}
