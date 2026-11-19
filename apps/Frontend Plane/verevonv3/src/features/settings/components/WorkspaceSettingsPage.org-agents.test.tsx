// @vitest-environment jsdom

import { cleanup, render, screen } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { VerevonWorkspaceSettingsPage } from '@/features/settings/components/WorkspaceSettingsPage'
import { clearSession, markSessionOnboardingComplete, setSessionUser } from '@/shared/session/session-store'

// D19: the organization agents are their own settings section. They used to sit at the foot of the room-policy section,
// which is listed only while the room-policy admin API is on, so in a deployment with it off an admin had no way to
// them but the URL. The agent routes have no switch of their own.

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), { headers: { 'Content-Type': 'application/json' }, status })
}

/** An org with one org-visible agent, a room-policy admin API that answers "not switched on", and nothing else. */
function stubGateway() {
  const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input)
    if (url === '/api/v1/orgs/org_1/agents') {
      return json({ data: { agents: [{ agent_subject_id: 'agent-abc123', visibility: 'org', add_policy: 'managers', lifecycle: 'active', revision: 3 }] } })
    }
    if (url === '/api/v1/agents/installations') {
      return json({ data: { definitions: [{ agent_ref: 'abc123', name: 'Salgsassistent', installations: [] }] } })
    }
    if (url.endsWith('/space-policy')) return json({ error: { code: 'space_policy_admin_disabled', message: 'off' } }, 404)
    if (url.includes('/space-policy/events')) return json({ data: [] })
    return json({})
  })
  vi.stubGlobal('fetch', fetchMock)
  return fetchMock
}

beforeEach(() => {
  setSessionUser({ id: 'user_1', email: 'user@example.com', name: 'User', emailVerified: true })
  flush()
  markSessionOnboardingComplete({ id: 'org_1', name: 'Acme', role: 'admin' })
  flush()
})

afterEach(() => {
  cleanup()
  clearSession()
  vi.unstubAllGlobals()
})

describe('workspace settings: the organization agents section (D19)', () => {
  it('renders the agent list, with the agent named, on its own section', async () => {
    stubGateway()
    render(() => <VerevonWorkspaceSettingsPage section="org-agents" />)

    expect(await screen.findByText('Salgsassistent')).toBeTruthy()
    expect(screen.getByText('agent-abc123')).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Suspender' })).toBeTruthy()
  })

  it('is not under room policy any more: that section draws the policy alone', async () => {
    const fetchMock = stubGateway()
    render(() => <VerevonWorkspaceSettingsPage section="space-policy" />)

    expect(await screen.findByText('Rom-policy er ikke slått på for denne installasjonen.')).toBeTruthy()
    expect(screen.queryByText('agent-abc123')).toBeNull()
    expect(fetchMock.mock.calls.map((call) => String(call[0]))).not.toContain('/api/v1/orgs/org_1/agents')
  })
})
