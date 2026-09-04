import type {
  DisconnectReason,
  QualityLevel,
  RemoteAction,
  RemoteDisplay,
  RemotePermission,
  RemotePermission as Permission,
  RemoteVideoFrame,
} from './public.js';

/**
 * Interne øktilstander — finere granulert enn det offentlige `SessionState`-
 * unionet. 'idle' finnes før RemoteSession-objektet i det hele tatt
 * eksisterer, og 'rendezvous' er en RustDesk-spesifikk underfase av
 * "connecting" som UI ikke trenger å skille fra resten av tilkoblingen.
 */
export type InternalSessionState =
  | 'idle'
  | 'connecting'
  | 'rendezvous'
  | 'authenticating'
  | 'connected'
  | 'reconnecting'
  | 'disconnected'
  | 'failed';

export interface ProtocolEventMap extends Record<string, unknown> {
  readonly state: { readonly from: InternalSessionState; readonly to: InternalSessionState };
  readonly frame: RemoteVideoFrame;
  readonly 'display-change': { readonly displays: readonly RemoteDisplay[] };
  readonly latency: { readonly latencyMs: number };
  readonly quality: { readonly level: QualityLevel };
  readonly 'permission-change': { readonly permissions: readonly RemotePermission[] };
  readonly clipboard: { readonly text?: string };
  readonly disconnect: { readonly reason: DisconnectReason };
}

export type ProtocolEvent = keyof ProtocolEventMap;

/** Hvilken tillatelse en gitt handling krever. Uttømmende over RemoteAction. */
export function requiredPermissionFor(action: RemoteAction): Permission {
  switch (action.type) {
    case 'pointer.move':
    case 'pointer.click':
    case 'pointer.doubleClick':
    case 'pointer.scroll':
      return 'input.pointer';
    case 'keyboard.keyDown':
    case 'keyboard.keyUp':
    case 'keyboard.type':
      return 'input.keyboard';
    case 'clipboard.write':
      return 'clipboard.write';
    case 'display.select':
      return 'screen.view';
    default: {
      const exhaustive: never = action;
      throw new Error(`Unhandled action type: ${JSON.stringify(exhaustive)}`);
    }
  }
}

const RETRYABLE_DISCONNECT_REASONS: ReadonlySet<DisconnectReason> = new Set([
  'transient-network',
  'relay-failure',
]);

export function isRetryableDisconnectReason(reason: DisconnectReason): boolean {
  return RETRYABLE_DISCONNECT_REASONS.has(reason);
}

const FAILURE_DISCONNECT_REASONS: ReadonlySet<DisconnectReason> = new Set([
  'authentication-failed',
  'protocol-error',
  'unknown',
]);

/**
 * Klassifiserer hvor en frakobling skal lande i tilstandsmaskinen. Forbigående
 * årsaker gir en reconnect-forsøk BARE når økten faktisk var tilkoblet fra
 * før; ellers er det ingenting å gjenopprette. Autentiseringsfeil, protokoll-
 * feil og ukjente årsaker regnes som feil (failed); resten (brukerinitiert,
 * ekstern nedstenging, tilbakekalt tillatelse, tidsavbrudd) er en ren, ikke-
 * feilaktig avslutning (disconnected).
 */
export function classifyDisconnect(
  reason: DisconnectReason,
  wasConnected: boolean,
): 'reconnecting' | 'disconnected' | 'failed' {
  if (wasConnected && isRetryableDisconnectReason(reason)) return 'reconnecting';
  if (FAILURE_DISCONNECT_REASONS.has(reason)) return 'failed';
  return 'disconnected';
}
