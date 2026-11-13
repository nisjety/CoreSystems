import { createEffect, createMemo, createSignal, For, Show } from 'solid-js'
import { ApiError } from '@/shared/api/http'
import { cancelRun, listChildRuns, type LineageEdgeSummary } from '@/shared/api/orchestration-client'
import { getRun } from '@/shared/api/runs-client'
import { useI18n } from '@/shared/i18n'
import { describeApiError, ERROR_COPY } from '@/shared/i18n/errors'
import { Grid2X2, Square, XCircle } from '@/shared/icons'
import { Button } from '@/shared/ui/Button'

// W5 — FleetRunConsole. The child runs of one parent run, read back from its thread's lineage (EP-2):
// each child's goal, its edge status and its own run's status. A child, or the whole fleet (the parent
// run), is stopped through the existing cancel route. The statuses are read again every few seconds
// while the parent or a child can still change, and not after; the timer stops on unmount. A fleet has
// no budget read yet, so none is shown. Dark behind VITE_FLEET_CONSOLE=1 (App.tsx).

/** How often the parent run and its children are read again while one of them can still change. */
const POLL_INTERVAL_MS = 3000

/**
 * Run statuses under which a run cannot do more work. The wide-research panel reads the same set
 * (it keeps its own copy, module-local there).
 */
const TERMINAL_RUN_STATUSES = new Set([
  'completed',
  'failed',
  'cancelled',
  'canceled',
  'stopped',
  'rejected',
  'expired',
])

type I18n = ReturnType<typeof useI18n>

/** Which not-found copy a 404 gets: the run itself, or the conversation its lineage belongs to. */
type ReadScope = 'run' | 'thread'

/** A read that failed: the typed message to show, and whether asking again can change it (a 403 or a 404). */
type ReadFailure = { message: string; permanent: boolean }

/** A run as it was last read. `status` is absent until a read answers; a failed read never invents one. */
type RunRead = { status?: string; failure?: ReadFailure }

/** The child list as it was read: its edges, or the typed reason it could not be read. */
type ChildList = { edges: LineageEdgeSummary[] } | { failure: ReadFailure }

function isTerminalRunStatus(status: string | undefined): boolean {
  return status !== undefined && TERMINAL_RUN_STATUSES.has(status.trim().toLowerCase())
}

/** Whether a read can no longer change: its run has finished, or it cannot be read for good. */
function isSettled(read: RunRead | undefined): boolean {
  return read?.failure?.permanent === true || isTerminalRunStatus(read?.status)
}

/** The typed message for a failed read or stop. A 403 keeps the shared forbidden copy; a 404 names what was not found. */
function failureMessage(i18n: I18n, error: unknown, scope: ReadScope): string {
  const status = error instanceof ApiError ? error.status : null
  const notFound = status !== 404 ? undefined : scope === 'thread' ? ERROR_COPY.thread_not_found : ERROR_COPY.run_not_found
  return describeApiError(error, i18n.tr, notFound).message
}

function readFailureOf(i18n: I18n, error: unknown, scope: ReadScope): ReadFailure {
  const status = error instanceof ApiError ? error.status : null
  return { message: failureMessage(i18n, error, scope), permanent: status === 403 || status === 404 }
}

async function readRun(i18n: I18n, runId: string, signal: AbortSignal): Promise<RunRead> {
  try {
    const run = await getRun(runId, signal)
    // A reply with no run is not a status to show: this person cannot see such a run.
    if (!run) {
      return {
        failure: {
          message: i18n.tr(
            'Fant ikke kjøringen, eller du har ikke tilgang til den.',
            'The run was not found, or you do not have access to it.',
          ),
          permanent: true,
        },
      }
    }
    return { status: run.status }
  } catch (error) {
    return { failure: readFailureOf(i18n, error, 'run') }
  }
}

async function readChildList(
  i18n: I18n,
  threadId: string,
  parentRunId: string,
  signal: AbortSignal,
): Promise<ChildList> {
  try {
    return { edges: await listChildRuns(threadId, parentRunId, signal) }
  } catch (error) {
    return { failure: readFailureOf(i18n, error, 'thread') }
  }
}

/** The label for a run's status token. A token this view does not name is shown as the server sent it. */
function runStatusLabel(i18n: I18n, status: string): string {
  switch (status.trim().toLowerCase()) {
    case 'queued':
      return i18n.tr('I kø', 'Queued')
    case 'running':
      return i18n.tr('Kjører', 'Running')
    case 'awaiting_approval':
      return i18n.tr('Venter på godkjenning', 'Awaiting approval')
    case 'completed':
      return i18n.tr('Ferdig', 'Done')
    case 'failed':
      return i18n.tr('Feilet', 'Failed')
    case 'cancelled':
    case 'canceled':
    case 'stopped':
      return i18n.tr('Stoppet', 'Stopped')
    case 'rejected':
      return i18n.tr('Avvist', 'Rejected')
    case 'expired':
      return i18n.tr('Utløpt', 'Expired')
    default:
      return status
  }
}

/** The label for a child run's status in its row: its own status once read, a note while it is not. */
function childRunLabel(i18n: I18n, read: RunRead | undefined): string {
  if (read?.status !== undefined) return runStatusLabel(i18n, read.status)
  return read?.failure ? i18n.tr('Kan ikke leses', 'Cannot be read') : i18n.tr('Leses …', 'Reading …')
}

/** The label for a lineage edge's own status (`attached` or `detached`), which is not the child run's status. */
function edgeStatusLabel(i18n: I18n, status: string): string {
  if (status === 'attached') return i18n.tr('Koblet', 'Attached')
  if (status === 'detached') return i18n.tr('Frakoblet', 'Detached')
  return status === '' ? i18n.tr('Ukjent', 'Unknown') : status
}

function childCountLabel(i18n: I18n, count: number): string {
  if (count === 1) return i18n.tr('1 underkjøring', '1 child run')
  return i18n.tr(`${count} underkjøringer`, `${count} child runs`)
}

/** Whether two child lists hold the same rows, so the grid keeps its rows (and their buttons) between reads. */
function sameEdges(a: LineageEdgeSummary[], b: LineageEdgeSummary[]): boolean {
  return JSON.stringify(a) === JSON.stringify(b)
}

function withoutKey<T>(record: Record<string, T>, key: string): Record<string, T> {
  const next = { ...record }
  delete next[key]
  return next
}

export function FleetRunConsole(props: { threadId: string; runId: string }) {
  const i18n = useI18n()
  const [parent, setParent] = createSignal<RunRead>({})
  const [edges, setEdges] = createSignal<LineageEdgeSummary[]>([])
  const [listAnswered, setListAnswered] = createSignal(false)
  const [listFailure, setListFailure] = createSignal<ReadFailure | null>(null)
  const [childReads, setChildReads] = createSignal<Record<string, RunRead>>({})
  const [stopping, setStopping] = createSignal<ReadonlySet<string>>(new Set<string>())
  const [stopFailures, setStopFailures] = createSignal<Record<string, string>>({})

  // Reads are numbered as they start. An answer that arrives after a newer read has been shown is
  // dropped, so a slow poll never puts back a status the view has already moved past.
  let readsStarted = 0
  let readsShown = 0
  // Aborted when the view moves to another run or unmounts, so its reads stop with it.
  let session: AbortController | undefined

  /** Read the run, its children and each child's status, then show them together. */
  const refresh = async (signal: AbortSignal): Promise<void> => {
    const seq = ++readsStarted
    const threadId = props.threadId
    const runId = props.runId
    const [parentRead, list] = await Promise.all([
      readRun(i18n, runId, signal),
      readChildList(i18n, threadId, runId, signal),
    ])
    const found = 'edges' in list ? list.edges : []
    const childReadsNow = await Promise.all(
      found.map(async (edge) => [edge.childRunId, await readRun(i18n, edge.childRunId, signal)] as const),
    )
    if (signal.aborted || seq < readsShown) return
    readsShown = seq
    setParent(parentRead)
    setListAnswered(true)
    if ('edges' in list) {
      setListFailure(null)
      setEdges((current) => (sameEdges(current, list.edges) ? current : list.edges))
      setChildReads(Object.fromEntries(childReadsNow))
      return
    }
    // A child list that cannot be read keeps the rows it last showed; one refused for good shows none.
    setListFailure(list.failure)
    if (list.failure.permanent) {
      setEdges([])
      setChildReads({})
    }
  }

  const childRead = (runId: string): RunRead | undefined => childReads()[runId]

  /** Whether anything is left to read: the run, or a child, can still change. */
  const active = createMemo(() => {
    const failure = listFailure()
    if (failure?.permanent === true) return false
    // A child list that failed for now is read again, as is a parent run that has not settled.
    if (failure || !isSettled(parent())) return true
    return edges().some((edge) => !isSettled(childRead(edge.childRunId)))
  })

  // Read again every few seconds while something can still change. The timer stops as soon as nothing
  // can, and on unmount.
  createEffect(
    () => active(),
    (live) => {
      if (!live) return
      const timer = window.setInterval(() => {
        const current = session
        if (current && !current.signal.aborted) void refresh(current.signal)
      }, POLL_INTERVAL_MS)
      return () => window.clearInterval(timer)
    },
  )

  // A run this view moves to starts from nothing: no rows, no statuses and no earlier answer.
  createEffect(
    () => [props.threadId, props.runId] as const,
    () => {
      const next = new AbortController()
      session = next
      setParent({})
      setEdges([])
      setListAnswered(false)
      setListFailure(null)
      setChildReads({})
      setStopping(new Set<string>())
      setStopFailures({})
      void refresh(next.signal)
      return () => {
        next.abort()
        if (session === next) session = undefined
      }
    },
  )

  /** Stop one run (a child, or the parent for the whole fleet), then read the statuses again. */
  const stop = async (runId: string): Promise<void> => {
    if (stopping().has(runId)) return
    setStopping((current) => new Set([...current, runId]))
    setStopFailures((current) => withoutKey(current, runId))
    try {
      await cancelRun(runId)
    } catch (error) {
      setStopFailures((current) => ({ ...current, [runId]: failureMessage(i18n, error, 'run') }))
    } finally {
      setStopping((current) => new Set([...current].filter((id) => id !== runId)))
    }
    const latest = session
    if (latest && !latest.signal.aborted) await refresh(latest.signal)
  }

  /** The fleet can be stopped while the run it belongs to can still change. */
  const fleetStoppable = (): boolean => {
    const read = parent()
    if (read.failure) return !read.failure.permanent
    return read.status !== undefined && !isTerminalRunStatus(read.status)
  }

  /** A child can be stopped only while its own run is known to be unfinished. */
  const childStoppable = (runId: string): boolean => {
    const read = childRead(runId)
    return read?.status !== undefined && !isTerminalRunStatus(read.status)
  }

  return (
    <div class="flex flex-col gap-4 p-4" data-testid="fleet-run-console">
      <div class="flex flex-wrap items-center gap-3">
        <Grid2X2 class="h-5 w-5" />
        <span class="font-medium">
          {i18n.tr('Flåte', 'Fleet')} {props.runId.slice(0, 12)}
        </span>
        <Show when={parent().status !== undefined}>
          <span class="text-sm text-muted-foreground">{runStatusLabel(i18n, parent().status ?? '')}</span>
        </Show>
        <Show when={listAnswered() && listFailure() === null}>
          <span class="text-sm text-muted-foreground">{childCountLabel(i18n, edges().length)}</span>
        </Show>
        <Show when={fleetStoppable()}>
          <div class="ml-auto">
            <Button
              variant="outline"
              size="sm"
              disabled={stopping().has(props.runId)}
              onClick={() => void stop(props.runId)}
            >
              <XCircle class="mr-2 h-4 w-4" />
              {i18n.tr('Stopp alle', 'Stop all')}
            </Button>
          </div>
        </Show>
      </div>

      <Show when={parent().failure}>
        {(failure) => <p role="alert" class="text-sm text-destructive">{failure().message}</p>}
      </Show>
      <Show when={listFailure()}>
        {(failure) => <p role="alert" class="text-sm text-destructive">{failure().message}</p>}
      </Show>
      <Show when={stopFailures()[props.runId]}>
        {(message) => <p role="alert" class="text-sm text-destructive">{message()}</p>}
      </Show>

      <Show
        when={listAnswered()}
        fallback={<p class="text-sm text-muted-foreground">{i18n.tr('Laster …', 'Loading …')}</p>}
      >
        <Show when={edges().length === 0 && listFailure() === null}>
          <p class="text-sm text-muted-foreground">{i18n.tr('Ingen underkjøringer ennå.', 'No child runs yet.')}</p>
        </Show>
        <div class="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-3">
          <For each={edges()}>
            {(edge) => (
              <article class="flex flex-col gap-2 rounded-lg border p-3" data-run-id={edge.childRunId}>
                <p class="text-sm font-medium" title={edge.goalSummary}>
                  {edge.goalSummary || i18n.tr('(uten oppgitt mål)', '(no stated goal)')}
                </p>
                <div class="flex flex-wrap gap-x-3 gap-y-1 text-xs text-muted-foreground">
                  <span>
                    {i18n.tr('Kobling', 'Link')}: {edgeStatusLabel(i18n, edge.status)}
                  </span>
                  <span>
                    {i18n.tr('Kjøring', 'Run')}: {childRunLabel(i18n, childRead(edge.childRunId))}
                  </span>
                  <span class="font-mono">{edge.childRunId.slice(0, 12)}</span>
                </div>
                <Show when={childRead(edge.childRunId)?.failure}>
                  {(failure) => <p role="alert" class="text-xs text-destructive">{failure().message}</p>}
                </Show>
                <Show when={childStoppable(edge.childRunId)}>
                  <div>
                    <Button
                      variant="outline"
                      size="sm"
                      disabled={stopping().has(edge.childRunId)}
                      onClick={() => void stop(edge.childRunId)}
                    >
                      <Square class="mr-2 h-3 w-3" />
                      {stopping().has(edge.childRunId)
                        ? i18n.tr('Stopper …', 'Stopping …')
                        : i18n.tr('Stopp', 'Stop')}
                    </Button>
                  </div>
                </Show>
                <Show when={stopFailures()[edge.childRunId]}>
                  {(message) => <p role="alert" class="text-xs text-destructive">{message()}</p>}
                </Show>
              </article>
            )}
          </For>
        </div>
      </Show>
    </div>
  )
}

export default FleetRunConsole
