import { anyReloadGuardDirty, registerReloadGuard, reloadGuardCount } from '@/app/chunk-recovery'

/**
 * Unsaved-work registry (WS07-S3). A reload, automatic or not, throws away state a
 * page holds only in memory (unsent composer text, a running stream, attachments
 * not yet sent). A feature that holds such state registers a probe under a stable
 * key; the client-contract check (and chunk recovery, which shares the same
 * registry) never reloads the tab for the user while any probe answers `true`.
 *
 * Registering under a key that is already registered replaces that probe, so a
 * remounted component never leaves a stale one behind. A probe that throws counts
 * as dirty: losing someone's draft is worse than showing a banner.
 *
 * Nothing is persisted. Probes live only as long as the page.
 */

const probes = new Map<string, () => boolean>()
const unregisters = new Map<string, () => void>()

/** Register `probe` (true while there is unsaved work) under `key`. Returns the unregister function. */
export function registerUnsavedWork(key: string, probe: () => boolean): () => void {
  unregisters.get(key)?.()
  probes.set(key, probe)
  const release = registerReloadGuard(probe)
  const unregister = () => {
    // Only the owner of the current registration may remove it (a replaced probe's
    // late cleanup must not drop its successor).
    if (probes.get(key) !== probe) return
    probes.delete(key)
    unregisters.delete(key)
    release()
  }
  unregisters.set(key, unregister)
  return unregister
}

/** True when at least one probe is registered; without any, no reload can be judged safe. */
export const hasUnsavedWorkProbes = (): boolean => reloadGuardCount() > 0

/** True while any registered probe (including chunk-recovery guards) reports unsaved work. */
export const hasUnsavedWork = (): boolean => anyReloadGuardDirty()
