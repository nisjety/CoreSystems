import { Show, Switch, Match, createMemo, createSignal, onCleanup, untrack } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { AlertCircle, BarChart3, Check, Copy, Download } from '@/shared/icons'
import { useI18n } from '@/shared/i18n'
import { downloadBytes } from '../lib/chat-export/download'
import { chartFromTable, type ChartSpec } from '../lib/chat-chart/spec'
import { csvBytes, tableToCsv, tableToTsv } from '../lib/chat-chart/table-text'
import { ChatChart } from './ChatChart'

/**
 * The small toolbar of a markdown table (copy, download CSV, show as chart),
 * and the chart it opens under the table.
 *
 * The toolbar is out of the way: it appears on hover and on keyboard focus
 * only, floating over the table's corner, so a table keeps the density it has.
 * "Vis som diagram" is offered only when `chartFromTable` can make an honest
 * chart of the table.
 *
 * A chart the reader opened stays open while the message keeps streaming:
 * `ChatMarkdown` remounts every block on each token, so the open state lives
 * in a module-level set keyed by the table's own text.
 */

const COPY_CONFIRMATION_MS = 1800
const openCharts = new Set<string>()
const MAX_REMEMBERED = 200

function tableKey(header: readonly string[], rows: readonly (readonly string[])[]): string {
  return JSON.stringify([header, rows])
}

function rememberOpen(key: string, open: boolean): void {
  openCharts.delete(key)
  if (open) {
    openCharts.add(key)
    if (openCharts.size > MAX_REMEMBERED) {
      const oldest = openCharts.values().next().value
      if (oldest !== undefined) openCharts.delete(oldest)
    }
  }
}

export type TableToolsState = {
  /** The chart of the table, or null when the table has no honest chart. */
  chart: () => ChartSpec | null
  open: () => boolean
  toggle: () => void
  key: () => string
}

/** Shared by the toolbar button and the chart mounted under the table. */
export function createTableChartState(
  header: () => readonly string[],
  rows: () => readonly (readonly string[])[],
): TableToolsState {
  const key = createMemo(() => tableKey(header(), rows()))
  const chart = createMemo(() => chartFromTable(header(), rows()))
  const [open, setOpen] = createSignal(openCharts.has(untrack(key)))
  return {
    chart,
    open: () => open() && chart() !== null,
    key,
    toggle: () => {
      const next = !open()
      setOpen(next)
      rememberOpen(key(), next)
    },
  }
}

type Outcome = 'idle' | 'copied' | 'failed' | 'saved'

export function ChatTableTools(props: {
  header: readonly string[]
  rows: readonly (readonly string[])[]
  state: TableToolsState
}): JSX.Element {
  const i18n = useI18n()
  const [outcome, setOutcome] = createSignal<Outcome>('idle')
  let timer: number | undefined
  onCleanup(() => window.clearTimeout(timer))

  const settle = (next: Outcome) => {
    setOutcome(next)
    window.clearTimeout(timer)
    timer = window.setTimeout(() => setOutcome('idle'), COPY_CONFIRMATION_MS)
  }

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(tableToTsv(props.header, props.rows))
      settle('copied')
    } catch {
      // Clipboard access can be denied (no permission, insecure context): say so
      // instead of flashing a success for something that did not happen.
      settle('failed')
    }
  }

  const download = () => {
    const locale = i18n.locale()
    const bytes = csvBytes(tableToCsv(props.header, props.rows, locale))
    downloadBytes(bytes, locale === 'en' ? 'table.csv' : 'tabell.csv', 'text/csv;charset=utf-8')
    settle('saved')
  }

  const copyLabel = () => {
    if (outcome() === 'copied') return i18n.tr('Kopiert', 'Copied')
    if (outcome() === 'failed') return i18n.tr('Kunne ikke kopiere', 'Could not copy')
    return i18n.tr('Kopier', 'Copy')
  }

  return (
    <div class="verevon-chat-table-tools">
      <div class="verevon-chat-table-tools__bar" role="toolbar" aria-label={i18n.tr('Tabellverktøy', 'Table tools')}>
        <button
          type="button"
          class={{ 'verevon-chat-table-tools__button': true, 'verevon-chat-table-tools__button--failed': outcome() === 'failed' }}
          onClick={() => void copy()}
        >
          <Switch fallback={<Copy size={13} />}>
            <Match when={outcome() === 'copied'}><Check size={13} /></Match>
            <Match when={outcome() === 'failed'}><AlertCircle size={13} /></Match>
          </Switch>
          <span>{copyLabel()}</span>
        </button>
        <button type="button" class="verevon-chat-table-tools__button" onClick={download}>
          <Show when={outcome() === 'saved'} fallback={<Download size={13} />}>
            <Check size={13} />
          </Show>
          <span>{i18n.tr('Last ned CSV', 'Download CSV')}</span>
        </button>
        <Show when={props.state.chart()}>
          <button
            type="button"
            class="verevon-chat-table-tools__button"
            aria-pressed={props.state.open() ? 'true' : 'false'}
            onClick={() => props.state.toggle()}
          >
            <BarChart3 size={13} />
            <span>{props.state.open() ? i18n.tr('Skjul diagram', 'Hide chart') : i18n.tr('Vis som diagram', 'Show as chart')}</span>
          </button>
        </Show>
      </div>
    </div>
  )
}

/** The chart a table was switched to, mounted under the table. */
export function TableChart(props: { state: TableToolsState }): JSX.Element {
  return (
    <Show when={props.state.open() ? props.state.chart() : null}>
      {(spec) => <ChatChart spec={spec()} stateKey={`table:${props.state.key()}`} />}
    </Show>
  )
}
