/**
 * The run ledger view (EP-14 S4): the Trace as turns, each holding its records in recorded-time order.
 * A tool call is one row, paired from its call and its result. A row opens in the inspector beside the
 * list.
 *
 * Nothing here takes focus. The arrow keys move focus between rows the reader is already on, and an
 * Inspect action from a tool card selects its call without moving focus. The page mounts this only
 * behind `VITE_CHAT_RUN_LEDGER` (see `run-ledger.ts`). With `VITE_CHAT_RUN_WATERFALL` on, a turn of a
 * known run also offers its durable timing (`RunWaterfall.tsx`).
 *
 * A turn whose `turn_timing` carries rounds shows a sub-header per round, with the records of that round
 * under it. Past WINDOW_ROW_THRESHOLD rows the list is windowed with `planTranscriptWindow`: every row
 * is a fixed height, only the rows near the viewport are mounted, and the arrow keys still reach every
 * record. Under the threshold the list is mounted whole, as before. In the windowed list the durable
 * timing is offered for the turn of the selected row, below the list, because a panel of its own height
 * cannot be one of the fixed-height rows.
 */

import { createEffect, createMemo, createSignal, flush, For, Show, untrack } from 'solid-js'
import type { ChatTurnTiming } from '@/shared/api/chat-client'
import { uiEventLabel, type VerevonUiEvent } from '@/shared/chat/verevon-ui-events'
import { useI18n } from '@/shared/i18n'
import { formatLatency } from './chat-media-markdown'
import { LedgerInspector } from './LedgerInspector'
import { RunTimingPanel, runWaterfallEnabled } from './RunWaterfall'
import {
  buildLedger,
  flattenLedger,
  formatClock,
  recordAt,
  spanMs,
  turnBodyRows,
  turnKey,
  type LedgerOutcome,
  type LedgerRecord,
  type LedgerRound,
  type LedgerRow,
  type LedgerTurn,
} from '../lib/run-ledger'
import { planTranscriptWindow } from '../lib/transcript-window'

/** Above this many rows the ledger mounts a window of them instead of all of them. */
export const WINDOW_ROW_THRESHOLD = 200
/** Every row of the windowed list is this tall (see `.verevon-chat-ledger__window` in global.css). */
export const LEDGER_ROW_HEIGHT_PX = 30
/** The windowed list's height, which its stylesheet rule also sets. Used until the list is measured. */
export const LEDGER_VIEWPORT_PX = 480

type I18n = ReturnType<typeof useI18n>

function toggled(keys: ReadonlySet<string>, key: string): ReadonlySet<string> {
  const next = new Set(keys)
  if (next.has(key)) next.delete(key)
  else next.add(key)
  return next
}

function opened(keys: ReadonlySet<string>, key: string): ReadonlySet<string> {
  if (!keys.has(key)) return keys
  const next = new Set(keys)
  next.delete(key)
  return next
}

function recordTitle(record: LedgerRecord, i18n: I18n): string {
  if (record.kind === 'call') return record.name ?? record.id ?? i18n.tr('Verktøykall', 'Tool call')
  if (record.kind === 'unmatched') return i18n.tr('Verktøyresultat uten kall', 'Tool result with no call')
  return uiEventLabel(record.event)
}

/** Only a tool call has a status of its own; a turn's outcome is shown on the turn. */
function callStatus(record: LedgerRecord, i18n: I18n): string | undefined {
  if (record.kind !== 'call') return undefined
  switch (record.status) {
    case 'running': return i18n.tr('kjører', 'running')
    case 'completed': return i18n.tr('fullført', 'completed')
    case 'failed': return i18n.tr('feilet', 'failed')
  }
}

function outcomeText(outcome: LedgerOutcome, i18n: I18n): string {
  switch (outcome) {
    case 'completed': return i18n.tr('fullført', 'completed')
    case 'failed': return i18n.tr('feilet', 'failed')
    case 'stopped': return i18n.tr('stoppet', 'stopped')
    case 'open': return i18n.tr('ingen sluttstatus registrert', 'no end recorded')
  }
}

function findSelected(turns: readonly LedgerTurn[], key: string | null): { turn: LedgerTurn; record: LedgerRecord } | null {
  if (key === null) return null
  for (const turn of turns) {
    const record = turn.records.find((candidate) => candidate.key === key)
    if (record !== undefined) return { turn, record }
  }
  return null
}

function roundTitle(round: LedgerRound | null, i18n: I18n): string {
  if (round === null) return i18n.tr('Utenfor en runde', 'Outside any round')
  const span = formatLatency(round.endMs - round.startMs)
  return i18n.tr(`Runde ${round.round} · ${span}`, `Round ${round.round} · ${span}`)
}

/** The flat-list index a focus or key event came from, or -1 when it did not come from a windowed row. */
function rowIndexOf(target: EventTarget | null): number {
  const element = target instanceof Element ? target.closest('[data-ledger-index]') : null
  const raw = element?.getAttribute('data-ledger-index')
  return raw === null || raw === undefined ? -1 : Number(raw)
}

function callDuration(record: LedgerRecord): string | undefined {
  return record.kind === 'call' && record.durationMs !== undefined ? formatLatency(record.durationMs) : undefined
}

export function RunLedger(props: {
  events: readonly VerevonUiEvent[]
  replayTruncated?: boolean
  /** A tool call to select, from the Inspect action on its card. */
  inspectRequest?: { callId: string } | null
  /** Each turn's `turn_timing` by turn id. With it, a turn that has rounds shows them. */
  timings?: ReadonlyMap<string, ChatTurnTiming>
}) {
  const i18n = useI18n()
  let root: HTMLDivElement | undefined
  let windowEl: HTMLDivElement | undefined
  const turns = createMemo(() => buildLedger(props.events, props.timings))
  // Attributed turns are numbered in the order they happened. The unattributed group is not a turn.
  const view = createMemo(() => {
    let number = 0
    return turns().map((turn) => {
      if (turn.turnId !== null) number += 1
      return { turn, key: turnKey(turn), number: turn.turnId === null ? 0 : number }
    })
  })
  const turnCount = createMemo(() => view().filter((entry) => entry.number > 0).length)
  const callCount = createMemo(() => turns().reduce(
    (count, turn) => count + turn.records.filter((record) => record.kind === 'call').length,
    0,
  ))
  const [collapsed, setCollapsed] = createSignal<ReadonlySet<string>>(new Set())
  const [selectedKey, setSelectedKey] = createSignal<string | null>(null)
  const [callMissing, setCallMissing] = createSignal(false)
  const [scrollTop, setScrollTop] = createSignal(0)
  const [focusedIndex, setFocusedIndex] = createSignal(-1)
  const [revealKey, setRevealKey] = createSignal<string | null>(null)

  const turnNumbers = createMemo(() => new Map(view().map((entry) => [entry.key, entry.number] as const)))
  const flat = createMemo(() => flattenLedger(turns(), collapsed()))
  const windowed = createMemo(() => flat().length > WINDOW_ROW_THRESHOLD)
  const flatIndex = createMemo(() => new Map(flat().map((row, index) => [row, index] as const)))
  const plan = createMemo(() => {
    if (!windowed()) return undefined
    const rows = flat()
    return planTranscriptWindow({
      heights: rows.map(() => LEDGER_ROW_HEIGHT_PX),
      rowGap: 0,
      scrollTop: scrollTop(),
      viewportHeight: LEDGER_VIEWPORT_PX,
      focusedIndex: focusedIndex() >= 0 ? focusedIndex() : undefined,
    })
  })
  // Row objects are stable until the ledger changes, so scrolling moves rows in and out without
  // remounting the ones that stay, and a focused row keeps its focus.
  const mountedRows = createMemo(() => {
    const rows = flat()
    return (plan()?.items ?? []).flatMap((item) => {
      const row = rows[item.index]
      return row === undefined ? [] : [row]
    })
  })
  const gapBefore = createMemo(() => {
    const rows = flat()
    return new Map((plan()?.items ?? []).flatMap((item) => {
      const row = rows[item.index]
      return row === undefined ? [] : [[row, item.gapBefore] as const]
    }))
  })

  const selected = createMemo(() => findSelected(turns(), selectedKey()))

  // Only a new request selects. The ledger re-derives on every event, and a selection the reader
  // made since must survive that, so the lookup reads the turns untracked.
  createEffect(
    () => props.inspectRequest ?? null,
    (request) => {
      if (request === null) return
      for (const turn of untrack(turns)) {
        const record = turn.records.find((candidate) => candidate.kind === 'call' && candidate.id === request.callId)
        if (record === undefined) continue
        setCollapsed((keys) => opened(keys, turnKey(turn)))
        setSelectedKey(record.key)
        setRevealKey(record.key)
        setCallMissing(false)
        return
      }
      setSelectedKey(null)
      setCallMissing(true)
    },
  )

  const scrollToIndex = (index: number) => {
    const height = windowEl?.clientHeight || LEDGER_VIEWPORT_PX
    const top = index * LEDGER_ROW_HEIGHT_PX
    let next = untrack(scrollTop)
    if (top < next) next = top
    else if (top + LEDGER_ROW_HEIGHT_PX > next + height) next = top + LEDGER_ROW_HEIGHT_PX - height
    if (windowEl !== undefined) windowEl.scrollTop = next
    setScrollTop(next)
  }

  // A call asked for by Inspect may be outside the window: bring its row into it.
  createEffect(
    () => revealKey(),
    (key) => {
      if (key === null) return
      setRevealKey(null)
      if (!untrack(windowed)) return
      const index = untrack(flat).findIndex((row) => row.kind === 'record' && row.key === key)
      if (index >= 0) scrollToIndex(index)
    },
  )

  const onWindowScroll = () => {
    if (windowEl !== undefined) setScrollTop(windowEl.scrollTop)
  }

  const onFocusIn = (event: FocusEvent) => {
    const index = rowIndexOf(event.target)
    if (index !== -1) setFocusedIndex(index)
  }

  /** The arrow keys of the windowed list walk the record rows of the whole list, mounted or not. */
  const onWindowedKeyDown = (event: KeyboardEvent) => {
    const rows = flat()
    const current = rowIndexOf(event.target)
    if (current === -1 || rows[current]?.kind !== 'record') return
    const records: number[] = []
    rows.forEach((row, index) => { if (row.kind === 'record') records.push(index) })
    const position = records.indexOf(current)
    let destination: number | undefined
    switch (event.key) {
      case 'ArrowDown': destination = records[Math.min(position + 1, records.length - 1)]; break
      case 'ArrowUp': destination = records[Math.max(position - 1, 0)]; break
      case 'Home': destination = records[0]; break
      case 'End': destination = records[records.length - 1]; break
      default: return
    }
    event.preventDefault()
    if (destination === undefined) return
    scrollToIndex(destination)
    setFocusedIndex(destination)
    flush()
    root?.querySelector<HTMLElement>(`[data-ledger-index="${destination}"]`)?.focus()
  }

  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key === 'Escape' && selectedKey() !== null) {
      event.preventDefault()
      setSelectedKey(null)
      return
    }
    if (windowed()) {
      onWindowedKeyDown(event)
      return
    }
    const rows = Array.from(root?.querySelectorAll<HTMLButtonElement>('[data-ledger-row]') ?? [])
    const current = rows.indexOf(event.target as HTMLButtonElement)
    if (current === -1) return
    const last = rows.length - 1
    let destination: number
    switch (event.key) {
      case 'ArrowDown': destination = Math.min(current + 1, last); break
      case 'ArrowUp': destination = Math.max(current - 1, 0); break
      case 'Home': destination = 0; break
      case 'End': destination = last; break
      default: return
    }
    event.preventDefault()
    rows[destination]?.focus()
  }

  const recordButton = (row: Extract<LedgerRow, { kind: 'record' }>, index?: number) => (
    <button
      type="button"
      class="verevon-chat-ledger__row"
      data-ledger-row
      data-ledger-index={index}
      aria-current={selectedKey() === row.record.key ? 'true' : undefined}
      onClick={() => setSelectedKey(row.record.key)}
    >
      <span class="verevon-chat-ledger__row-title">{recordTitle(row.record, i18n)}</span>
      <Show when={callStatus(row.record, i18n)}>{(text) => <em>{text()}</em>}</Show>
      <Show when={callDuration(row.record)}>{(text) => <em>{text()}</em>}</Show>
      <time>{formatClock(recordAt(row.record), i18n.locale())}</time>
    </button>
  )

  const turnHeadContent = (turn: LedgerTurn, number: number) => (
    <>
      <strong>
        {number === 0
          ? i18n.tr('Ikke knyttet til en tur', 'Not tied to a turn')
          : i18n.tr(`Tur ${number}`, `Turn ${number}`)}
      </strong>
      <Show when={turn.outcome}>
        {(outcome) => <em>{outcomeText(outcome(), i18n)}</em>}
      </Show>
      <Show when={spanMs(turn.startAt, turn.endAt)}>
        {(span) => <span>{formatLatency(span())}</span>}
      </Show>
      <span>{i18n.tr(`${turn.records.length} poster`, `${turn.records.length} records`)}</span>
      <time>{formatClock(turn.startAt, i18n.locale())}</time>
    </>
  )

  const roundHeader = (row: Extract<LedgerRow, { kind: 'round' }>) => (
    <div class="verevon-chat-ledger__round">
      <span>{roundTitle(row.round, i18n)}</span>
      <span>{i18n.tr(`${row.recordCount} poster`, `${row.recordCount} records`)}</span>
    </div>
  )

  const windowedRow = (row: LedgerRow) => {
    const index = flatIndex().get(row)
    const style = () => {
      const gap = gapBefore().get(row) ?? 0
      return gap > 0 ? { 'margin-top': `${gap}px` } : undefined
    }
    switch (row.kind) {
      case 'turn':
        return (
          <button
            type="button"
            class="verevon-chat-ledger__turn-head"
            style={style()}
            aria-expanded={collapsed().has(row.turnKey) ? 'false' : 'true'}
            onClick={() => setCollapsed((keys) => toggled(keys, row.turnKey))}
          >
            {turnHeadContent(row.turn, turnNumbers().get(row.turnKey) ?? 0)}
          </button>
        )
      case 'round':
        return <div style={style()}>{roundHeader(row)}</div>
      case 'record':
        return <div style={style()}>{recordButton(row, index)}</div>
    }
  }

  return (
    <div class="verevon-chat-ledger" ref={(element) => { root = element }} onKeyDown={onKeyDown} onFocusIn={onFocusIn}>
      <p class="verevon-chat-ledger__summary">
        {i18n.tr(`${turnCount()} turer · ${callCount()} verktøykall`, `${turnCount()} turns · ${callCount()} tool calls`)}
      </p>
      <Show when={props.replayTruncated}>
        <p class="verevon-chat-trace-log__notice" role="status">
          {i18n.tr(
            'Visningen er avkortet ved 5 000 hendelser — de eldste vises ikke her. Den varige loggen er fortsatt komplett.',
            'The view is cut at 5,000 events, so the oldest are not shown here. The durable log is still complete.',
          )}
        </p>
      </Show>
      <Show when={callMissing()}>
        <p class="verevon-chat-ledger__note" role="status">
          {i18n.tr('Kallet er ikke i denne visningen.', 'That call is not in this view.')}
        </p>
      </Show>
      <Show
        when={view().length > 0}
        fallback={<p class="verevon-chat-trace-log__empty">{i18n.tr('Ingen hendelser er registrert ennå.', 'No events have been recorded yet.')}</p>}
      >
        <div class="verevon-chat-ledger__layout">
          <Show
            when={windowed()}
            fallback={
              <div class="verevon-chat-ledger__turns">
                <For each={view()}>
                  {(entry) => (
                    <section class="verevon-chat-ledger__turn">
                      <button
                        type="button"
                        class="verevon-chat-ledger__turn-head"
                        aria-expanded={collapsed().has(entry.key) ? 'false' : 'true'}
                        onClick={() => setCollapsed((keys) => toggled(keys, entry.key))}
                      >
                        {turnHeadContent(entry.turn, entry.number)}
                      </button>
                      <Show when={!collapsed().has(entry.key)}>
                        <div class="verevon-chat-ledger__records">
                          {/* EP-14 S5: the run's durable timing, which survives a reload; fetched only on request. */}
                          <Show when={runWaterfallEnabled ? entry.turn.runId : undefined}>
                            {(runId) => <RunTimingPanel runId={runId()} />}
                          </Show>
                          <For each={turnBodyRows(entry.turn)}>
                            {(row) => (row.kind === 'record' ? recordButton(row) : row.kind === 'round' ? roundHeader(row) : null)}
                          </For>
                        </div>
                      </Show>
                    </section>
                  )}
                </For>
              </div>
            }
          >
            <div class="verevon-chat-ledger__turns">
              <div
                class="verevon-chat-ledger__window"
                ref={(element) => { windowEl = element }}
                onScroll={onWindowScroll}
              >
                <For each={mountedRows()}>{(row) => windowedRow(row)}</For>
                <Show when={(plan()?.gapAfter ?? 0) > 0}>
                  <div aria-hidden="true" style={{ height: `${plan()?.gapAfter ?? 0}px` }} />
                </Show>
              </div>
              <Show when={runWaterfallEnabled ? selected()?.turn.runId : undefined}>
                {(runId) => <RunTimingPanel runId={runId()} />}
              </Show>
            </div>
          </Show>
          <Show when={selected()}>
            {(entry) => (
              <LedgerInspector
                turn={entry().turn}
                record={entry().record}
                title={recordTitle(entry().record, i18n)}
                status={callStatus(entry().record, i18n)}
                onClose={() => setSelectedKey(null)}
              />
            )}
          </Show>
        </div>
      </Show>
    </div>
  )
}
