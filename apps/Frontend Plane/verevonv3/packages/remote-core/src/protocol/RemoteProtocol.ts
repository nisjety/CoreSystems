import type {
  ActionResult,
  CaptureFrameOptions,
  RemoteAction,
  RemoteDisplay,
  RemotePermission,
  RemoteVideoFrame,
  SessionAuthenticator,
  Unsubscribe,
} from '../types/public.js';
import type { ProtocolEvent, ProtocolEventMap } from '../types/internal.js';

export interface ProtocolConnectOptions {
  readonly deviceId: string;
  readonly authenticator: SessionAuthenticator;
  readonly signal?: AbortSignal;
}

/**
 * Eneste grensesnitt resten av remote-core får lov å snakke med. Ingen annen
 * modul enn protocol/rustdesk/* skal vite at RustDesk finnes ("RustDesk
 * protocol isolation") — en fremtidig, annen backend-protokoll kan
 * implementere dette uten at RemoteClient/RemoteSession eller den offentlige
 * API-en endres.
 */
export interface RemoteProtocol {
  readonly displays: readonly RemoteDisplay[];
  /** Tillatelser slik de forelå da forbindelsen ble etablert. Senere endringer kommer som 'permission-change'-hendelser. */
  readonly permissions: readonly RemotePermission[];
  connect(options: ProtocolConnectOptions): Promise<void>;
  sendAction(action: RemoteAction): Promise<ActionResult>;
  captureFrame(options?: CaptureFrameOptions): Promise<RemoteVideoFrame>;
  disconnect(): Promise<void>;
  on<T extends ProtocolEvent>(event: T, handler: (payload: ProtocolEventMap[T]) => void): Unsubscribe;
}
