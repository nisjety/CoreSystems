import { describe, expect, it } from 'vitest'
import * as attention from '@/shared/attention/attention-state'
import {
  ACTIVE_RUN_STATUSES,
  ATTENTION_PRECEDENCE,
  AWAITING_APPROVAL_RUN_STATUS,
  FAILED_RUN_STATUSES,
  LIVE_RUN_STATUSES,
  attentionSection,
  isQueuedRunStatus,
  isStoppedRunStatus,
  isWaitingState,
  legacyStateFromRunStatus,
} from './thread-status'

describe('thread status', () => {
  it('re-exports the attention vocabulary without forking it', () => {
    expect(legacyStateFromRunStatus).toBe(attention.legacyStateFromRunStatus)
    expect(attentionSection).toBe(attention.attentionSection)
    expect(isWaitingState).toBe(attention.isWaitingState)
    expect(ATTENTION_PRECEDENCE).toBe(attention.ATTENTION_PRECEDENCE)
  })

  it('reads each run status the thread surfaces name as one attention state', () => {
    expect(legacyStateFromRunStatus('queued')).toBe('working')
    expect(legacyStateFromRunStatus('running')).toBe('working')
    expect(legacyStateFromRunStatus('awaiting_approval')).toBe('needs_approval')
    expect(legacyStateFromRunStatus('paused')).toBe('needs_input')
    expect(legacyStateFromRunStatus('completed')).toBe('completed')
    expect(legacyStateFromRunStatus('failed')).toBe('failed')
    expect(legacyStateFromRunStatus('cancelled')).toBe('idle')
  })

  it('names a stopped run in both spellings and no other run', () => {
    expect(isStoppedRunStatus('cancelled')).toBe(true)
    expect(isStoppedRunStatus(' Canceled ')).toBe(true)
    expect(isStoppedRunStatus('completed')).toBe(false)
    expect(isStoppedRunStatus(undefined)).toBe(false)
  })

  it('names a queued run apart from a running one', () => {
    expect(isQueuedRunStatus('queued')).toBe(true)
    expect(isQueuedRunStatus('QUEUED')).toBe(true)
    expect(isQueuedRunStatus('running')).toBe(false)
    expect(isQueuedRunStatus(undefined)).toBe(false)
  })

  it('keeps the run sets the room has always classified with', () => {
    expect([...LIVE_RUN_STATUSES]).toEqual(['queued', 'running'])
    expect([...ACTIVE_RUN_STATUSES]).toEqual(['queued', 'running', 'awaiting_approval'])
    expect([...FAILED_RUN_STATUSES]).toEqual(['failed'])
    expect(AWAITING_APPROVAL_RUN_STATUS).toBe('awaiting_approval')
  })

  // Grep check (the plan's WS15 S6 gate): the thread surfaces delegate their run derivation to
  // this module and name no run-status token of their own. The sidebar, the thread organiser and
  // the room share one list; the console keeps its own RunStatus union (running, paused, cancelled)
  // and is checked only for the tokens no console or room enum uses. `completed` and `failed` are
  // also attention states, so they are not scanned for.
  it('names no run-status token in the thread surfaces outside this module', async () => {
    const { readFileSync } = await import('node:fs')
    const { resolve } = await import('node:path')
    const roomTokens = ['queued', 'running', 'awaiting_approval', 'paused', 'cancelled', 'canceled', 'blocked', 'ambiguous', 'waiting_approval', 'pending', 'in_progress']
    const consoleTokens = ['queued', 'awaiting_approval', 'canceled', 'blocked', 'ambiguous', 'waiting_approval', 'pending', 'in_progress']
    const surfaces: Array<[string, string[]]> = [
      ['features/core/components/CoreSidebar.tsx', roomTokens],
      ['features/chat/lib/chat-organization.ts', roomTokens],
      ['features/spaces/lib/space-thread-presentation.ts', roomTokens],
      ['features/agents/components/AgentRunConsole.tsx', consoleTokens],
    ]
    for (const [file, tokens] of surfaces) {
      const source = readFileSync(resolve(process.cwd(), 'src', file), 'utf8')
      expect(source, `${file} delegates to thread-status`).toContain("from '@/shared/chat/thread-status'")
      // Full-line comments may name a token in prose; only code is scanned.
      const code = source.split('\n').filter((line) => !/^\s*(\/\/|\/?\*)/.test(line)).join('\n')
      const named = tokens.filter((token) => new RegExp(`['"]${token}['"]`).test(code))
      expect(named, `${file} names a run-status token`).toEqual([])
    }
  })
})
