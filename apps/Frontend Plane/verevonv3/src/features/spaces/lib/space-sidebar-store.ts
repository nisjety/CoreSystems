import { createSignal, type Accessor } from 'solid-js'

import {
  emptySidebarDoc,
  parseSidebarDoc,
  type SidebarAction,
  type SidebarDoc,
} from '@/shared/actions/space-sidebar'
import {
  getSpaceSidebar,
  putSpaceSidebar,
  sidebarConflictFrom,
  type SpaceSidebarSaved,
  type SpaceSidebarSnapshot,
} from '@/shared/api/spaces-sidebar-client'
import { ApiError } from '@/shared/api/http'
import { LocalizedError } from '@/shared/i18n/errors'
import { readClientValue, writeClientJson } from '@/shared/session/client-storage'
import { applySidebarAction, pruneDoc, sidebarDocsEqual } from './space-sidebar-organization'

// WS16-S9: the store behind the personal Spaces sidebar (docs/plans WS16 D8.5).
//
// Boot renders from the local cache, then GETs. An action applies to the local document at once and
// is sent as a debounced `PUT` carrying the last server revision. On a 409 the store re-applies its
// un-acknowledged actions on the document the server returned, with the same reducer, and retries; a
// conflict whose returned document equals the one sent counts as success (a replayed PUT).
// Cross-tab: a BroadcastChannel carries only { scope, revision } and triggers a refetch; cross-device:
// a refetch after 60 s hidden and on `online`. There is no realtime push: last writer wins, with rebase.
//
// Modes:
//   - `server`    the document lives in Application; edits are PUT.
//   - `zdr_local` the organization keeps no durable state (Zero Data Retention). The store NEVER
//                 calls PUT; the document (room refs and the labels the person typed, no content)
//                 stays in this browser and the panel says so.
//   - `disabled`  the gateway flag is off (404): the caller renders the legacy panel.
//   - `loading`   the first GET has not answered; edits are refused until it has.
// Authority is never read from here: hide, star and sort change what is drawn, never what is allowed.

export type SidebarStoreMode = 'loading' | 'server' | 'zdr_local' | 'disabled'
export type SidebarStoreStatus = 'idle' | 'saving' | 'error'

export type SidebarStoreState = {
  readonly mode: SidebarStoreMode
  readonly revision: number
  readonly doc: SidebarDoc
  readonly status: SidebarStoreStatus
  /** False while loading, when the server could not be reached, and in `disabled`. */
  readonly canEdit: boolean
  /** True in `zdr_local`: the panel shows "Saved in this browser only". */
  readonly browserOnly: boolean
}

export type SidebarStoreClient = {
  get: () => Promise<SpaceSidebarSnapshot>
  put: (baseRevision: number, doc: SidebarDoc) => Promise<SpaceSidebarSaved>
}

export type SpaceSidebarStore = {
  state: Accessor<SidebarStoreState>
  /** Applies the actions to the local document as one step (all or none). Throws `LocalizedError`. */
  dispatch: (...actions: SidebarAction[]) => void
  /** The rooms the viewer can see, so the next write can prune references to rooms they cannot. */
  setVisibleRefs: (refs: ReadonlySet<string> | null) => void
  refresh: () => Promise<void>
  dispose: () => void
}

export const SPACE_SIDEBAR_DEBOUNCE_MS = 400
export const SPACE_SIDEBAR_STALE_AFTER_MS = 60_000
const RETRY_AFTER_ERROR_MS = 5_000
const MAX_REBASES = 2
const CHANNEL_NAME = 'verevon-space-sidebar'

export function spaceSidebarStorageKey(orgId: string | undefined, userId: string | undefined): string | null {
  return orgId && userId
    ? `verevon.space-sidebar.v1:${encodeURIComponent(orgId)}:${encodeURIComponent(userId)}`
    : null
}

type Op = SidebarAction | { readonly type: 'replace'; readonly doc: SidebarDoc }

type CacheEntry = { mode: 'server' | 'zdr_local'; revision: number; doc: SidebarDoc }

function readCache(key: string): CacheEntry | null {
  try {
    const raw = JSON.parse(readClientValue(key) ?? 'null') as Record<string, unknown> | null
    const doc = raw ? parseSidebarDoc(raw.doc) : null
    if (!raw || !doc) return null
    return {
      mode: raw.mode === 'zdr_local' ? 'zdr_local' : 'server',
      revision: typeof raw.revision === 'number' && Number.isInteger(raw.revision) && raw.revision >= 0 ? raw.revision : 0,
      doc,
    }
  } catch {
    return null
  }
}

export function createSpaceSidebarStore(options: {
  /** `spaceSidebarStorageKey(org, user)`. */
  scopeKey: string
  client?: SidebarStoreClient
  debounceMs?: number
}): SpaceSidebarStore {
  const client: SidebarStoreClient = options.client ?? { get: getSpaceSidebar, put: putSpaceSidebar }
  const debounceMs = options.debounceMs ?? SPACE_SIDEBAR_DEBOUNCE_MS
  const key = options.scopeKey

  // Authoritative state lives in plain variables: Solid 2 batches signal writes, so a read right
  // after a write would see the old value. The signal only mirrors it for rendering.
  const cached = readCache(key)
  let mode: SidebarStoreMode = 'loading'
  let revision = cached?.mode === 'server' ? cached.revision : 0
  let doc: SidebarDoc = cached?.doc ?? emptySidebarDoc()
  let status: SidebarStoreStatus = 'idle'
  let loadFailed = false
  let unacked: Op[] = []
  let inflight: Op[] | null = null
  let visibleRefs: ReadonlySet<string> | null = null
  let timer: ReturnType<typeof setTimeout> | undefined
  let retryTimer: ReturnType<typeof setTimeout> | undefined
  let hiddenAt: number | null = null
  let disposed = false
  let refreshing = false

  const snapshot = (): SidebarStoreState => ({
    mode,
    revision,
    doc,
    status,
    canEdit: !loadFailed && (mode === 'zdr_local' || (mode === 'server' && status !== 'error')),
    browserOnly: mode === 'zdr_local',
  })
  const [state, setState] = createSignal<SidebarStoreState>(snapshot())
  const publish = () => {
    if (!disposed) setState(snapshot())
  }
  const saveCache = () => {
    if (mode === 'server' || mode === 'zdr_local') writeClientJson(key, { mode, revision, doc })
  }

  let channel: BroadcastChannel | null = null
  try {
    channel = typeof BroadcastChannel === 'undefined' ? null : new BroadcastChannel(CHANNEL_NAME)
  } catch {
    channel = null
  }
  const pending = () => inflight !== null || unacked.length > 0
  if (channel) {
    channel.onmessage = (event: MessageEvent) => {
      const data = event.data as { scope?: unknown; revision?: unknown } | null
      if (!data || data.scope !== key || mode !== 'server' || pending()) return
      if (typeof data.revision === 'number' && data.revision !== revision) void refresh()
    }
  }

  function applyOps(base: SidebarDoc, ops: readonly Op[]): SidebarDoc {
    let next = base
    for (const op of ops) {
      // A replace is "push my local document into an empty server row"; if someone else wrote
      // first, theirs wins and the replace is dropped.
      if (op.type === 'replace') continue
      try {
        next = applySidebarAction(next, op)
      } catch {
        // The action no longer applies on the newer document (the section was removed, a cap is
        // reached). Dropping it keeps the rebase total.
      }
    }
    return next
  }

  function scheduleFlush(delay = debounceMs) {
    clearTimeout(timer)
    timer = setTimeout(() => void flush(), delay)
  }

  function scheduleRetry() {
    clearTimeout(retryTimer)
    retryTimer = setTimeout(() => {
      if (disposed) return
      if (pending()) void flush()
      else void refresh()
    }, RETRY_AFTER_ERROR_MS)
  }

  async function flush(): Promise<void> {
    clearTimeout(timer)
    if (disposed || mode !== 'server' || inflight !== null || unacked.length === 0) return
    inflight = unacked
    unacked = []
    status = 'saving'
    publish()
    let rebases = 0
    while (inflight !== null && !disposed) {
      const sent = visibleRefs && visibleRefs.size > 0 ? pruneDoc(doc, visibleRefs) : doc
      try {
        const saved = await client.put(revision, sent)
        revision = saved.revision
        inflight = null
        if (unacked.length === 0) doc = sent
        done()
        return
      } catch (error) {
        const conflict = sidebarConflictFrom(error)
        if (conflict) {
          if (conflict.doc && sidebarDocsEqual(conflict.doc, sent)) {
            // Our own PUT, replayed: the server already holds exactly this document.
            revision = conflict.revision
            inflight = null
            if (unacked.length === 0) doc = sent
            done()
            return
          }
          rebases += 1
          if (rebases > MAX_REBASES) {
            giveUp()
            return
          }
          const replay: Op[] = [...(inflight ?? []), ...unacked]
          doc = applyOps(conflict.doc ?? emptySidebarDoc(), replay)
          revision = conflict.revision
          inflight = replay
          unacked = []
          saveCache()
          publish()
          continue
        }
        if (error instanceof ApiError && error.status === 412) {
          // The organization is under Zero Data Retention: no durable write is allowed. Keep the
          // layout in this browser and never try again.
          mode = 'zdr_local'
          revision = 0
          inflight = null
          unacked = []
          status = 'idle'
          saveCache()
          publish()
          return
        }
        if (error instanceof ApiError && error.status === 404) {
          mode = 'disabled'
          inflight = null
          unacked = []
          status = 'idle'
          publish()
          return
        }
        if (error instanceof ApiError && error.status === 400) {
          // The server refused the document itself. Local edits that built it are discarded and the
          // stored layout is shown again.
          inflight = null
          unacked = []
          status = 'error'
          publish()
          void refresh()
          return
        }
        // Network, 5xx, 403: keep the edits for a retry and hold the cached document read-only.
        unacked = [...(inflight ?? []), ...unacked]
        inflight = null
        status = 'error'
        publish()
        scheduleRetry()
        return
      }
    }
  }

  function done() {
    status = 'idle'
    saveCache()
    publish()
    try {
      channel?.postMessage({ scope: key, revision })
    } catch {
      // A closed channel is not an error for the person.
    }
    if (unacked.length > 0) scheduleFlush(0)
  }

  function giveUp() {
    // Repeated conflicts: stop fighting, show what the server holds.
    inflight = null
    unacked = []
    status = 'error'
    publish()
    void refresh()
  }

  async function refresh(): Promise<void> {
    if (disposed || refreshing) return
    if (pending()) {
      void flush()
      return
    }
    refreshing = true
    try {
      const snap = await client.get()
      if (disposed) return
      if (pending()) return // an edit began while the GET was in flight: it wins this round
      loadFailed = false
      status = 'idle'
      if (snap.mode === 'zdr_local') {
        mode = 'zdr_local'
        revision = 0
        // The document is whatever this browser holds (the cache was read at boot).
        doc = readCache(key)?.doc ?? doc
      } else {
        mode = 'server'
        if (snap.doc) {
          doc = snap.doc
          revision = snap.revision
        } else {
          revision = snap.revision
          // Nothing stored yet. A layout built while the organization was under ZDR (or before the
          // flag) is pushed once; the server row then exists.
          if (!sidebarDocsEqual(doc, emptySidebarDoc())) {
            unacked = [{ type: 'replace', doc }]
            scheduleFlush(0)
          }
        }
      }
      saveCache()
      publish()
    } catch (error) {
      if (disposed) return
      if (error instanceof ApiError && error.status === 404) {
        mode = 'disabled'
        loadFailed = false
      } else {
        // Offline, 403, 503: show the cached document read-only; never block the shell on this.
        if (mode === 'loading') mode = cached?.mode ?? 'server'
        loadFailed = true
        status = 'error'
        scheduleRetry()
      }
      publish()
    } finally {
      refreshing = false
    }
  }

  const onVisibility = () => {
    if (typeof document === 'undefined') return
    if (document.visibilityState === 'hidden') {
      hiddenAt = Date.now()
      return
    }
    const away = hiddenAt === null ? 0 : Date.now() - hiddenAt
    hiddenAt = null
    if (away >= SPACE_SIDEBAR_STALE_AFTER_MS) void refresh()
  }
  const onOnline = () => {
    if (pending()) void flush()
    else void refresh()
  }
  const onStorage = (event: StorageEvent) => {
    // Another tab of a ZDR-local user changed the shared browser copy.
    if (mode !== 'zdr_local' || pending() || (event.key !== key && event.key !== null)) return
    const next = readCache(key)
    if (next && !sidebarDocsEqual(next.doc, doc)) {
      doc = next.doc
      publish()
    }
  }
  if (typeof document !== 'undefined') document.addEventListener('visibilitychange', onVisibility)
  if (typeof window !== 'undefined') {
    window.addEventListener('online', onOnline)
    window.addEventListener('storage', onStorage)
  }

  void refresh()

  return {
    state,
    dispatch(...actions) {
      const current = snapshot()
      if (!current.canEdit) {
        throw new LocalizedError({
          no: 'Oppsettet kan ikke lagres akkurat nå. Prøv igjen om litt.',
          en: 'Your layout cannot be saved right now. Try again in a moment.',
        })
      }
      // All or nothing: the reducer throws before anything is committed.
      const next = actions.reduce<SidebarDoc>((acc, action) => applySidebarAction(acc, action), doc)
      if (next === doc) return
      doc = next
      if (mode === 'server') {
        unacked = [...unacked, ...actions]
        scheduleFlush()
      }
      saveCache()
      publish()
    },
    setVisibleRefs(refs) {
      visibleRefs = refs
    },
    refresh,
    dispose() {
      disposed = true
      clearTimeout(timer)
      clearTimeout(retryTimer)
      channel?.close()
      if (typeof document !== 'undefined') document.removeEventListener('visibilitychange', onVisibility)
      if (typeof window !== 'undefined') {
        window.removeEventListener('online', onOnline)
        window.removeEventListener('storage', onStorage)
      }
    },
  }
}
