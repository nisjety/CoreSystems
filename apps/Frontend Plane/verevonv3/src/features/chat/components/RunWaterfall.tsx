/**
 * The run waterfall (EP-14 S5): a run's durable timing drawn as bars, after a reload too.
 *
 * Only recorded values are drawn. A bar is one recorded span from its start to its end; spans that
 * overlap (parallel review batches, nested draft checks, concurrent tool calls) go on separate lanes,
 * and no total is ever summed from spans: `turn_timing` says consumers must not, because they overlap.
 * A tool call with no recorded end is shown as open, never given an estimated one. Every chart has a
 * table with the same rows for screen readers and for reading exact values.
 *
 * The ledger mounts the panel per turn behind `VITE_CHAT_RUN_WATERFALL`, and nothing is fetched until
 * the reader asks for it.
 */

import { createSignal, For, Match, Show, Switch } from 'solid-js'
import { getRunTiming, type RunActionTiming, type RunTiming, type RunTimingRecord } from '@/shared/api/run-console-client'
import { useI18n } from '@/shared/i18n'
import { formatLatency } from './chat-media-markdown'

/** Only the literal `true` enables the waterfall, as for the other chat switches. */
export function isRunWaterfallEnabled(raw: string | undefined): boolean {
  return raw === 'true'
}

/** Fixed per build, like every `VITE_` switch: read once, at load. */
export const runWaterfallEnabled = isRunWaterfallEnabled(import.meta.env.VITE_CHAT_RUN_WATERFALL)

export type WaterfallRow = {
  key: string
  label: string
  startMs: number
  /** Absent for a span whose end was never recorded. */
  endMs?: number
  lane: number
}

export type WaterfallGroup = {
  key: string
  kind: 'turn' | 'rounds' | 'actions'
  rows: WaterfallRow[]
  lanes: number
  /** The latest recorded instant in the group, the chart's right edge. */
  extentMs: number
}

function finiteMs(value: unknown): number | undefined {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0 ? value : undefined
}

function record(value: unknown): Record<string, unknown> | undefined {
  return value && typeof value === 'object' && !Array.isArray(value) ? (value as Record<string, unknown>) : undefined
}

/** Put each span on the first lane whose last span has ended, so overlapping spans stack. */
export function assignLanes(rows: readonly Omit<WaterfallRow, 'lane'>[]): WaterfallRow[] {
  const laneEnds: number[] = []
  return [...rows]
    .sort((left, right) => left.startMs - right.startMs)
    .map((row) => {
      const end = row.endMs ?? row.startMs
      let lane = laneEnds.findIndex((laneEnd) => laneEnd <= row.startMs)
      if (lane === -1) {
        lane = laneEnds.length
        laneEnds.push(end)
      } else {
        laneEnds[lane] = end
      }
      return { ...row, lane }
    })
}

function group(key: string, kind: WaterfallGroup['kind'], spans: Omit<WaterfallRow, 'lane'>[]): WaterfallGroup | null {
  if (spans.length === 0) return null
  const rows = assignLanes(spans)
  return {
    key,
    kind,
    rows,
    lanes: rows.reduce((count, row) => Math.max(count, row.lane + 1), 0),
    extentMs: rows.reduce((extent, row) => Math.max(extent, row.endMs ?? row.startMs), 0),
  }
}

function turnSpans(timing: RunTimingRecord): Omit<WaterfallRow, 'lane'>[] {
  const phases = Array.isArray(timing.report.phases) ? timing.report.phases : []
  return phases.flatMap((raw, index) => {
    const phase = record(raw)
    const startMs = finiteMs(phase?.start_ms)
    const endMs = finiteMs(phase?.end_ms)
    const name = typeof phase?.phase === 'string' ? phase.phase : undefined
    if (!phase || name === undefined || startMs === undefined || endMs === undefined || endMs < startMs) return []
    const round = finiteMs(phase.round)
    return [{ key: `phase-${index}`, label: round === undefined ? name : `${name} · ${round}`, startMs, endMs }]
  })
}

function roundSpans(timing: RunTimingRecord): Omit<WaterfallRow, 'lane'>[] {
  const rounds = Array.isArray(timing.report.rounds) ? timing.report.rounds : []
  return rounds.flatMap((raw, index) => {
    const round = record(raw)
    const number = finiteMs(round?.round) ?? index + 1
    const start = finiteMs(round?.inference_start_ms)
    const inferenceEnd = finiteMs(round?.inference_end_ms)
    const dispatchEnd = finiteMs(round?.dispatch_end_ms)
    if (start === undefined || inferenceEnd === undefined || inferenceEnd < start) return []
    const spans: Omit<WaterfallRow, 'lane'>[] = [
      { key: `round-${index}-inference`, label: `inference · ${number}`, startMs: start, endMs: inferenceEnd },
    ]
    if (dispatchEnd !== undefined && dispatchEnd > inferenceEnd) {
      spans.push({ key: `round-${index}-dispatch`, label: `dispatch · ${number}`, startMs: inferenceEnd, endMs: dispatchEnd })
    }
    return spans
  })
}

/** Tool calls on their own clock: milliseconds from the earliest recorded reservation. */
function actionSpans(actions: readonly RunActionTiming[]): Omit<WaterfallRow, 'lane'>[] {
  const reserved = actions
    .map((action) => (action.reservedAt ? Date.parse(action.reservedAt) : Number.NaN))
  const origin = Math.min(...reserved.filter((value) => Number.isFinite(value)))
  if (!Number.isFinite(origin)) return []
  return actions.flatMap((action, index) => {
    const start = reserved[index]
    if (start === undefined || !Number.isFinite(start)) return []
    const end = action.finalizedAt ? Date.parse(action.finalizedAt) : Number.NaN
    return [{
      key: `action-${action.actionId}`,
      label: action.tool,
      startMs: start - origin,
      endMs: Number.isFinite(end) && end >= start ? end - origin : undefined,
    }]
  })
}

/** The groups the waterfall draws: each timing record on its own clock, then the tool calls. */
export function waterfallGroups(timing: RunTiming): WaterfallGroup[] {
  const groups: (WaterfallGroup | null)[] = timing.timings.map((entry, index) => {
    if (entry.schema === 'turn-timing-v1') return group(`turn-${index}`, 'turn', turnSpans(entry))
    if (entry.schema === 'round-timing-v1') return group(`rounds-${index}`, 'rounds', roundSpans(entry))
    return null
  })
  groups.push(group('actions', 'actions', actionSpans(timing.actions)))
  return groups.filter((entry): entry is WaterfallGroup => entry !== null)
}

const CHART_WIDTH = 1000
const LANE_HEIGHT = 14
const LANE_GAP = 4

export function RunWaterfall(props: { timing: RunTiming }) {
  const i18n = useI18n()
  const groups = () => waterfallGroups(props.timing)
  const title = (entry: WaterfallGroup) => {
    switch (entry.kind) {
      case 'turn': return i18n.tr('Faser i turen', 'Turn phases')
      case 'rounds': return i18n.tr('Runder', 'Rounds')
      case 'actions': return i18n.tr('Verktøykall', 'Tool calls')
    }
  }
  const span = (row: WaterfallRow) => (row.endMs === undefined ? undefined : row.endMs - row.startMs)

  return (
    <div class="verevon-chat-waterfall">
      <Show
        when={groups().length > 0}
        fallback={<p class="verevon-chat-ledger__note">{i18n.tr('Ingen tidsdata er registrert for denne kjøringen.', 'No timing was recorded for this run.')}</p>}
      >
        <For each={groups()}>
          {(entry) => (
            <figure class="verevon-chat-waterfall__group">
              <figcaption>{title(entry)}</figcaption>
              <svg
                class="verevon-chat-waterfall__chart"
                viewBox={`0 0 ${CHART_WIDTH} ${entry.lanes * (LANE_HEIGHT + LANE_GAP)}`}
                preserveAspectRatio="none"
                aria-hidden="true"
              >
                <For each={entry.rows}>
                  {(row) => {
                    const scale = (ms: number) => (entry.extentMs > 0 ? (ms / entry.extentMs) * CHART_WIDTH : 0)
                    const x = scale(row.startMs)
                    const width = row.endMs === undefined ? 2 : Math.max(scale(row.endMs) - x, 1)
                    return (
                      <rect
                        data-waterfall-bar={row.key}
                        data-open={row.endMs === undefined ? 'true' : undefined}
                        x={x}
                        y={row.lane * (LANE_HEIGHT + LANE_GAP)}
                        width={width}
                        height={LANE_HEIGHT}
                        rx={2}
                        fill="currentColor"
                      />
                    )
                  }}
                </For>
              </svg>
              <details class="verevon-chat-waterfall__table">
                <summary>{i18n.tr('Vis som tabell', 'Show as a table')}</summary>
                <table>
                  <thead>
                    <tr>
                      <th scope="col">{i18n.tr('Steg', 'Step')}</th>
                      <th scope="col">{i18n.tr('Start', 'Start')}</th>
                      <th scope="col">{i18n.tr('Slutt', 'End')}</th>
                      <th scope="col">{i18n.tr('Varighet', 'Duration')}</th>
                    </tr>
                  </thead>
                  <tbody>
                    <For each={entry.rows}>
                      {(row) => (
                        <tr data-waterfall-row={row.key}>
                          <th scope="row">{row.label}</th>
                          <td>{formatLatency(row.startMs)}</td>
                          <td>{row.endMs === undefined ? i18n.tr('ikke registrert', 'not recorded') : formatLatency(row.endMs)}</td>
                          <td>{span(row) === undefined ? i18n.tr('pågår eller ukjent', 'open or unknown') : formatLatency(span(row) ?? 0)}</td>
                        </tr>
                      )}
                    </For>
                  </tbody>
                </table>
              </details>
            </figure>
          )}
        </For>
        <Show when={props.timing.truncated}>
          <p class="verevon-chat-ledger__note" role="status">
            {i18n.tr('Bare de første 500 verktøykallene vises.', 'Only the first 500 tool calls are shown.')}
          </p>
        </Show>
      </Show>
    </div>
  )
}

type PanelState =
  | { kind: 'idle' }
  | { kind: 'loading' }
  | { kind: 'loaded'; timing: RunTiming }
  | { kind: 'unavailable' }

/**
 * The durable timing of one run, loaded on request. A run the reader cannot see, a run that kept no
 * timing and a read that failed all say the timing is not available; nothing is retried on its own.
 */
export function RunTimingPanel(props: { runId: string; load?: (runId: string) => Promise<RunTiming> }) {
  const i18n = useI18n()
  const [state, setState] = createSignal<PanelState>({ kind: 'idle' })
  const load = async () => {
    setState({ kind: 'loading' })
    try {
      const timing = await (props.load ?? getRunTiming)(props.runId)
      setState({ kind: 'loaded', timing })
    } catch {
      setState({ kind: 'unavailable' })
    }
  }

  return (
    <div class="verevon-chat-waterfall__panel">
      <Switch>
        <Match when={state().kind === 'idle'}>
          <button type="button" class="verevon-chat-ledger__row" onClick={() => void load()}>
            {i18n.tr('Vis tidslinje', 'Show timing')}
          </button>
        </Match>
        <Match when={state().kind === 'loading'}>
          <p class="verevon-chat-ledger__note" role="status">{i18n.tr('Henter tidslinjen …', 'Loading the timing…')}</p>
        </Match>
        <Match when={state().kind === 'unavailable'}>
          <p class="verevon-chat-ledger__note" role="status">
            {i18n.tr('Tidslinjen er ikke tilgjengelig for denne kjøringen.', 'Timing is not available for this run.')}
          </p>
        </Match>
        <Match when={(() => { const current = state(); return current.kind === 'loaded' ? current.timing : undefined })()}>
          {(timing) => <RunWaterfall timing={timing()} />}
        </Match>
      </Switch>
    </div>
  )
}
