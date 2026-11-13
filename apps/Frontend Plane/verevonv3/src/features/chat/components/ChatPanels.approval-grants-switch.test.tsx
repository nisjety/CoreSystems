// @vitest-environment jsdom

// Verification plan 3.4, VITE_APPROVAL_SCOPE_GRANTS (W2-WS13-S6-spa-grants): the Steps panel mounts the thread's
// standing approvals only for the literal "true" and only on a saved thread. Off, it asks nothing and renders
// nothing. The "on" case is the control.

import { cleanup, render } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const { listApprovalGrants } = vi.hoisted(() => ({ listApprovalGrants: vi.fn() }))

vi.mock('@/shared/api/orchestration-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/orchestration-client')>()
  return {
    ...actual,
    listApprovalGrants,
    revokeApprovalGrant: vi.fn(),
    listPlans: vi.fn(async () => []),
    listTodos: vi.fn(async () => []),
  }
})

import { StepsPanel } from './ChatPanels'

// One settled step, so the panel renders its body rather than the empty state.
const STEPS = [{ id: 'count', title: 'Count words', detail: '17 words', status: 'done' as const, createdAt: '2026-09-20T07:00:00Z' }]

async function settle() {
  for (let i = 0; i < 3; i++) {
    await new Promise((resolve) => setTimeout(resolve, 0))
    flush()
  }
}

beforeEach(() => {
  listApprovalGrants.mockReset().mockResolvedValue([])
})

afterEach(() => {
  cleanup()
  vi.unstubAllEnvs()
})

describe('VITE_APPROVAL_SCOPE_GRANTS in the Steps panel', () => {
  it.each([[undefined], ['1'], ['TRUE'], ['yes']])('off for %j: no grants read on a saved thread', async (value) => {
    if (value !== undefined) vi.stubEnv('VITE_APPROVAL_SCOPE_GRANTS', value)
    render(() => <StepsPanel steps={STEPS} threadId="thread-1" onStopTask={() => undefined} />)
    await settle()
    expect(listApprovalGrants).not.toHaveBeenCalled()
  })

  it('on for the literal "true": reads the grants of the saved thread (control)', async () => {
    vi.stubEnv('VITE_APPROVAL_SCOPE_GRANTS', 'true')
    render(() => <StepsPanel steps={STEPS} threadId="thread-1" onStopTask={() => undefined} />)
    await settle()
    expect(listApprovalGrants).toHaveBeenCalledWith('thread-1')
  })

  it('on, but with no saved thread: reads nothing', async () => {
    vi.stubEnv('VITE_APPROVAL_SCOPE_GRANTS', 'true')
    render(() => <StepsPanel steps={STEPS} threadId={null} onStopTask={() => undefined} />)
    await settle()
    expect(listApprovalGrants).not.toHaveBeenCalled()
  })
})
