/**
 * The run ledger view (EP-14 S4): the Trace as turns, each holding its records in recorded-time order.
 * A tool call is one row, paired from its call and its result. A row opens in the inspector beside the
 * list.
 *
 * Nothing here takes focus. The arrow keys move focus between rows the reader is already on, and an
 * Inspect action from a tool card selects its call without moving focus. The page mounts this only
 * behind `VITE_CHAT_RUN_LEDGER` (see `run-ledger.ts`). With `VITE_CHAT_RUN_WATERFALL` on, a turn of a
 * known run also offers its durable timing (`RunWaterfall.tsx`).
 */

import { createEffect, createMemo, createSignal, For, Show, untrack } from 'solid-js'
import { uiEventLabel, type VerevonUiEvent } from '@/shared/chat/verevon-ui-events'
import { useI18n } from '@/shared/i18n'
import { formatLatency } from './chat-media-markdown'
import { LedgerInspector } from './LedgerInspector'
import { RunTimingPanel, runWaterfallEnabled } from './RunWaterfall'
import {
  buildLedger,
  formatClock,
  recordAt,
  spanMs,
  turnKey,
  type LedgerOutcome,
  type LedgerRecord,
  type LedgerTurn,
} from '../lib/run-ledger'

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

function callDuration(record: LedgerRecord): string | undefined {
  return record.kind === 'call' && record.durationMs !== undefined ? formatLatency(record.durationMs) : undefined
}

export function RunLedger(props: {
  events: readonly VerevonUiEvent[]
  replayTruncated?: boolean
  /** A tool call to select, from the Inspect action on its card. */
  inspectRequest?: { callId: string } | null
}) {
  const i18n = useI18n()
  let root: HTMLDivElement | undefined
  const turns = createMemo(() => buildLedger(props.events))
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
        setCallMissing(false)
        return
      }
      setSelectedKey(null)
      setCallMissing(true)
    },
  )

  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key === 'Escape' && selectedKey() !== null) {
      event.preventDefault()
      setSelectedKey(null)
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

  return (
    <div class="verevon-chat-ledger" ref={(element) => { root = element }} onKeyDown={onKeyDown}>
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
                    <strong>
                      {entry.number === 0
                        ? i18n.tr('Ikke knyttet til en tur', 'Not tied to a turn')
                        : i18n.tr(`Tur ${entry.number}`, `Turn ${entry.number}`)}
                    </strong>
                    <Show when={entry.turn.outcome}>
                      {(outcome) => <em>{outcomeText(outcome(), i18n)}</em>}
                    </Show>
                    <Show when={spanMs(entry.turn.startAt, entry.turn.endAt)}>
                      {(span) => <span>{formatLatency(span())}</span>}
                    </Show>
                    <span>{i18n.tr(`${entry.turn.records.length} poster`, `${entry.turn.records.length} records`)}</span>
                    <time>{formatClock(entry.turn.startAt, i18n.locale())}</time>
                  </button>
                  <Show when={!collapsed().has(entry.key)}>
                    <div class="verevon-chat-ledger__records">
                      {/* EP-14 S5: the run's durable timing, which survives a reload; fetched only on request. */}
                      <Show when={runWaterfallEnabled ? entry.turn.runId : undefined}>
                        {(runId) => <RunTimingPanel runId={runId()} />}
                      </Show>
                      <For each={entry.turn.records}>
                        {(record) => (
                          <button
                            type="button"
                            class="verevon-chat-ledger__row"
                            data-ledger-row
                            aria-current={selectedKey() === record.key ? 'true' : undefined}
                            onClick={() => setSelectedKey(record.key)}
                          >
                            <span class="verevon-chat-ledger__row-title">{recordTitle(record, i18n)}</span>
                            <Show when={callStatus(record, i18n)}>{(text) => <em>{text()}</em>}</Show>
                            <Show when={callDuration(record)}>{(text) => <em>{text()}</em>}</Show>
                            <time>{formatClock(recordAt(record), i18n.locale())}</time>
                          </button>
                        )}
                      </For>
                    </div>
                  </Show>
                </section>
              )}
            </For>
          </div>
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
