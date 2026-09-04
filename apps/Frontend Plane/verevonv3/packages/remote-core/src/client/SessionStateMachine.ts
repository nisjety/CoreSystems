import type { DisconnectReason } from '../types/public.js';
import type { InternalSessionState } from '../types/internal.js';
import { ProtocolError } from '../errors/RemoteError.js';

const TRANSITIONS: Readonly<Record<InternalSessionState, readonly InternalSessionState[]>> = {
  idle: ['connecting'],
  connecting: ['rendezvous', 'failed', 'disconnected'],
  rendezvous: ['authenticating', 'failed', 'disconnected'],
  authenticating: ['connected', 'failed', 'disconnected'],
  connected: ['reconnecting', 'disconnected', 'failed'],
  reconnecting: ['connected', 'failed', 'disconnected'],
  disconnected: [],
  failed: [],
};

export interface StateChange {
  readonly from: InternalSessionState;
  readonly to: InternalSessionState;
  readonly reason?: DisconnectReason;
}

/**
 * Streng tilstandsmaskin for økten: ulovlige overganger kaster i stedet for
 * å bli stille godtatt ("Do not allow impossible states"). Klassifisering av
 * *hvilken* tilstand en frakoblingsårsak skal føre til, er bevisst IKKE
 * maskinens ansvar — se `classifyDisconnect` i types/internal.ts — denne
 * klassen validerer og varsler, den tar ingen beslutninger selv.
 */
export class SessionStateMachine {
  private current: InternalSessionState = 'idle';
  private readonly listeners = new Set<(change: StateChange) => void>();

  get state(): InternalSessionState {
    return this.current;
  }

  canTransition(to: InternalSessionState): boolean {
    return TRANSITIONS[this.current].includes(to);
  }

  transition(to: InternalSessionState, reason?: DisconnectReason): StateChange {
    if (!this.canTransition(to)) {
      throw new ProtocolError(`Illegal session state transition: ${this.current} -> ${to}`, {
        from: this.current,
        to,
      });
    }
    const change: StateChange = { from: this.current, to, reason };
    this.current = to;
    for (const listener of [...this.listeners]) listener(change);
    return change;
  }

  onChange(handler: (change: StateChange) => void): () => void {
    this.listeners.add(handler);
    return () => this.listeners.delete(handler);
  }
}
