import type { PermissionController, RemotePermission, Unsubscribe } from '../types/public.js';
import { PermissionDeniedError } from '../errors/RemoteError.js';
import { EventBus } from '../events/EventBus.js';

interface PermissionEventMap extends Record<string, unknown> {
  change: readonly RemotePermission[];
}

/**
 * Eneste kilde til sannhet for hvilke tillatelser en økt har akkurat nå.
 * Tillatelser kan oppdateres når som helst i en aktiv økt (f.eks. hvis
 * kunden trekker tilbake museknontroll midtveis) — se "Support permission
 * updates during an active session" i spec-en.
 */
export class PermissionManager implements PermissionController {
  private granted: Set<RemotePermission>;
  private readonly bus = new EventBus<PermissionEventMap>();

  constructor(initial: readonly RemotePermission[] = []) {
    this.granted = new Set(initial);
  }

  has(permission: RemotePermission): boolean {
    return this.granted.has(permission);
  }

  list(): readonly RemotePermission[] {
    return [...this.granted];
  }

  /** Erstatter hele tillatelsessettet og varsler abonnenter ved endring. */
  update(permissions: readonly RemotePermission[]): void {
    const next = new Set(permissions);
    if (setsAreEqual(this.granted, next)) return;
    this.granted = next;
    this.bus.emit('change', this.list());
  }

  onChange(handler: (permissions: readonly RemotePermission[]) => void): Unsubscribe {
    return this.bus.on('change', handler);
  }

  /** Kaster PermissionDeniedError hvis tillatelsen mangler; ellers no-op. */
  require(permission: RemotePermission): void {
    if (!this.has(permission)) {
      throw new PermissionDeniedError(permission);
    }
  }
}

function setsAreEqual<T>(a: ReadonlySet<T>, b: ReadonlySet<T>): boolean {
  if (a.size !== b.size) return false;
  for (const value of a) {
    if (!b.has(value)) return false;
  }
  return true;
}
