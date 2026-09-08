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
  /**
   * Satt når kallet kommer fra den automatiske gjenoppkoblingen, ikke fra en
   * bruker. En protokoll skal da IKKE be om interaktiv legitimasjon (f.eks.
   * en tofaktorkode): fem bakgrunnsforsøk ville gitt fem dialogbokser
   * operatøren ikke ba om. Feil raskt i stedet, så økten lander i 'failed'
   * og operatøren kan koble til på nytt selv.
   */
  readonly isAutomaticRetry?: boolean;
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
