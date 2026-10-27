import { createSignal } from 'solid-js'

/**
 * Recovery from a stale build under an open tab (WS09 S5).
 *
 * A deploy replaces the hashed chunk files, so a tab opened before it fails the
 * next lazy `import()` with a 404. Vite announces that as `vite:preloadError`
 * (production builds only). The first failure reloads the page once, so the tab
 * picks up the new build; a second failure inside the window shows a notice
 * instead, because a reload that did not fix it will not fix it twice.
 *
 * A reload throws away whatever the page holds only in memory. Features that
 * hold such state register a guard; while any guard says it is dirty the page is
 * never reloaded for the user, and the notice offers the reload instead.
 *
 * Persists one timestamp (when the last automatic reload happened), nothing else.
 */

export const CHUNK_RELOAD_KEY = 'verevon.chunk-reload.v1'

/** A second failure this soon after the automatic reload means the reload did not help. */
export const CHUNK_RELOAD_WINDOW_MS = 30_000

/** `VITE_CHUNK_RECOVERY_DISABLED`: only the literal `true` turns recovery off. */
const chunkRecoveryDisabled = () => import.meta.env.VITE_CHUNK_RECOVERY_DISABLED === 'true'

const guards = new Set<() => boolean>()
const [noticeVisible, setNoticeVisible] = createSignal(false)

/** Whether the notice that a newer build is available is showing. */
export const staleBuildNotice = noticeVisible

/** Hide the notice (the user chose to keep working on the old build). */
export const dismissStaleBuildNotice = () => setNoticeVisible(false)

/**
 * Register a check that is true while the page holds state a reload would lose
 * (unsent composer text, a running stream, attachments not yet sent). Returns
 * the unregister function. A guard that throws counts as dirty: losing someone's
 * draft is worse than showing the notice.
 */
export function registerReloadGuard(isDirty: () => boolean): () => void {
  guards.add(isDirty)
  return () => {
    guards.delete(isDirty)
  }
}

/** How many guards are registered. Without any, no reload can be judged safe. */
export const reloadGuardCount = (): number => guards.size

export const anyReloadGuardDirty = () => {
  for (const guard of guards) {
    try {
      if (guard()) return true
    } catch {
      return true
    }
  }
  return false
}

/** True when an automatic reload already happened inside the window (or this can not be told). */
const reloadedRecently = (now: number): boolean => {
  try {
    const raw = window.sessionStorage.getItem(CHUNK_RELOAD_KEY)
    if (raw === null) return false
    const at = Number(raw)
    if (!Number.isFinite(at)) return false
    return now - at < CHUNK_RELOAD_WINDOW_MS
  } catch {
    // Without the marker a failing reload could repeat forever, so a tab that
    // cannot keep it never reloads on its own.
    return true
  }
}

const markReload = (now: number): boolean => {
  try {
    window.sessionStorage.setItem(CHUNK_RELOAD_KEY, String(now))
    return true
  } catch {
    return false
  }
}

/**
 * Reload once for a stale build, for a caller that has already decided a reload is
 * wanted (the `enforce` client-contract policy, WS07-S3). Shares the chunk-reload
 * marker and window, so it can never loop with `vite:preloadError` or the boot
 * watchdog. Returns false, and does nothing, when a guard is dirty, a reload
 * already happened inside the window, or the marker cannot be written.
 */
export function reloadOnceForStaleBuild(reload: () => void = () => window.location.reload()): boolean {
  const now = Date.now()
  if (chunkRecoveryDisabled() || anyReloadGuardDirty() || reloadedRecently(now) || !markReload(now)) return false
  reload()
  return true
}

/**
 * Listen for `vite:preloadError`. Returns the function that removes the listener.
 * The event is left un-prevented on every path that does not reload, so the
 * failed import still rejects and the caller's own error handling runs.
 */
export function installChunkRecovery(reload: () => void = () => window.location.reload()): () => void {
  if (chunkRecoveryDisabled()) return () => undefined
  const onPreloadError = (event: Event) => {
    const now = Date.now()
    if (anyReloadGuardDirty() || reloadedRecently(now) || !markReload(now)) {
      setNoticeVisible(true)
      return
    }
    event.preventDefault()
    reload()
  }
  window.addEventListener('vite:preloadError', onPreloadError)
  return () => window.removeEventListener('vite:preloadError', onPreloadError)
}
