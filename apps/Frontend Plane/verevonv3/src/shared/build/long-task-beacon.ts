import { BUILD_ID } from '@/shared/build/build-info'

/**
 * Content-free long-task beacon (F-t3-096). A PerformanceObserver on `longtask` entries (main-thread
 * tasks of 50 ms or more) sorts each task into a duration bucket. A one-minute timer then sends at most
 * one beacon per bucket that saw a task in that minute, to the gateway's client-diagnostics route:
 *
 *   POST /api/v1/client-diagnostics  {"kind":"long_task","build":"<build id>","bucket":"250_1000"}
 *
 * That is the whole payload. It carries no entry name, no attribution, no URL or route, and no user or
 * session id. The gateway stores nothing; it only increments `verevon_client_long_tasks_total{bucket,
 * build_match}`.
 *
 * Off unless the build sets `VITE_LONG_TASK_BEACON=true` (the literal `true`). App.tsx starts it once.
 */

/** The closed duration buckets in milliseconds: 50 to under 100, 100 to under 250, 250 to under 1000, 1000 and up. */
export const LONG_TASK_BUCKETS = ['50_100', '100_250', '250_1000', '1000_plus'] as const

export type LongTaskBucket = (typeof LONG_TASK_BUCKETS)[number]

/** The aggregation window: at most one beacon per bucket per window. */
export const LONG_TASK_WINDOW_MS = 60_000

/** The same content-free endpoint the failure beacons use. */
export const LONG_TASK_BEACON_URL = '/api/v1/client-diagnostics'

/** The bucket of one task, or null when it is shorter than the 50 ms `longtask` threshold. */
export function longTaskBucket(durationMs: number): LongTaskBucket | null {
  if (!(durationMs >= 50)) return null
  if (durationMs < 100) return '50_100'
  if (durationMs < 250) return '100_250'
  if (durationMs < 1000) return '250_1000'
  return '1000_plus'
}

/** True only for the literal `true` in the build (VITE_LONG_TASK_BEACON). */
export function longTaskBeaconEnabled(): boolean {
  return import.meta.env.VITE_LONG_TASK_BEACON === 'true'
}

/** Send one beacon. Best effort: a missing or refused sendBeacon is dropped without a trace. */
function sendLongTaskBeacon(bucket: LongTaskBucket): void {
  try {
    const payload = JSON.stringify({ kind: 'long_task', build: BUILD_ID, bucket })
    const body = new Blob([payload], { type: 'application/json' })
    if (typeof navigator.sendBeacon === 'function') navigator.sendBeacon(LONG_TASK_BEACON_URL, body)
  } catch {
    // Diagnostics never break the page.
  }
}

/** Observe main-thread tasks, including those buffered before the observer started. Null where the browser cannot. */
function observeLongTasks(onEntries: PerformanceObserverCallback): PerformanceObserver | null {
  if (typeof PerformanceObserver === 'undefined') return null
  const supported: readonly string[] = PerformanceObserver.supportedEntryTypes ?? []
  if (!supported.includes('longtask')) return null
  try {
    const observer = new PerformanceObserver(onEntries)
    observer.observe({ type: 'longtask', buffered: true })
    return observer
  } catch {
    return null
  }
}

let started = false

/**
 * Start the observer and the one-minute flush, and return the function that stops both. A no-op (it
 * returns a no-op stop) when the flag is off, the browser cannot observe long tasks, or the beacon has
 * already started.
 */
export function startLongTaskBeacon(): () => void {
  const noop = () => undefined
  if (started || !longTaskBeaconEnabled()) return noop
  const pending = new Set<LongTaskBucket>()
  const observer = observeLongTasks((list) => {
    for (const entry of list.getEntries()) {
      const bucket = longTaskBucket(entry.duration)
      if (bucket !== null) pending.add(bucket)
    }
  })
  if (observer === null) return noop
  started = true
  const flush = () => {
    for (const bucket of pending) sendLongTaskBeacon(bucket)
    pending.clear()
  }
  const timer = window.setInterval(flush, LONG_TASK_WINDOW_MS)
  return () => {
    observer.disconnect()
    window.clearInterval(timer)
    pending.clear()
    started = false
  }
}
