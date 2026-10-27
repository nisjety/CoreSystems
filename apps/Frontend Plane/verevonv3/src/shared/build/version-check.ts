import { createSignal } from 'solid-js'
import { BUILD_ID, isDevBuild } from '@/shared/build/build-info'

/**
 * Stale-tab check (WS07-S2). A tab that stays open across a deploy keeps running
 * the old bundle. `/version.json` (served `no-store` by nginx) names the live
 * build; when it differs from the compiled-in BUILD_ID the non-blocking
 * NewVersionBanner is shown. Never reloads by itself: the user chooses when.
 *
 * Checks run at start, when the tab becomes visible after at least 5 minutes
 * hidden, on `online`, every 15 minutes while visible, and when a caller (the
 * stale_deploy failure card, later the SSE reconnect) calls `checkClientVersion`.
 * A response that is not JSON is ignored: until nginx serves the file, the SPA
 * fallback answers this URL with index.html.
 *
 * Persists one sessionStorage value: the build id whose banner the user dismissed.
 */

export const VERSION_URL = '/version.json'
export const DISMISSED_KEY = 'verevon.new-version-dismissed'
export const HIDDEN_RECHECK_MS = 5 * 60_000
export const POLL_INTERVAL_MS = 15 * 60_000
/** Two checks closer than this collapse into one (visibility + online bursts). */
export const MIN_CHECK_GAP_MS = 30_000

const [newBuildId, setNewBuildId] = createSignal<string | null>(null)

/** The live build id when it differs from this tab's and was not dismissed, else null. */
export const newVersionBuildId = newBuildId

const readDismissed = (): string | null => {
  try {
    return window.sessionStorage.getItem(DISMISSED_KEY)
  } catch {
    return null
  }
}

/** Hide the banner for this session. Keyed by the new build id, so a later deploy shows it again. */
export function dismissNewVersion(): void {
  const id = newBuildId()
  if (id !== null) {
    try {
      window.sessionStorage.setItem(DISMISSED_KEY, id)
    } catch {
      // Dismissal is per-render only when storage is blocked.
    }
  }
  setNewBuildId(null)
}

let lastCheckAt = 0
let inFlight: Promise<void> | null = null

/** Parse the version.json body; null for anything that is not `{build_id: string}`. */
export function liveBuildIdFrom(body: unknown): string | null {
  if (typeof body !== 'object' || body === null) return null
  const id = (body as { build_id?: unknown }).build_id
  return typeof id === 'string' && id !== '' ? id : null
}

/**
 * Fetch `/version.json` and update the banner state. Never throws, never reloads.
 * `force` skips the throttle (used after a stale_deploy card, where the answer matters now).
 */
export function checkClientVersion(options: { force?: boolean } = {}): Promise<void> {
  if (isDevBuild()) return Promise.resolve()
  const now = Date.now()
  if (inFlight) return inFlight
  if (options.force !== true && now - lastCheckAt < MIN_CHECK_GAP_MS) return Promise.resolve()
  lastCheckAt = now
  inFlight = (async () => {
    try {
      const res = await fetch(VERSION_URL, { cache: 'no-store', credentials: 'omit' })
      const type = res.headers.get('content-type') ?? ''
      if (!res.ok || !type.includes('json')) return
      const live = liveBuildIdFrom(await res.json())
      if (live === null || live === BUILD_ID || live === readDismissed()) {
        if (live === BUILD_ID) setNewBuildId(null)
        return
      }
      setNewBuildId(live)
    } catch {
      // Offline or blocked: the next trigger tries again.
    } finally {
      inFlight = null
    }
  })()
  return inFlight
}

/**
 * Register the triggers. Returns the function that removes them. Call once, after
 * the first render (index.tsx).
 */
export function startVersionCheck(): () => void {
  if (isDevBuild()) return () => undefined
  let hiddenAt: number | null = null
  const onVisibility = () => {
    if (document.visibilityState === 'hidden') {
      hiddenAt = Date.now()
      return
    }
    if (hiddenAt !== null && Date.now() - hiddenAt >= HIDDEN_RECHECK_MS) void checkClientVersion()
    hiddenAt = null
  }
  const onOnline = () => void checkClientVersion()
  document.addEventListener('visibilitychange', onVisibility)
  window.addEventListener('online', onOnline)
  const timer = window.setInterval(() => {
    if (document.visibilityState === 'visible') void checkClientVersion()
  }, POLL_INTERVAL_MS)
  void checkClientVersion()
  return () => {
    document.removeEventListener('visibilitychange', onVisibility)
    window.removeEventListener('online', onOnline)
    window.clearInterval(timer)
  }
}
