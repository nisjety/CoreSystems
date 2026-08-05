// Shared read-data substrate (Phase 4 PR-1 / A9).
//
// This is a CONTRACT, not a framework: a tiny, dependency-free state machine for
// surfaces that read from a backend that may be live, empty, not-connected,
// unavailable, or not-yet-built. Extracted verbatim (no behavior change) from
// the gold `insights-workspace.ts` so every data-driven v3 surface can share the
// same honesty discipline.
//
// THE HONESTY INVARIANT: `live` describes a CONTRACT that responded with real
// rows. It must NEVER be attached to a rendered metric VALUE that no producer
// actually emitted. Use `listResult`/the builders below so an empty backend
// resolves to `empty`/`not_connected`, never to a fabricated `live` value.

/**
 * The measurement state of a read.
 * - `live` — a real backend contract responded WITH rows.
 * - `empty` — a real backend contract responded with ZERO rows (honest empty).
 * - `not_connected` — the source exists but the org has not connected it.
 * - `unavailable` — the contract failed/timed out (transient).
 * - `planned` — the producer is not built yet (honest "blueprint" state).
 */
export type MeasurementState = 'live' | 'empty' | 'not_connected' | 'unavailable' | 'planned'

/** A read result: the data, the honest state, and a human-readable message. */
export type ResourceResult<T> = {
  data: T
  message: string
  state: MeasurementState
}

/** Default per-read timeout for dashboard reads (ms). */
export const DEFAULT_RESOURCE_TIMEOUT_MS = 3500

/** Build a typed `ResourceResult`. */
export function resourceResult<T>(data: T, state: MeasurementState, message: string): ResourceResult<T> {
  return { data, message, state }
}

/**
 * Resolve a list read into `live` (rows present) or `empty` (zero rows).
 * This is the core anti-fabrication helper: `live` is only ever attached when
 * the backend actually produced rows.
 */
export function listResult<T>(
  items: readonly T[],
  messages: { empty: string; live: string },
): ResourceResult<T[]> {
  const data = [...items]
  return data.length > 0
    ? { data, message: messages.live, state: 'live' }
    : { data, message: messages.empty, state: 'empty' }
}

/** A transient-failure result (contract failed or timed out). */
export function unavailableResult<T>(message: string, fallbackData: T): ResourceResult<T> {
  return { data: fallbackData, message, state: 'unavailable' }
}

/** The source exists but the org has not connected it. */
export function notConnectedResult<T>(message: string, fallbackData: T): ResourceResult<T> {
  return { data: fallbackData, message, state: 'not_connected' }
}

/** The producer is not built yet — an honest "blueprint / planned" state. */
export function plannedResult<T>(message: string, fallbackData: T): ResourceResult<T> {
  return { data: fallbackData, message, state: 'planned' }
}

/**
 * Race a resource read against a timeout, returning `fallback` if it does not
 * settle in time. The timer is always cleared so it never leaks.
 */
export async function withResourceTimeout<T>(
  resource: Promise<ResourceResult<T>>,
  fallback: ResourceResult<T>,
  timeoutMs = DEFAULT_RESOURCE_TIMEOUT_MS,
): Promise<ResourceResult<T>> {
  let timeout: ReturnType<typeof setTimeout> | undefined
  const timeoutResult = new Promise<ResourceResult<T>>((resolve) => {
    timeout = setTimeout(() => resolve(fallback), timeoutMs)
  })

  try {
    return await Promise.race([resource, timeoutResult])
  } finally {
    if (timeout) clearTimeout(timeout)
  }
}

/** True when a state represents real, produced rows safe to render as measured. */
export function isMeasured(state: MeasurementState): boolean {
  return state === 'live'
}
