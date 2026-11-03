import type { BaselineSnapshot } from '@/shared/api/monitoring-client'

/**
 * Browse AI-style change timeline. Quarry keeps only content fingerprints per
 * baseline (no stored text), so "changed" means the fingerprint differs from the
 * next older baseline. The oldest baseline has nothing to compare against.
 */
export type TimelineEntry = BaselineSnapshot & { change: 'changed' | 'unchanged' | 'first' }

/** `history` is newest first, as the gateway returns it. */
export function changeTimeline(history: BaselineSnapshot[]): TimelineEntry[] {
  return history.map((snapshot, index) => {
    const older = history[index + 1]
    if (!older) return { ...snapshot, change: 'first' }
    return { ...snapshot, change: older.fingerprint === snapshot.fingerprint ? 'unchanged' : 'changed' }
  })
}

/** How many content changes the timeline holds. */
export function changeCount(timeline: TimelineEntry[]): number {
  return timeline.filter((entry) => entry.change === 'changed').length
}

/** Fingerprints look like `blake3:<64 hex>`; show the algorithm and a short prefix. */
export function shortFingerprint(value: string): string {
  const colon = value.indexOf(':')
  const algo = colon >= 0 ? value.slice(0, colon) : ''
  const hash = colon >= 0 ? value.slice(colon + 1) : value
  const head = hash.slice(0, 12)
  return algo ? `${algo}:${head}…` : `${head}…`
}
