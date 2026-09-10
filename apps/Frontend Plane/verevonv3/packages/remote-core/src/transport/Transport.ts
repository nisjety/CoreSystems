export type Unsubscribe = () => void;

export interface TransportCloseInfo {
  readonly code?: number;
  readonly reason?: string;
  readonly wasClean: boolean;
}

/**
 * Transportlaget vet ingenting om RustDesk-protokollen — det flytter bare
 * bytes. `onClose`/`onError` går utover det minimale eksempelet i spec-en,
 * men er nødvendige: uten dem har øktlaget ingen måte å oppdage at
 * forbindelsen falt, og reconnection-tilstanden kan aldri utløses.
 */
export interface Transport {
  connect(): Promise<void>;
  send(data: Uint8Array): void;
  close(): Promise<void>;
  onMessage(handler: (data: Uint8Array) => void): Unsubscribe;
  onClose(handler: (info: TransportCloseInfo) => void): Unsubscribe;
  onError(handler: (error: Error) => void): Unsubscribe;
}
