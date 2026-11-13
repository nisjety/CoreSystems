// @vitest-environment jsdom

// Verification plan 3.4, VITE_APPROVAL_RULE_PROPOSALS (W2-WS13-S7-spa-proposals): the integrations settings mount the
// approval rule proposals section only for the literal "true". Off, nothing is read and nothing renders. The "on" case
// is the control, so an "off" pass cannot come from a page that never reaches the section.

import { createRouter, memoryHistory } from '@solidjs/router'
import { QueryClient, QueryClientProvider } from '@tanstack/solid-query'
import { cleanup, render } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const { listApprovalRuleProposals } = vi.hoisted(() => ({ listApprovalRuleProposals: vi.fn() }))

vi.mock('@/shared/api/orchestration-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/orchestration-client')>()
  return { ...actual, listApprovalRuleProposals, confirmApprovalRule: vi.fn(), dismissApprovalRule: vi.fn() }
})

import { VerevonWorkspaceSettingsPage } from './WorkspaceSettingsPage'

function renderIntegrations() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const TestRouter = createRouter({
    routes: [{ path: '/*all', component: () => <VerevonWorkspaceSettingsPage section="integrations" /> }],
    history: memoryHistory(),
    explicitLinks: true,
  })
  return render(() => (
    <QueryClientProvider client={queryClient}>
      <TestRouter>{(props) => <>{props.children}</>}</TestRouter>
    </QueryClientProvider>
  ))
}

async function settle() {
  for (let i = 0; i < 3; i++) {
    await new Promise((resolve) => setTimeout(resolve, 0))
    flush()
  }
}

beforeEach(() => {
  listApprovalRuleProposals.mockReset().mockResolvedValue([])
  vi.stubGlobal(
    'fetch',
    vi.fn(async () => new Response('{}', { headers: { 'Content-Type': 'application/json' }, status: 200 })),
  )
})

afterEach(() => {
  cleanup()
  vi.unstubAllEnvs()
  vi.unstubAllGlobals()
})

describe('VITE_APPROVAL_RULE_PROPOSALS on the integrations settings', () => {
  it.each([[undefined], ['1'], ['TRUE'], ['yes']])('off for %j: reads no proposals', async (value) => {
    if (value !== undefined) vi.stubEnv('VITE_APPROVAL_RULE_PROPOSALS', value)
    renderIntegrations()
    await settle()
    expect(listApprovalRuleProposals).not.toHaveBeenCalled()
  })

  it('on for the literal "true": reads the proposals (control)', async () => {
    vi.stubEnv('VITE_APPROVAL_RULE_PROPOSALS', 'true')
    renderIntegrations()
    await settle()
    expect(listApprovalRuleProposals).toHaveBeenCalled()
  })
})
