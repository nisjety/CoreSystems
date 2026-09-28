import { describe, expect, it } from 'vitest'
import type { ChatMessage } from '@/shared/api/chat-client'
import type { RunDetail } from '@/shared/api/runs-client'
import {
  cancelledRunTurn,
  failedRunMessage,
  formatUsageSummary,
  savedAnswerForTrailingTurn,
  unsettledRunTurn,
} from './chat-normalizers'
import type { ChatTurn } from './chat-types'

const turn = (overrides: Partial<ChatTurn>): ChatTurn => ({
  id: 'id',
  role: 'user',
  content: 'q',
  createdAt: '',
  streaming: false,
  tools: [],
  attachments: [],
  ...overrides,
})

describe('savedAnswerForTrailingTurn', () => {
  const local = [
    turn({ id: 'u1', content: 'First question' }),
    turn({ id: 'a1', role: 'assistant', content: 'First answer' }),
    turn({ id: 'u2', content: 'Write the plan' }),
    turn({ id: 'a2', role: 'assistant', content: 'Partial', status: 'stopped' }),
  ]

  it('finds the answer the server saved after the trailing question', () => {
    const server = [
      turn({ id: 's1', content: 'First question' }),
      turn({ id: 's2', role: 'assistant', content: 'First answer' }),
      turn({ id: 's3', content: 'Write  the plan\n\n--- VEDLEGG' }),
      turn({ id: 's4', role: 'assistant', content: 'The saved plan.' }),
    ]
    expect(savedAnswerForTrailingTurn(local, 'a2', server)?.content).toBe('The saved plan.')
  })

  it('finds nothing while only the question is saved', () => {
    const server = [
      turn({ id: 's1', content: 'First question' }),
      turn({ id: 's2', role: 'assistant', content: 'First answer' }),
      turn({ id: 's3', content: 'Write the plan' }),
    ]
    expect(savedAnswerForTrailingTurn(local, 'a2', server)).toBeNull()
  })

  it('refuses whenever the correlation is in doubt', () => {
    const answered = [
      turn({ id: 's1', content: 'First question' }),
      turn({ id: 's2', role: 'assistant', content: 'First answer' }),
    ]
    // The trailing question never reached the server: the last saved answer
    // belongs to an EARLIER question and must not be adopted.
    expect(savedAnswerForTrailingTurn(local, 'a2', answered)).toBeNull()
    // Another device added a question: positions no longer line up.
    expect(savedAnswerForTrailingTurn(local, 'a2', [
      ...answered,
      turn({ id: 'x', content: 'Write the plan' }),
      turn({ id: 'x2', role: 'assistant', content: 'Other device answer' }),
      turn({ id: 's3', content: 'Write the plan' }),
      turn({ id: 's4', role: 'assistant', content: 'Saved' }),
    ])).toBeNull()
    // A different last question.
    expect(savedAnswerForTrailingTurn(local, 'a2', [
      ...answered,
      turn({ id: 's3', content: 'Something else' }),
      turn({ id: 's4', role: 'assistant', content: 'Saved' }),
    ])).toBeNull()
    // Not the trailing turn.
    expect(savedAnswerForTrailingTurn(local, 'a1', answered)).toBeNull()
    // An empty saved message is not an answer.
    expect(savedAnswerForTrailingTurn(local, 'a2', [
      ...answered,
      turn({ id: 's3', content: 'Write the plan' }),
      turn({ id: 's4', role: 'assistant', content: '  ' }),
    ])).toBeNull()
  })
})

describe('unsettledRunTurn', () => {
  const message: ChatMessage = { id: '01M2ZNAY0W7NFJY543JZ2DJZTB', role: 'user', content: 'Draft from sources.', createdAt: '' }
  const run = (status: string, overrides: Partial<RunDetail> = {}): RunDetail => ({
    runId: '01M2ZNAZ0W48WCREHFTGPKPNV2',
    threadId: 'thread',
    status,
    goal: 'Draft from sources.',
    checkpointIndex: 0,
    stepsCompleted: 0,
    inputTokens: 0,
    outputTokens: 0,
    updatedAt: '2026-09-23T10:00:00.000Z',
    ...overrides,
  })

  it('still produces the cancelled tail exactly as cancelledRunTurn does', () => {
    const cancelled = run('cancelled')
    expect(unsettledRunTurn('thread', cancelled.runId, message, cancelled)).toEqual(cancelledRunTurn('thread', cancelled.runId, message, cancelled))
  })

  it('shows a failed run as its translated failure, never the raw code', () => {
    const failed = run('failed', { error: 'provider_unavailable' })
    expect(unsettledRunTurn('thread', failed.runId, message, failed)).toMatchObject({
      role: 'assistant', status: 'error', streaming: false, runId: failed.runId,
      content: 'Modelltjenesten var utilgjengelig, så svaret ble ikke fullført.',
    })
  })

  it('holds an active run as a pending turn with no request id to resume', () => {
    for (const status of ['queued', 'running', 'awaiting_approval']) {
      const active = run(status)
      const tail = unsettledRunTurn('thread', active.runId, message, active)
      expect(tail).toMatchObject({ role: 'assistant', status: 'waiting', streaming: true, runId: active.runId, content: '' })
      expect(tail?.requestId).toBeUndefined()
    }
  })

  it('keeps the correlation rule: no completed, foreign, older or mismatched run', () => {
    for (const invalid of [
      run('completed'),
      run('running', { threadId: 'other' }),
      run('running', { runId: '01M2ZNAX0W48WCREHFTGPKPNV2' }),
      run('failed', { goal: 'Another question' }),
      run('running', { parentRunId: 'parent' }),
    ]) expect(unsettledRunTurn('thread', invalid.runId, message, invalid)).toBeNull()
  })
})

describe('failedRunMessage', () => {
  it('translates the fixed Session Core failure vocabulary', () => {
    expect(failedRunMessage('provider_timeout')).toContain('svarte ikke i tide')
    expect(failedRunMessage('execution_failed')).toContain('kunne ikke lagres')
    expect(failedRunMessage('inference_failed')).toBe('Kjøringen feilet før svaret ble lagret.')
    expect(failedRunMessage(undefined)).toBe('Kjøringen feilet før svaret ble lagret.')
  })
})

describe('formatUsageSummary cost basis', () => {
  it('never prints a subscription placeholder as a dollar amount', () => {
    expect(formatUsageSummary({ inputTokens: 10, outputTokens: 5, costUsd: 0, costBasis: 'subscription' }))
      .toBe('10 in / 5 out tokens · included in subscription')
    expect(formatUsageSummary({ costUsd: 0.0031, costBasis: 'metered' })).toBe('$0.0031')
    expect(formatUsageSummary({ costUsd: 0.0031 })).toBe('$0.0031')
  })
})
