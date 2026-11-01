import { describe, expect, it } from 'vitest'
import { toVerevonUiEvent } from './verevon-ui-events'

const AT = '2026-10-23T10:00:00.000Z'

describe('usage progress (WS27 S3)', () => {
  it('projects a progress frame into usage.progress with its closed fields', () => {
    expect(
      toVerevonUiEvent(
        'usage_progress',
        { calls: 2, phase: 'tool_round', input_tokens: 1200, output_tokens: 90, cache_read_tokens: 0, cache_write_tokens: 0, cost_usd: 0.0041, cost_basis: 'metered', request_id: 'req-1' },
        AT,
      ),
    ).toEqual({
      type: 'usage.progress',
      at: AT,
      calls: 2,
      phase: 'tool_round',
      inputTokens: 1200,
      outputTokens: 90,
      cacheReadTokens: 0,
      cacheWriteTokens: 0,
      costUsd: 0.0041,
      costBasis: 'metered',
    })
  })

  it('leaves an unpriced call without a cost and states its count', () => {
    expect(toVerevonUiEvent('usage_progress', { calls: 3, phase: 'overhead', cost_usd: null, unpriced_calls: 1 }, AT)).toEqual({
      type: 'usage.progress',
      at: AT,
      calls: 3,
      phase: 'overhead',
      unpricedCalls: 1,
    })
  })

  it('projects the same frame from an AG-UI CUSTOM envelope', () => {
    expect(
      toVerevonUiEvent('CUSTOM', { name: 'usage_progress', value: { calls: 2, phase: 'answer', cost_usd: 0.002, cost_basis: 'metered' } }, AT),
    ).toEqual({ type: 'usage.progress', at: AT, calls: 2, phase: 'answer', costUsd: 0.002, costBasis: 'metered' })
  })
})

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

describe('sandbox workspace state (WS22 S6)', () => {
  it('projects the workspace frame into lifecycle.sandbox with its closed tokens only', () => {
    expect(toVerevonUiEvent('sandbox_state', { scope: 'thread', state: 'kept', request_id: 'req-1' }, AT)).toEqual({
      type: 'lifecycle.sandbox',
      at: AT,
      scope: 'thread',
      state: 'kept',
      cause: undefined,
    })
  })

  it('projects a conflict with its cause, from the native frame and from the AG-UI CUSTOM envelope', () => {
    const conflict = { scope: 'thread', state: 'conflict', cause: 'workspace_conflict' }
    expect(toVerevonUiEvent('sandbox_state', conflict, AT)).toMatchObject({
      type: 'lifecycle.sandbox',
      state: 'conflict',
      cause: 'workspace_conflict',
    })
    expect(toVerevonUiEvent('CUSTOM', { name: 'sandbox_state', value: conflict }, AT)).toMatchObject({
      type: 'lifecycle.sandbox',
      state: 'conflict',
      cause: 'workspace_conflict',
    })
  })
})
