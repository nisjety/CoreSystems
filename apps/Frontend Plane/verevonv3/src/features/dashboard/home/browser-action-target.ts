import type { BrowserTraceEvent } from '@/shared/api/browser-client'

export type BrowserActionTarget = { x: number; y: number; width: number; height: number }

/** Only show a recent, finite target from the current stream, never replayed or guessed coordinates. */
export function browserActionTarget(event: BrowserTraceEvent, subscribedAt: number, now: number): BrowserActionTarget | null {
  const at = Date.parse(event.ts)
  if (!Number.isFinite(at) || at < subscribedAt - 1000 || at < now - 3000 || at > now + 1000) return null
  const rect = event.payload.rect
  if (!rect || typeof rect !== 'object' || Array.isArray(rect)) return null
  const { x, y, width, height } = rect as Record<string, unknown>
  if (typeof x !== 'number' || typeof y !== 'number' || typeof width !== 'number' || typeof height !== 'number') return null
  if (![x, y, width, height].every(Number.isFinite) || width <= 0 || height <= 0) return null
  return { x, y, width, height }
}
