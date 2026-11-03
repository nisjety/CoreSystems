/**
 * The run ledger (EP-14 S4): the Trace grouped by turn, then by tool call.
 *
 * Pure. The Trace hands in the events the chat controller already holds and gets back the turns in
 * the order their recorded times put them. Nothing is fetched or stored, and no time is estimated: a
 * duration is the gap between two recorded `at` values and is absent when either end is missing. An
 * event that no turn owns stays in one unattributed group, so the ledger never drops a record it was
 * given.
 *
 * Behind `VITE_CHAT_RUN_LEDGER`, read through `runLedgerEnabled`: off, the Trace is the flat list.
 */

import type { VerevonUiEvent } from '@/shared/chat/verevon-ui-events'

/** Only the literal `true` enables the ledger, as for the other chat switches. */
export function isRunLedgerEnabled(raw: string | undefined): boolean {
  return raw === 'true'
}

/** Fixed per build, like every `VITE_` switch: read once, at load. */
export const runLedgerEnabled = isRunLedgerEnabled(import.meta.env.VITE_CHAT_RUN_LEDGER)

/**
 * How a turn ended, from the last terminal event recorded for it. `open` means the record holds no
 * terminal event for the turn. That is not a failure, and the ledger does not present it as one.
 */
export type LedgerOutcome = 'completed' | 'failed' | 'stopped' | 'open'

/** A tool call and its result, paired by the call id both events carry. */
export type LedgerCall = {
  kind: 'call'
  key: string
  /** The call id the stream sent. Absent when the stream sent none. */
  id?: string
  name?: string
  /** `running` until a result is recorded for the call. */
  status: 'running' | 'completed' | 'failed'
  startAt: string
  endAt?: string
  durationMs?: number
}

/**
 * Any other recorded event. `unmatched` marks a tool result whose call the turn never recorded, so
 * the reader can see that the record is incomplete rather than read it as a whole call.
 */
export type LedgerEventRecord = {
  kind: 'event' | 'unmatched'
  key: string
  event: VerevonUiEvent
}

export type LedgerRecord = LedgerCall | LedgerEventRecord

export type LedgerTurn = {
  /** The turn the records belong to. Null for the events no turn owns. */
  turnId: string | null
  startAt: string
  endAt: string
  requestId?: string
  runId?: string
  /** Absent for the unattributed group, which has no turn to end. */
  outcome?: LedgerOutcome
  receiptIds: string[]
  /** In recorded-time order. */
  records: LedgerRecord[]
}

type ToolCallEvent = Extract<VerevonUiEvent, { type: 'tool.call' }>
type ToolResultEvent = Extract<VerevonUiEvent, { type: 'tool.result' }>

const UNATTRIBUTED = 'unattributed'
/** A resumed stream is its own segment of a turn, recorded under `<turn>:resume`. */
const RESUME_SUFFIX = ':resume'

const TERMINAL_OUTCOMES: Partial<Record<VerevonUiEvent['type'], LedgerOutcome>> = {
  'message.done': 'completed',
  'run.completed': 'completed',
  'run.error': 'failed',
  'run.stopped': 'stopped',
  'run.cancelled': 'stopped',
}

/** The key a turn is shown and collapsed under. */
export function turnKey(turn: Pick<LedgerTurn, 'turnId'>): string {
  return turn.turnId === null ? UNATTRIBUTED : `turn:${turn.turnId}`
}

/** A recorded time as milliseconds. A time that does not parse sorts last; it is never guessed. */
function timeOf(at: string): number {
  const time = Date.parse(at)
  return Number.isNaN(time) ? Number.POSITIVE_INFINITY : time
}

function compareTimes(a: number, b: number): number {
  return a === b ? 0 : a < b ? -1 : 1
}

/** The gap between two recorded times, or undefined when either is missing or they run backwards. */
export function spanMs(from: string, to: string | undefined): number | undefined {
  if (to === undefined) return undefined
  const start = timeOf(from)
  const end = timeOf(to)
  if (!Number.isFinite(start) || !Number.isFinite(end) || end < start) return undefined
  return end - start
}

export function recordAt(record: LedgerRecord): string {
  return record.kind === 'call' ? record.startAt : record.event.at
}

/** Clock time with seconds, in the page locale. Empty when the recorded time does not parse. */
export function formatClock(at: string, locale: 'no' | 'en'): string {
  if (Number.isNaN(Date.parse(at))) return ''
  return new Intl.DateTimeFormat(locale === 'no' ? 'nb-NO' : 'en', {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  }).format(new Date(at))
}

function turnIdOf(event: VerevonUiEvent): string | null {
  const turnId = event.turnId
  if (!turnId) return null
  return turnId.endsWith(RESUME_SUFFIX) ? turnId.slice(0, -RESUME_SUFFIX.length) : turnId
}

function requestIdOf(event: VerevonUiEvent): string | undefined {
  switch (event.type) {
    case 'run.connected':
    case 'message.done':
    case 'run.error':
    case 'run.stopped':
      return event.requestId || undefined
    default:
      return undefined
  }
}

function runIdOf(event: VerevonUiEvent): string | undefined {
  return 'runId' in event ? event.runId || undefined : undefined
}

function receiptIdOf(event: VerevonUiEvent): string | undefined {
  return event.type === 'receipt.verified' || event.type === 'run.cancelled' ? event.receiptId || undefined : undefined
}

/** A tool result counts as failed as the tool row does: it carries an error or says so. */
function settle(call: LedgerCall, result: ToolResultEvent): void {
  call.endAt = result.at
  call.status =result.error || result.status === 'error' ? 'failed' : 'completed'
  call.durationMs = spanMs(call.startAt, result.at)
}

type Draft = {
  turnId: string | null
  startAt: string
  endAt: string
  startTime: number
  endTime: number
  requestId?: string
  runId?: string
  outcome?: LedgerOutcome
  outcomeTime: number
  receiptIds: string[]
  records: LedgerRecord[]
  callsById: Map<string, LedgerCall>
}

function newDraft(turnId: string | null): Draft {
  return {
    turnId,
    startAt: '',
    endAt: '',
    startTime: Number.POSITIVE_INFINITY,
    endTime: Number.NEGATIVE_INFINITY,
    outcomeTime: Number.NEGATIVE_INFINITY,
    receiptIds: [],
    records: [],
    callsById: new Map(),
  }
}

function keyOf(draft: Draft, token: string): string {
  return `${turnKey(draft)}/${token}`
}

function absorbToolCall(draft: Draft, event: ToolCallEvent, index: number): void {
  const known = event.id === undefined ? undefined : draft.callsById.get(event.id)
  // The stream sends a call's name and its arguments in separate frames under one id.
  if (known) {
    known.name ??= event.name
    return
  }
  const call: LedgerCall = {
    kind: 'call',
    key: keyOf(draft, `call/${event.id ?? index}`),
    id: event.id,
    name: event.name,
    status: 'running',
    startAt: event.at,
  }
  draft.records.push(call)
  if (event.id !== undefined) draft.callsById.set(event.id, call)
}

function absorbToolResult(draft: Draft, event: ToolResultEvent, index: number): void {
  const call = event.id === undefined ? undefined : draft.callsById.get(event.id)
  if (call && call.endAt === undefined) {
    settle(call, event)
    return
  }
  const kind = call ? 'event' : 'unmatched'
  draft.records.push({ kind, key: keyOf(draft, `result/${index}`), event })
}

function absorb(draft: Draft, event: VerevonUiEvent, index: number): void {
  const time = timeOf(event.at)
  if (draft.startAt === '' || time < draft.startTime) {
    draft.startTime = time
    draft.startAt = event.at
  }
  if (draft.endAt === '' || time >= draft.endTime) {
    draft.endTime = time
    draft.endAt = event.at
  }
  draft.requestId ??= requestIdOf(event)
  draft.runId ??= runIdOf(event)
  const terminal = TERMINAL_OUTCOMES[event.type]
  if (terminal !== undefined && (draft.outcome === undefined || time >= draft.outcomeTime)) {
    draft.outcome = terminal
    draft.outcomeTime = time
  }
  const receiptId = receiptIdOf(event)
  if (receiptId !== undefined && !draft.receiptIds.includes(receiptId)) draft.receiptIds.push(receiptId)

  if (event.type === 'tool.call') absorbToolCall(draft, event, index)
  else if (event.type === 'tool.result') absorbToolResult(draft, event, index)
  else draft.records.push({ kind: 'event', key: keyOf(draft, `event/${index}`), event })
}

function finish(draft: Draft): LedgerTurn {
  return {
    turnId: draft.turnId,
    startAt: draft.startAt,
    endAt: draft.endAt,
    requestId: draft.requestId,
    runId: draft.runId,
    outcome: draft.turnId === null ? undefined : (draft.outcome ?? 'open'),
    receiptIds: draft.receiptIds,
    records: draft.records
      .slice()
      .sort((a, b) => compareTimes(timeOf(recordAt(a)), timeOf(recordAt(b)))),
  }
}

/**
 * Groups recorded events by turn, then by tool call. Turns come out in the order their first recorded
 * time puts them; each turn's records come out in recorded-time order, with ties kept in arrival order.
 * Every input event lands in exactly one record, either on its own or folded into the call it belongs to.
 */
export function buildLedger(events: readonly VerevonUiEvent[]): LedgerTurn[] {
  const drafts = new Map<string, Draft>()
  events.forEach((event, index) => {
    const turnId = turnIdOf(event)
    const key = turnKey({ turnId })
    let draft = drafts.get(key)
    if (draft === undefined) {
      draft = newDraft(turnId)
      drafts.set(key, draft)
    }
    absorb(draft, event, index)
  })
  return [...drafts.values()]
    .sort((a, b) => compareTimes(a.startTime, b.startTime))
    .map(finish)
}
