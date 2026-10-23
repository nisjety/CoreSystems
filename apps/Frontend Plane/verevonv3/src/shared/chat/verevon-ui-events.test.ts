import { describe, expect, it } from 'vitest'
import { toVerevonUiEvent } from './verevon-ui-events'

const AT = '2026-10-23T10:00:00.000Z'

describe('lifecycle notices (WS15 S1)', () => {
  it('projects a retry notice into a lifecycle.retry cue with its counts and token', () => {
    expect(
      toVerevonUiEvent(
        'retry_scheduled',
        { scope: 'tool', tool: 'knowledge_search', attempt: 2, max_attempts: 3, wait_ms: 250, reason: 'rate_limited', request_id: 'req-1' },
        AT,
      ),
    ).toEqual({
      type: 'lifecycle.retry',
      at: AT,
      scope: 'tool',
      tool: 'knowledge_search',
      attempt: 2,
      maxAttempts: 3,
      waitMs: 250,
      reason: 'rate_limited',
    })
  })

  it('projects compaction and context_change, leaving unmeasured counts undefined', () => {
    expect(toVerevonUiEvent('compaction', { scope: 'history', outcome: 'truncated' }, AT)).toEqual({
      type: 'lifecycle.compaction',
      at: AT,
      scope: 'history',
      outcome: 'truncated',
      messagesCompacted: undefined,
      messagesKept: undefined,
      pinnedKept: undefined,
    })
    expect(
      toVerevonUiEvent('context_change', { change: 'history_shed', messages_dropped: 4, layer: 'memory', state: 'skipped_by_policy' }, AT),
    ).toEqual({
      type: 'lifecycle.context',
      at: AT,
      change: 'history_shed',
      messagesDropped: 4,
      skillsTruncated: undefined,
      skillsDropped: undefined,
      layer: 'memory',
      state: 'skipped_by_policy',
    })
  })

  it('projects a lifecycle notice that arrives inside the AG-UI CUSTOM envelope', () => {
    expect(
      toVerevonUiEvent('CUSTOM', { name: 'compaction', value: { scope: 'tool_results', outcome: 'cleared' } }, AT),
    ).toMatchObject({ type: 'lifecycle.compaction', scope: 'tool_results', outcome: 'cleared' })
  })
})
