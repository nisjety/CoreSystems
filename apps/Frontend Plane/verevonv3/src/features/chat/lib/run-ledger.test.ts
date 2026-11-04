import { describe, expect, it } from 'vitest'
import type { VerevonUiEvent } from '@/shared/chat/verevon-ui-events'
import { buildLedger, isRunLedgerEnabled, recordAt, spanMs, type LedgerCall } from './run-ledger'

const at = (seconds: number) => new Date(Date.UTC(2026, 10, 3, 10, 0, seconds)).toISOString()

const toolCall = (id: string, seconds: number, turnId = 'turn-1'): VerevonUiEvent => ({
  type: 'tool.call',
  at: at(seconds),
  id,
  name: 'web_search',
  turnId,
})

const toolResult = (id: string, seconds: number, turnId = 'turn-1', error?: string): VerevonUiEvent => ({
  type: 'tool.result',
  at: at(seconds),
  id,
  status: error ? 'error' : 'completed',
  error,
  turnId,
})

const lifecycle = (seconds: number, turnId?: string): VerevonUiEvent => ({
  type: 'lifecycle.retry',
  at: at(seconds),
  turnId,
})

describe('isRunLedgerEnabled', () => {
  it('enables only on the literal true, as the other chat switches do', () => {
    expect(isRunLedgerEnabled('true')).toBe(true)
    for (const raw of [undefined, '', 'false', '1', 'TRUE', ' true']) {
      expect(isRunLedgerEnabled(raw)).toBe(false)
    }
  })
})

describe('spanMs', () => {
  it('is the gap between two recorded times', () => {
    expect(spanMs(at(0), at(3))).toBe(3000)
  })

  it('is absent when an end is missing, the times run backwards, or a time does not parse', () => {
    expect(spanMs(at(0), undefined)).toBeUndefined()
    expect(spanMs(at(3), at(0))).toBeUndefined()
    expect(spanMs('not a time', at(0))).toBeUndefined()
  })
})

describe('buildLedger grouping', () => {
  it('groups records under their turn, in the order the turns started', () => {
    const turns = buildLedger([
      lifecycle(5, 'turn-2'),
      toolCall('c1', 1, 'turn-1'),
      toolResult('c1', 2, 'turn-1'),
      lifecycle(6, 'turn-2'),
    ])
    expect(turns.map((turn) => turn.turnId)).toEqual(['turn-1', 'turn-2'])
    expect(turns[0]?.records.map((record) => record.kind)).toEqual(['call'])
    expect(turns[1]?.records).toHaveLength(2)
  })

  it('pairs a tool result with its call and takes the duration from the recorded times', () => {
    const [turn] = buildLedger([toolCall('c1', 0), toolResult('c1', 2)])
    const call = turn?.records[0] as LedgerCall
    expect(call).toMatchObject({
      kind: 'call',
      id: 'c1',
      name: 'web_search',
      status: 'completed',
      startAt: at(0),
      endAt: at(2),
      durationMs: 2000,
    })
  })

  it('marks a failed result as failed, from its error', () => {
    const [turn] = buildLedger([toolCall('c1', 0), toolResult('c1', 1, 'turn-1', 'boom')])
    expect(turn?.records[0]).toMatchObject({ kind: 'call', status: 'failed' })
  })

  it('keeps a call with no result as running, with no end and no duration', () => {
    const [turn] = buildLedger([toolCall('c1', 0)])
    const call = turn?.records[0] as LedgerCall
    expect(call).toMatchObject({ kind: 'call', status: 'running' })
    expect(call.endAt).toBeUndefined()
    expect(call.durationMs).toBeUndefined()
  })

  it('merges the name a later frame carries for a call under the same id', () => {
    const [turn] = buildLedger([
      { type: 'tool.call', at: at(0), id: 'c1', turnId: 'turn-1' },
      { type: 'tool.call', at: at(1), id: 'c1', name: 'read_file', turnId: 'turn-1' },
    ])
    expect(turn?.records).toHaveLength(1)
    expect(turn?.records[0]).toMatchObject({ kind: 'call', name: 'read_file', startAt: at(0) })
  })

  it('keeps a tool result that has no call as an unmatched record, never dropped', () => {
    const [turn] = buildLedger([toolResult('orphan', 1)])
    expect(turn?.records).toHaveLength(1)
    expect(turn?.records[0]).toMatchObject({ kind: 'unmatched' })
  })

  it('keeps a second result for a call that already has one as a plain record', () => {
    const [turn] = buildLedger([toolCall('c1', 0), toolResult('c1', 1), toolResult('c1', 2)])
    expect(turn?.records.map((record) => record.kind)).toEqual(['call', 'event'])
  })

  it('keeps events no turn owns in one unattributed group, with no outcome', () => {
    const turns = buildLedger([lifecycle(0), lifecycle(1), toolCall('c1', 2, 'turn-1')])
    const unattributed = turns.find((turn) => turn.turnId === null)
    expect(unattributed?.records).toHaveLength(2)
    expect(unattributed?.outcome).toBeUndefined()
  })

  it('folds a resumed stream into the turn it resumes', () => {
    const turns = buildLedger([
      { type: 'message.started', at: at(0), turnId: 'turn-1' },
      { type: 'message.done', at: at(3), turnId: 'turn-1:resume', requestId: 'req-1' },
    ])
    expect(turns).toHaveLength(1)
    expect(turns[0]).toMatchObject({ turnId: 'turn-1', outcome: 'completed', requestId: 'req-1' })
  })

  it('keeps every event in exactly one record, folding a result into its call', () => {
    const events = [
      toolCall('c1', 0),
      toolCall('c2', 1),
      toolResult('c1', 2),
      toolResult('c2', 3, 'turn-1', 'boom'),
      toolResult('orphan', 4),
      lifecycle(5, 'turn-1'),
      lifecycle(6),
    ]
    const turns = buildLedger(events)
    const records = turns.flatMap((turn) => turn.records)
    const folded = records.filter((record) => record.kind === 'call' && record.endAt !== undefined).length
    expect(records.length + folded).toBe(events.length)
  })
})

describe('buildLedger ordering', () => {
  it('orders records by their recorded time, not by arrival', () => {
    const [turn] = buildLedger([
      lifecycle(5, 'turn-1'),
      lifecycle(1, 'turn-1'),
      lifecycle(3, 'turn-1'),
    ])
    expect(turn?.records.map(recordAt)).toEqual([at(1), at(3), at(5)])
  })

  it('keeps arrival order for records that share a recorded time', () => {
    const first: VerevonUiEvent = { type: 'thread.titled', at: at(2), title: 'first', turnId: 'turn-1' }
    const second: VerevonUiEvent = { type: 'followups.suggested', at: at(2), suggestions: [], turnId: 'turn-1' }
    const [turn] = buildLedger([first, second])
    expect(turn?.records.map((record) => (record.kind === 'event' ? record.event.type : record.kind))).toEqual([
      'thread.titled',
      'followups.suggested',
    ])
  })

  it('orders turns by the earliest recorded time among their records', () => {
    const turns = buildLedger([lifecycle(9, 'late'), lifecycle(2, 'early'), lifecycle(4, 'middle')])
    expect(turns.map((turn) => turn.turnId)).toEqual(['early', 'middle', 'late'])
  })
})

describe('buildLedger turn facts', () => {
  it('reads the outcome from the last terminal event recorded for the turn', () => {
    const [turn] = buildLedger([
      { type: 'message.done', at: at(1), turnId: 'turn-1' },
      { type: 'run.error', at: at(2), code: 'boom', message: 'failed', turnId: 'turn-1' },
    ])
    expect(turn?.outcome).toBe('failed')
  })

  it('reports open, not failed, when the record holds no terminal event for the turn', () => {
    const [turn] = buildLedger([{ type: 'message.started', at: at(0), turnId: 'turn-1' }])
    expect(turn?.outcome).toBe('open')
  })

  it('takes the request and run ids and the receipts from the events that carry them', () => {
    const [turn] = buildLedger([
      { type: 'run.connected', at: at(0), requestId: 'req-1', runId: 'run-1', turnId: 'turn-1' },
      { type: 'tool.call', at: at(1), id: 'c1', runId: 'run-1', turnId: 'turn-1' },
      { type: 'receipt.verified', at: at(2), receiptId: 'rcpt-1', turnId: 'turn-1' },
      { type: 'receipt.verified', at: at(3), receiptId: 'rcpt-1', turnId: 'turn-1' },
    ])
    expect(turn).toMatchObject({ requestId: 'req-1', runId: 'run-1', receiptIds: ['rcpt-1'] })
  })
})
