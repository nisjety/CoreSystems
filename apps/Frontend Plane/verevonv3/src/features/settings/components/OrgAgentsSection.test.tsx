// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { OrgAgentsSection } from '@/features/settings/components/OrgAgentsSection'
import { clearSession, markSessionOnboardingComplete, setSessionUser } from '@/shared/session/session-store'

// W3-DEC-ORGAGENT (D19): the org-admin section lists the org-visible agents and suspends, reactivates or retires one.
// Control decides every change; the section sends only the revision it read, and shows the server's refusal.

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

const LIST = '/api/v1/orgs/org_1/agents'
const ACTIVE = { agent_subject_id: 'agt_1', visibility: 'org', add_policy: 'managers', lifecycle: 'active', revision: 3 }
const SUSPENDED = { ...ACTIVE, agent_subject_id: 'agt_2', lifecycle: 'suspended', suspended_reason: 'manual', revision: 5 }

const CHANGED = (lifecycle: string, revision: number) => ({
  agent_subject_id: 'agt_1',
  lifecycle,
  revision,
  changed: true,
  rooms_changed: 2,
})

/** Routes the section's reads and writes; a body is parsed so a test can assert exactly what was sent. */
function stubOrgAgents(
  agents: unknown[] = [ACTIVE],
  overrides: Record<string, (body: unknown) => Response> = {},
) {
  const fetchMock = vi.fn(async (input: unknown, init?: RequestInit) => {
    const key = `${init?.method ?? 'GET'} ${String(input)}`
    const body = typeof init?.body === 'string' ? JSON.parse(init.body) : undefined
    const override = overrides[key]
    if (override) return override(body)
    if (key === `GET ${LIST}`) return jsonResponse({ data: { agents } })
    return jsonResponse({ error: { code: 'not_found', message: 'no' } }, 404)
  })
  vi.stubGlobal('fetch', fetchMock)
  return fetchMock
}

function callsTo(fetchMock: ReturnType<typeof stubOrgAgents>, method: string, path: string) {
  return fetchMock.mock.calls.filter(
    ([input, init]) => String(input) === path && ((init as RequestInit | undefined)?.method ?? 'GET') === method,
  )
}

function signIn(role: 'owner' | 'admin' | 'member') {
  // Solid 2 applies store writes on flush(), so each write is flushed before the next reads it.
  setSessionUser({ id: 'user_1', email: 'user@example.com', name: 'User', emailVerified: true })
  flush()
  markSessionOnboardingComplete({ id: 'org_1', name: 'Acme', role })
  flush()
}

afterEach(() => {
  cleanup()
  clearSession()
  flush()
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
})

describe('OrgAgentsSection visibility (W3-DEC-ORGAGENT)', () => {
  it('lists the org-visible agents with their lifecycle for an org admin', async () => {
    signIn('admin')
    stubOrgAgents([ACTIVE, SUSPENDED])

    render(() => <OrgAgentsSection />)

    expect(await screen.findByText('agt_1')).toBeTruthy()
    expect(screen.getByText('Aktiv')).toBeTruthy()
    expect(screen.getByText('agt_2')).toBeTruthy()
    expect(screen.getByText('Suspendert')).toBeTruthy()
  })

  it('shows nothing and sends no request to a member', async () => {
    signIn('member')
    const fetchMock = stubOrgAgents()

    render(() => <OrgAgentsSection />)
    flush()

    expect(screen.queryByText('Organisasjonens agenter')).toBeNull()
    expect(callsTo(fetchMock, 'GET', LIST)).toHaveLength(0)
  })
})

describe('OrgAgentsSection changes (W3-DEC-ORGAGENT)', () => {
  it('suspends an active agent with the revision it read, and says what happened', async () => {
    signIn('admin')
    const fetchMock = stubOrgAgents([ACTIVE], {
      'PUT /api/v1/orgs/org_1/agents/agt_1/suspend': () => jsonResponse({ data: CHANGED('suspended', 4) }),
    })

    render(() => <OrgAgentsSection />)
    fireEvent.click(await screen.findByRole('button', { name: 'Suspender' }))
    flush()

    await waitFor(() => {
      expect(screen.getByText('Agenten er suspendert og fjernet fra alle rom.')).toBeTruthy()
    })
    const [, init] = callsTo(fetchMock, 'PUT', '/api/v1/orgs/org_1/agents/agt_1/suspend')[0] as [string, RequestInit]
    expect(JSON.parse(String(init.body))).toEqual({ expected_revision: 3 })
  })

  it('reactivates a suspended agent and tells the rooms to add it again', async () => {
    signIn('admin')
    const fetchMock = stubOrgAgents([SUSPENDED], {
      'PUT /api/v1/orgs/org_1/agents/agt_2/reactivate': () => jsonResponse({ data: { ...CHANGED('active', 6), agent_subject_id: 'agt_2' } }),
    })

    render(() => <OrgAgentsSection />)
    fireEvent.click(await screen.findByRole('button', { name: 'Aktiver igjen' }))
    flush()

    await waitFor(() => {
      expect(screen.getByText('Agenten er aktiv igjen. Rom må legge den til på nytt.')).toBeTruthy()
    })
    const [, init] = callsTo(fetchMock, 'PUT', '/api/v1/orgs/org_1/agents/agt_2/reactivate')[0] as [string, RequestInit]
    expect(JSON.parse(String(init.body))).toEqual({ expected_revision: 5 })
  })

  it('retires only after a confirming click, and sends the revision with DELETE', async () => {
    signIn('admin')
    const fetchMock = stubOrgAgents([ACTIVE], {
      'DELETE /api/v1/orgs/org_1/agents/agt_1': () => jsonResponse({ data: CHANGED('retired', 4) }),
    })

    render(() => <OrgAgentsSection />)
    fireEvent.click(await screen.findByRole('button', { name: 'Avslutt' }))
    flush()
    expect(callsTo(fetchMock, 'DELETE', '/api/v1/orgs/org_1/agents/agt_1')).toHaveLength(0)

    fireEvent.click(await screen.findByRole('button', { name: 'Bekreft avslutning av agenten' }))
    flush()

    await waitFor(() => {
      expect(screen.getByText('Agenten er avsluttet og fjernet fra alle rom. Den beholdes for revisjon.')).toBeTruthy()
    })
    const [, init] = callsTo(fetchMock, 'DELETE', '/api/v1/orgs/org_1/agents/agt_1')[0] as [string, RequestInit]
    expect(JSON.parse(String(init.body))).toEqual({ expected_revision: 3 })
  })

  it('reloads the list and asks for another try when the agent changed under the admin', async () => {
    signIn('admin')
    const fetchMock = stubOrgAgents([ACTIVE], {
      'PUT /api/v1/orgs/org_1/agents/agt_1/suspend': () =>
        jsonResponse({ error: { code: 'agent_state_changed', message: 'stale', details: { current_revision: 4 } } }, 409),
    })

    render(() => <OrgAgentsSection />)
    fireEvent.click(await screen.findByRole('button', { name: 'Suspender' }))
    flush()

    expect(
      await screen.findByText('Agenten er endret av noen andre. Listen er oppdatert; sjekk den og prøv igjen.'),
    ).toBeTruthy()
    await waitFor(() => {
      expect(callsTo(fetchMock, 'GET', LIST).length).toBeGreaterThanOrEqual(2)
    })
  })

  it('says so when the change was already in place', async () => {
    signIn('admin')
    stubOrgAgents([ACTIVE], {
      'PUT /api/v1/orgs/org_1/agents/agt_1/suspend': () =>
        jsonResponse({ data: { ...CHANGED('active', 3), changed: false, rooms_changed: 0 } }),
    })

    render(() => <OrgAgentsSection />)
    fireEvent.click(await screen.findByRole('button', { name: 'Suspender' }))
    flush()

    expect(await screen.findByText('Ingen endring. Agenten har allerede denne tilstanden.')).toBeTruthy()
  })
})
