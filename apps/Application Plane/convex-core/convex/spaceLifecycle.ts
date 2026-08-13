/**
 * Application-owned Space lifecycle rules.
 *
 * Control remains the authority for membership and access decisions. These
 * helpers deliberately only model the canonical Space aggregate lifecycle and
 * the stable event identity consumers use to deduplicate its outbox events.
 */

export const SPACE_LIFECYCLES = [
  'pending_registration',
  'active',
  'suspended',
  'deleting',
  'deleted',
  'failed_registration',
] as const;

export type SpaceLifecycle = (typeof SPACE_LIFECYCLES)[number];

export type SpaceLifecycleState = Readonly<{
  lifecycle: SpaceLifecycle;
  revision: number;
}>;

export type SpaceLifecycleEvent = Readonly<{
  eventId: string;
  externalOrgId: string;
  lifecycle: SpaceLifecycle;
  revision: number;
  spaceRef: string;
}>;

export type SpaceDeliveryState = 'pending' | 'claimed' | 'acknowledged' | 'failed' | 'rejected';

export type SpaceLifecycleDelivery = Readonly<{
  attempts: number;
  leaseExpiresAt?: number;
  leaseOwner?: string;
  nextAttemptAt: number;
  state: SpaceDeliveryState;
}>;

const NEXT_LIFECYCLES: Readonly<Record<SpaceLifecycle, readonly SpaceLifecycle[]>> = {
  pending_registration: ['active', 'failed_registration', 'deleting'],
  active: ['suspended', 'deleting'],
  suspended: ['active', 'deleting'],
  deleting: ['deleted'],
  deleted: [],
  failed_registration: ['pending_registration', 'deleting'],
};

function assertPositiveRevision(revision: number): void {
  if (!Number.isSafeInteger(revision) || revision <= 0) {
    throw new Error('Space lifecycle revision must be a positive safe integer');
  }
}

export function createSpaceLifecycleEvent(input: Omit<SpaceLifecycleEvent, 'eventId'>): SpaceLifecycleEvent {
  if (!input.externalOrgId || !input.spaceRef) {
    throw new Error('Space lifecycle identity is required');
  }
  assertPositiveRevision(input.revision);
  return {
    ...input,
    eventId: `space:${input.spaceRef}:lifecycle:${input.revision}`,
  };
}

export function transitionSpaceLifecycle(
  state: SpaceLifecycleState,
  lifecycle: SpaceLifecycle,
): SpaceLifecycleState {
  assertPositiveRevision(state.revision);
  if (state.lifecycle === 'deleted') {
    throw new Error('cannot transition a deleted Space');
  }
  if (!NEXT_LIFECYCLES[state.lifecycle].includes(lifecycle)) {
    throw new Error(`invalid Space lifecycle transition: ${state.lifecycle} -> ${lifecycle}`);
  }
  return { lifecycle, revision: state.revision + 1 };
}

/**
 * Returns whether a lifecycle outbox row may be claimed at `now`. A claim is
 * recoverable: a worker death only delays delivery until its lease expires.
 */
export function canClaimSpaceLifecycleDelivery(
  delivery: SpaceLifecycleDelivery,
  now: number,
): boolean {
  if (!Number.isSafeInteger(now) || now <= 0) {
    throw new Error('delivery claim time must be a positive safe integer');
  }
  if (delivery.state === 'pending' || delivery.state === 'failed') {
    return delivery.nextAttemptAt <= now;
  }
  return delivery.state === 'claimed' && Boolean(delivery.leaseExpiresAt && delivery.leaseExpiresAt <= now);
}

export function claimSpaceLifecycleDelivery(
  delivery: SpaceLifecycleDelivery,
  workerId: string,
  now: number,
  leaseMs: number,
): SpaceLifecycleDelivery {
  if (!workerId.trim()) throw new Error('delivery worker identity is required');
  if (!Number.isSafeInteger(leaseMs) || leaseMs <= 0) {
    throw new Error('delivery lease must be a positive safe integer');
  }
  if (!canClaimSpaceLifecycleDelivery(delivery, now)) {
    throw new Error('Space lifecycle delivery is not claimable');
  }
  return {
    attempts: delivery.attempts + 1,
    leaseExpiresAt: now + leaseMs,
    leaseOwner: workerId,
    nextAttemptAt: now,
    state: 'claimed',
  };
}

export function releaseSpaceLifecycleDelivery(
  delivery: SpaceLifecycleDelivery,
  workerId: string,
  now: number,
): SpaceLifecycleDelivery {
  if (delivery.state !== 'claimed' || delivery.leaseOwner !== workerId) {
    throw new Error('only the active delivery worker may release the claim');
  }
  // At-least-once transport: the destination must deduplicate eventId. Backoff
  // caps at one hour so configuration failures do not create a retry storm.
  const delayMs = Math.min(60 * 60 * 1000, 1000 * 2 ** Math.min(delivery.attempts, 12));
  return {
    attempts: delivery.attempts,
    nextAttemptAt: now + delayMs,
    state: 'failed',
  };
}

export function acknowledgeSpaceLifecycleDelivery(
  delivery: SpaceLifecycleDelivery,
  workerId: string,
  now: number,
): SpaceLifecycleDelivery {
  if (delivery.state !== 'claimed' || delivery.leaseOwner !== workerId) {
    throw new Error('only the active delivery worker may acknowledge the claim');
  }
  return {
    attempts: delivery.attempts,
    nextAttemptAt: now,
    state: 'acknowledged',
  };
}

/**
 * A successful HTTP status is not enough to activate Application's canonical
 * Space. Control's receipt must identify exactly the immutable Space event
 * that was claimed; otherwise the delivery stays retryable and no local
 * lifecycle transition occurs.
 */
export function controlRegistrationResourceRef(receipt: unknown, expectedSpaceRef: string): string {
  if (!expectedSpaceRef.trim() || !receipt || typeof receipt !== 'object') {
    throw new Error('Control Space registration receipt is invalid');
  }
  const data = (receipt as { data?: unknown }).data;
  const spaceRef = data && typeof data === 'object'
    ? (data as { space_ref?: unknown }).space_ref
    : undefined;
  if (typeof spaceRef !== 'string' || !spaceRef.trim() || spaceRef.trim() !== expectedSpaceRef.trim()) {
    throw new Error('Control Space registration receipt did not match the claimed Space');
  }
  return spaceRef.trim();
}
