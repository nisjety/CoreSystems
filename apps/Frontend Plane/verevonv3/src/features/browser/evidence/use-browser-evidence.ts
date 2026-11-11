import { createEffect, createMemo, createSignal, onCleanup, untrack } from 'solid-js'
import type { z } from 'zod'
import { createResource } from '@/shared/lib/create-resource-compat'
import {
  compileBrowserProcedure,
  listBrowserEgressReceipts,
  listBrowserReceipts,
  listBrowserRecipes,
  type BrowserEgressReceiptView,
  type BrowserProcedureResponse,
  type BrowserReceiptView,
  type BrowserSavedRecipe,
} from '@/shared/api/browser-client'
import { createAiRunStartKeyring, startBrowserAiRun, type StartBrowserAiRunParams } from '@/shared/api/browser-run-client'
import { getSessionContext } from '@/shared/api/auth-client'
import { ApiError } from '@/shared/api/http'
import { streamRunEvents } from '@/shared/api/run-console-client'
import { executeAction } from '@/shared/actions/action-client'
import { browserEvidenceSaveRecipeOutput } from '@/shared/actions/action-registry'
import type { ActionActor } from '@/shared/actions/types'
import { LocalizedError } from '@/shared/i18n/errors'
import { getSession } from '@/shared/session/session-store'

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
 * - A recipe is saved only when the person asks for it (`save`), never for a ZDR session.
 * - A saved recipe runs only when the person clicks Run (`runRecipe`). Its slot values go into the
 *   start and are not kept by these hooks.
 * - The saved-recipe list is the person's own recipes, not session content, so it is read in a ZDR
 *   session too; the panel disables Run there.
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

export type RecipeSaveStatus = 'idle' | 'running' | 'done' | 'error'

/** A saved recipe as the gateway answered it: its id, version, digest and the slot names and kinds. */
export type BrowserRecipeSaved = z.infer<typeof browserEvidenceSaveRecipeOutput>

/**
 * Saves the scoped session's compiled procedure as a recipe through the
 * `browser_evidence.save_recipe` action (Q4-E4.2-4b). It runs only when the person asks
 * (`save`), never for a ZDR session, and a save already running is not repeated. An answer
 * that arrives after the scope changed is dropped. The answer is read through the action's
 * output contract, so an answer of another shape is an error and nothing unchecked is shown.
 */
export function useSaveRecipe(scope: () => EvidenceSource | null): {
  status: () => RecipeSaveStatus
  result: () => BrowserRecipeSaved | null
  error: () => unknown
  save: (input: { name: string; description?: string }) => void
} {
  const [status, setStatus] = createSignal<RecipeSaveStatus>('idle')
  const [result, setResult] = createSignal<BrowserRecipeSaved | null>(null)
  const [error, setError] = createSignal<unknown>(null)
  const key = createMemo(() => scopeKey(scope()))
  // Bumped on every scope change, so an answer for a session the panel no longer shows is ignored.
  let generation = 0

  createEffect(
    () => key(),
    () => {
      generation += 1
      setStatus('idle')
      setResult(null)
      setError(null)
    },
  )

  const save = (input: { name: string; description?: string }) => {
    const source = scope()
    if (!source || source.zdr) return
    if (status() === 'running') return
    const userId = getSession().user?.id
    if (!userId) {
      setError(new LocalizedError({ no: 'Logg inn på nytt for å lagre oppskriften.', en: 'Sign in again to save the recipe.' }))
      setStatus('error')
      return
    }
    const current = generation
    setStatus('running')
    setError(null)
    setResult(null)
    const actor: ActionActor = { type: 'human', userId, orgId: source.orgId }
    const description = input.description?.trim()
    executeAction('browser_evidence.save_recipe', actor, {
      sessionId: source.sessionId,
      name: input.name,
      ...(description ? { description } : {}),
    }).then(
      (execution) => {
        if (current !== generation) return
        const parsed = browserEvidenceSaveRecipeOutput.safeParse(execution.result)
        if (!parsed.success) {
          setError(new ApiError('browser_recipe_response_invalid', 502, 'browser_recipe_response_invalid'))
          setStatus('error')
          return
        }
        setResult(parsed.data)
        setStatus('done')
      },
      (failure: unknown) => {
        if (current !== generation) return
        setError(failure)
        setStatus('error')
      },
    )
  }

  return { status, result, error, save }
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

/**
 * Whether the saved-recipe controls are built: the save in the Procedure panel, and the list of
 * saved recipes with its Run. Only the literal `true` of VITE_BROWSER_RECIPES turns them on.
 */
export function recipesEnabled(): boolean {
  return import.meta.env.VITE_BROWSER_RECIPES === 'true'
}

/** The recipes one page of the saved list asks for. The gateway caps a page at 100. */
export const RECIPES_LIST_LIMIT = 20

export type SavedRecipesStatus = 'loading' | 'ready' | 'error'

/**
 * The person's saved recipes, one page at a time (Q4-E4.2-4d). The first page loads once the org
 * id is known. `reload` reads the first page again, which the panel does after a save, and
 * `loadMore` reads the page after `nextAfterId`. The list is the person's own recipes, kept in
 * memory only.
 */
export function useSavedRecipes(): {
  status: () => SavedRecipesStatus
  recipes: () => BrowserSavedRecipe[]
  nextAfterId: () => string | null
  error: () => unknown
  reload: () => void
  loadMore: () => void
} {
  const [orgId] = createResource(loadEvidenceOrgId)
  const [status, setStatus] = createSignal<SavedRecipesStatus>('loading')
  const [recipes, setRecipes] = createSignal<BrowserSavedRecipe[]>([])
  const [nextAfterId, setNextAfterId] = createSignal<string | null>(null)
  const [error, setError] = createSignal<unknown>(null)
  let inflight: AbortController | undefined

  const abort = () => {
    inflight?.abort()
    inflight = undefined
  }

  const fetchPage = (org: string, afterId: string | undefined, append: boolean) => {
    abort()
    const controller = new AbortController()
    inflight = controller
    setStatus('loading')
    setError(null)
    listBrowserRecipes(org, { limit: RECIPES_LIST_LIMIT, afterId }, controller.signal).then(
      (page) => {
        if (controller.signal.aborted) return
        inflight = undefined
        setRecipes((current) => (append ? [...current, ...page.recipes] : page.recipes))
        setNextAfterId(page.nextAfterId ?? null)
        setStatus('ready')
      },
      (failure: unknown) => {
        if (controller.signal.aborted) return
        inflight = undefined
        setError(failure)
        setStatus('error')
      },
    )
  }

  createEffect(
    () => orgId(),
    (org) => {
      if (org === undefined) return
      if (org === '') {
        // The session context did not answer, so there is no org to ask the list for.
        abort()
        setRecipes([])
        setNextAfterId(null)
        setError(new ApiError('unauthenticated', 401, 'unauthenticated'))
        setStatus('error')
        return
      }
      fetchPage(org, undefined, false)
    },
  )
  onCleanup(abort)

  return {
    status,
    recipes,
    nextAfterId,
    error,
    reload: () => {
      const org = orgId()
      if (org) fetchPage(org, undefined, false)
    },
    loadMore: () => {
      const org = orgId()
      const after = nextAfterId()
      if (org && after && status() !== 'loading') fetchPage(org, after, true)
    },
  }
}

export type RecipeRunStatus = 'idle' | 'starting' | 'live' | 'done' | 'error'

/** What a run of a saved recipe names: the recipe, the version or tag to replay, and its slot values. */
export type RecipeRunInput = {
  id: string
  /** The recipe's name. The run is labelled with it. */
  name: string
  version?: number
  tag?: string
  slots: Record<string, string>
}

/**
 * Starts a browser AI run from a saved recipe (Q4-E4.2-4d). Only the person's click calls
 * `runRecipe`. The start reuses the composer's idempotency keyring, so a retry after an ambiguous
 * failure reaches the same run. The run is `live` while its event stream stays open. The slot
 * values go into the start and are not kept here: the caller clears its own inputs once the start
 * is accepted.
 */
export function useRecipeRun(): {
  status: () => RecipeRunStatus
  error: () => unknown
  runRecipe: (sessionId: string, input: RecipeRunInput) => Promise<boolean>
  reset: () => void
} {
  const [orgId] = createResource(loadEvidenceOrgId)
  const [status, setStatus] = createSignal<RecipeRunStatus>('idle')
  const [error, setError] = createSignal<unknown>(null)
  const keyring = createAiRunStartKeyring()
  let inflight: AbortController | undefined

  const abort = () => {
    inflight?.abort()
    inflight = undefined
  }
  onCleanup(abort)

  const reset = () => {
    abort()
    setStatus('idle')
    setError(null)
  }

  const runRecipe = (sessionId: string, input: RecipeRunInput): Promise<boolean> => {
    const org = orgId()
    const current = status()
    if (!org || !sessionId || current === 'starting' || current === 'live') return Promise.resolve(false)
    const params: StartBrowserAiRunParams = {
      goal: input.name,
      recipe: {
        id: input.id,
        ...(input.version === undefined ? {} : { version: input.version }),
        ...(input.tag === undefined ? {} : { tag: input.tag }),
        slots: input.slots,
      },
    }
    abort()
    const controller = new AbortController()
    inflight = controller
    setStatus('starting')
    setError(null)
    const idempotencyKey = keyring.keyFor(sessionId, params)
    return startBrowserAiRun(org, sessionId, params, controller.signal, { idempotencyKey }).then(
      (run) => {
        keyring.succeeded()
        if (controller.signal.aborted) return false
        setStatus('live')
        void streamRunEvents(
          run.runId,
          {
            onDone: () => {
              if (!controller.signal.aborted) setStatus('done')
            },
            onError: (failure) => {
              if (controller.signal.aborted) return
              setError(failure)
              setStatus('error')
            },
          },
          controller.signal,
        )
        return true
      },
      (failure: unknown) => {
        keyring.failed(failure)
        if (controller.signal.aborted) return false
        inflight = undefined
        setError(failure)
        setStatus('error')
        return false
      },
    )
  }

  return { status, error, runRecipe, reset }
}
