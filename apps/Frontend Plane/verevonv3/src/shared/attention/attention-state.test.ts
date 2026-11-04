import { describe, expect, it } from 'vitest'
import {
  ATTENTION_PRECEDENCE,
  ATTENTION_STATES,
  attentionSection,
  isAttentionState,
  isWaitingState,
  legacyStateFromRunStatus,
} from './attention-state'

describe('attention vocabulary', () => {
  it('is the closed set Session Core derives, in its precedence order from the most urgent down', () => {
    // The order is the server's (session-core attention.rs precedence): a drift here would
    // reorder the rail against the badge.
    expect(ATTENTION_PRECEDENCE).toEqual([
      'needs_approval',
      'needs_input',
      'plan_ready',
      'working',
      'failed',
      'completed',
      'archived',
      'idle',
    ])
    expect([...ATTENTION_PRECEDENCE].sort()).toEqual([...ATTENTION_STATES].sort())
  })

  it('accepts only the closed names, never a value it does not know', () => {
    expect(isAttentionState('needs_approval')).toBe(true)
    expect(isAttentionState('NEEDS_APPROVAL')).toBe(false)
    expect(isAttentionState('awaiting_approval')).toBe(false)
    expect(isAttentionState(undefined)).toBe(false)
    expect(isAttentionState(4)).toBe(false)
  })

  it('counts only the waiting states toward the badge', () => {
    expect(isWaitingState('needs_approval')).toBe(true)
    expect(isWaitingState('needs_input')).toBe(true)
    expect(isWaitingState('plan_ready')).toBe(true)
    expect(isWaitingState('working')).toBe(false)
    expect(isWaitingState('failed')).toBe(false)
    expect(isWaitingState(undefined)).toBe(false)
  })

  it('places a thread in the waiting and working sections by state, and in unseen only when a finished run is unread', () => {
    expect(attentionSection('needs_approval', false)).toBe('waiting')
    expect(attentionSection('plan_ready', true)).toBe('waiting')
    expect(attentionSection('working', false)).toBe('working')
    expect(attentionSection('completed', true)).toBe('unseen')
    expect(attentionSection('failed', true)).toBe('unseen')
    expect(attentionSection('completed', false)).toBeNull()
    expect(attentionSection('archived', true)).toBeNull()
    expect(attentionSection('idle', true)).toBeNull()
    expect(attentionSection(undefined, true)).toBeNull()
  })

  it('reads every legacy run status the chip and the filters knew, and nothing else', () => {
    const expected: Array<[string, ReturnType<typeof legacyStateFromRunStatus>]> = [
      ['queued', 'working'],
      ['pending', 'working'],
      ['running', 'working'],
      ['in_progress', 'working'],
      ['awaiting_approval', 'needs_approval'],
      ['waiting_approval', 'needs_approval'],
      ['paused', 'needs_input'],
      ['blocked', 'needs_input'],
      ['ambiguous', 'needs_input'],
      ['failed', 'failed'],
      ['timed_out', 'failed'],
      ['denied', 'failed'],
      ['rejected', 'failed'],
      ['completed', 'completed'],
      ['cancelled', 'idle'],
      ['  RUNNING ', 'working'],
      ['mystery', undefined],
      ['', undefined],
    ]
    for (const [status, state] of expected) {
      expect(legacyStateFromRunStatus(status), status).toBe(state)
    }
    expect(legacyStateFromRunStatus(undefined)).toBeUndefined()
  })
})
