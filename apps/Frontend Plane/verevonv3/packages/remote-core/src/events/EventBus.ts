import type { Unsubscribe } from '../types/public.js';

export type { Unsubscribe };

type Listener<TPayload> = (payload: TPayload) => void;

/**
 * Generisk, sterkt typet publish/subscribe-buss. `TEventMap` knytter hvert
 * hendelsesnavn til nyttelasttypen sin, slik at `on`/`emit` er typesikre uten
 * at kallerne trenger å importere protokollens interne typer.
 */
export class EventBus<TEventMap extends Record<string, unknown>> {
  private readonly listeners = new Map<keyof TEventMap, Set<Listener<never>>>();

  on<K extends keyof TEventMap>(event: K, handler: Listener<TEventMap[K]>): Unsubscribe {
    let set = this.listeners.get(event);
    if (!set) {
      set = new Set();
      this.listeners.set(event, set);
    }
    set.add(handler as Listener<never>);
    return () => {
      set.delete(handler as Listener<never>);
    };
  }

  off<K extends keyof TEventMap>(event: K, handler: Listener<TEventMap[K]>): void {
    this.listeners.get(event)?.delete(handler as Listener<never>);
  }

  emit<K extends keyof TEventMap>(event: K, payload: TEventMap[K]): void {
    const set = this.listeners.get(event);
    if (!set || set.size === 0) return;
    // Kopier settet før iterasjon: en handler kan avabonnere seg selv (eller
    // andre) midt i emit(), noe som ellers ville forstyrret Set-iterasjonen.
    for (const handler of [...set]) {
      (handler as Listener<TEventMap[K]>)(payload);
    }
  }

  clear(): void {
    this.listeners.clear();
  }
}
