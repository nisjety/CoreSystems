import { createEffect, createMemo, createSignal, onCleanup, untrack } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import {
  compileBrowserProcedure,
  listBrowserEgressReceipts,
  listBrowserReceipts,
  type BrowserEgressReceiptView,
  type BrowserProcedureResponse,
  type BrowserReceiptView,
} from '@/shared/api/browser-client'
import { getSessionContext } from '@/shared/api/auth-client'

/**
 * Data hooks for the evidence panels of the browser chrome (WS9.D).
 *
 * Rules these hooks keep:
 * - Nothing is written to browser storage. State lives in the hook and is dropped on
 *   unmount or when the session changes.
 * - Egress polling runs every {@link EVIDENCE_POLL_INTERVAL_MS} only while the document
 *   is visible, and the in-flight request is aborted on stop or session change.
 * - A zero-data-retention (ZDR) session sends the live egress read only. Receipts and
 *   the procedure compile are never requested for it; the server refuses them anyway.
 */

export const EVIDENCE_POLL_INTERVAL_MS = 5000
export const EGRESS_PAGE_LIMIT = 200
export const EGRESS_ROWS_MAX = 500
export const RECEIPTS_LIST_LIMIT = 20

export type EvidenceSource = {
  orgId: string
  sessionId: string
  zdr: boolean
}

export type EgressState = {
  rows: BrowserEgressReceiptView[]
  counts: { allow: number; block: number }
  /** Cursor for the next read; undefined until the first page has arrived. */
  cursor?: number
  zdr: boolean
}

export function emptyEgressState(): EgressState {
  return { rows: [], counts: { allow: 0, block: 0 }, zdr: false }
}

/** Merge one page of egress decisions into the state. Keeps the newest {@link EGRESS_ROWS_MAX} rows. */
export function mergeEgressPage(
  state: EgressState,
  page: { receipts: BrowserEgressReceiptView[]; counts?: { allow: number; block: number }; nextSequence?: number; zdr?: boolean },
): EgressState {
  const rows = [...state.rows, ...page.receipts].slice(-EGRESS_ROWS_MAX)
  const cursor = typeof page.nextSequence === 'number' ? Math.max(state.cursor ?? 0, page.nextSequence) : state.cursor
  return {
    rows,
    counts: {
      allow: state.counts.allow + (page.counts?.allow ?? 0),
      block: state.counts.block + (page.counts?.block ?? 0),
    },
    cursor,
    zdr: page.zdr ?? state.zdr,
  }
}

/** The document is visible unless it is explicitly hidden. Non-browser runs count as visible. */
export function isDocumentVisible(): boolean {
  if (typeof document === 'undefined') return true
  return document.visibilityState !== 'hidden'
}

export type EgressPollOptions = {
  source: EvidenceSource
  onState: (state: EgressState) => void
  onError: (error: unknown) => void
  intervalMs?: number
}

/**
 * Poll the live egress decisions of one session. Returns the stop function, which aborts
 * the in-flight read and removes the visibility listener. Plain function, no Solid
 * dependency, so the polling rules can be tested directly.
 */
export function startEgressPolling(options: EgressPollOptions): () => void {
  const { source } = options
  const intervalMs = options.intervalMs ?? EVIDENCE_POLL_INTERVAL_MS
  let state = emptyEgressState()
  let stopped = false
  let timer: ReturnType<typeof setTimeout> | undefined
  let inflight: AbortController | undefined

  const clearTimer = () => {
    if (timer !== undefined) clearTimeout(timer)
    timer = undefined
  }

  const schedule = (delay: number) => {
    if (stopped) return
    clearTimer()
    timer = setTimeout(() => {
      timer = undefined
      void poll()
    }, delay)
  }

  const poll = async (): Promise<void> => {
    if (stopped || inflight) return
    // Hidden: do not read. The visibilitychange listener resumes polling when the page is back.
    if (!isDocumentVisible()) return
    const controller = new AbortController()
    inflight = controller
    try {
      const page = await listBrowserEgressReceipts(
        source.orgId,
        source.sessionId,
        { afterSequence: state.cursor, limit: EGRESS_PAGE_LIMIT },
        controller.signal,
      )
      if (stopped) return
      state = mergeEgressPage(state, page)
      options.onState(state)
    } catch (error) {
      if (stopped || controller.signal.aborted) return
      options.onError(error)
    } finally {
      if (inflight === controller) inflight = undefined
    }
    schedule(intervalMs)
  }

  const onVisibility = () => {
    if (stopped || inflight || timer !== undefined) return
    if (isDocumentVisible()) void poll()
  }

  if (typeof document !== 'undefined') document.addEventListener('visibilitychange', onVisibility)
  void poll()

  return () => {
    stopped = true
    clearTimer()
    inflight?.abort()
    inflight = undefined
    if (typeof document !== 'undefined') document.removeEventListener('visibilitychange', onVisibility)
  }
}

function scopeKey(source: EvidenceSource | null): string {
  return source ? `${source.orgId}\u0000${source.sessionId}\u0000${source.zdr ? '1' : '0'}` : ''
}

/** Org id of the signed-in user, as the browser panels send it. Empty when unknown. */
export async function loadEvidenceOrgId(): Promise<string> {
  try {
    const ctx = await getSessionContext()
    return ctx.orgs[0]?.id ?? ctx.orgId ?? ''
  } catch {
    return ''
  }
}

/**
 * The evidence scope of a session: null until both the session id and the org id are
 * known. Pass the result to the other hooks of this file.
 */
export function useBrowserEvidenceScope(sessionId: () => string, zdr: () => boolean): () => EvidenceSource | null {
  const [orgId] = createResource(loadEvidenceOrgId)
  return createMemo(() => {
    const id = sessionId()
    const org = orgId()
    if (!id || !org) return null
    return { orgId: org, sessionId: id, zdr: zdr() }
  })
}

/** Live egress decisions of the scoped session, polled while mounted and visible. */
export function useBrowserEgress(scope: () => EvidenceSource | null): {
  state: () => EgressState
  error: () => unknown
} {
  const [state, setState] = createSignal<EgressState>(emptyEgressState())
  const [error, setError] = createSignal<unknown>(null)
  const key = createMemo(() => scopeKey(scope()))
  let stop: (() => void) | undefined

  createEffect(
    () => key(),
    (current) => {
      stop?.()
      stop = undefined
      setState(emptyEgressState())
      setError(null)
      if (!current) return
      const source = untrack(scope)
      if (!source) return
      stop = startEgressPolling({
        source,
        onState: (next) => setState(next),
        onError: (next) => setError(next),
      })
    },
  )

  onCleanup(() => stop?.())
  return { state, error }
}

export type ProcedureStatus = 'idle' | 'running' | 'done' | 'error'

/**
 * On-demand procedure compile for the scoped session. Never runs for a ZDR session and
 * never runs on its own: the panel calls `compile` when the person asks for it.
 */
export function useBrowserProcedure(scope: () => EvidenceSource | null): {
  status: () => ProcedureStatus
  result: () => BrowserProcedureResponse | null
  error: () => unknown
  compile: () => void
} {
  const [status, setStatus] = createSignal<ProcedureStatus>('idle')
  const [result, setResult] = createSignal<BrowserProcedureResponse | null>(null)
  const [error, setError] = createSignal<unknown>(null)
  const key = createMemo(() => scopeKey(scope()))
  let inflight: AbortController | undefined

  const abort = () => {
    inflight?.abort()
    inflight = undefined
  }

  createEffect(
    () => key(),
    () => {
      abort()
      setStatus('idle')
      setResult(null)
      setError(null)
    },
  )
  onCleanup(abort)

  const compile = () => {
    const source = scope()
    if (!source || source.zdr) return
    if (status() === 'running') return
    abort()
    const controller = new AbortController()
    inflight = controller
    setStatus('running')
    setError(null)
    compileBrowserProcedure(source.orgId, source.sessionId, undefined, controller.signal).then(
      (response) => {
        if (controller.signal.aborted) return
        inflight = undefined
        setResult(response)
        setStatus('done')
      },
      (failure: unknown) => {
        if (controller.signal.aborted) return
        inflight = undefined
        setError(failure)
        setStatus('error')
      },
    )
  }

  return { status, result, error, compile }
}

/**
 * The most recent receipts of the scoped session, read once when the scope is known.
 * Not requested for a ZDR session.
 */
export function useBrowserReceipts(scope: () => EvidenceSource | null): {
  receipts: () => BrowserReceiptView[]
  truncated: () => boolean
  error: () => unknown
  loading: () => boolean
} {
  const [receipts, setReceipts] = createSignal<BrowserReceiptView[]>([])
  const [truncated, setTruncated] = createSignal(false)
  const [error, setError] = createSignal<unknown>(null)
  const [loading, setLoading] = createSignal(false)
  const key = createMemo(() => scopeKey(scope()))
  let inflight: AbortController | undefined

  createEffect(
    () => key(),
    (current) => {
      inflight?.abort()
      inflight = undefined
      setReceipts([])
      setTruncated(false)
      setError(null)
      setLoading(false)
      if (!current) return
      const source = untrack(scope)
      if (!source || source.zdr) return
      const controller = new AbortController()
      inflight = controller
      setLoading(true)
      listBrowserReceipts(source.orgId, source.sessionId, { limit: RECEIPTS_LIST_LIMIT }, controller.signal).then(
        (page) => {
          if (controller.signal.aborted) return
          inflight = undefined
          setReceipts(page.receipts)
          setTruncated(page.truncated)
          setLoading(false)
        },
        (failure: unknown) => {
          if (controller.signal.aborted) return
          inflight = undefined
          setError(failure)
          setLoading(false)
        },
      )
    },
  )
  onCleanup(() => {
    inflight?.abort()
    inflight = undefined
  })

  return { receipts, truncated, error, loading }
}
