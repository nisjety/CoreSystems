// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor, within } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import AccountSettingsPage from '@/features/settings/components/AccountSettingsPage'
import { clearSession, markSessionOnboardingComplete, setSessionUser } from '@/shared/session/session-store'

// WS17: the person's own personal agents and memory are reached from the account page. The workspace Settings route
// is admin-only (RequireSettingsAccess in App.tsx; compliance sees the Trust Center only), so before these sections were mounted here an ordinary member
// could not create an agent, choose a primary agent, edit an agent's access, share a memory to a room or restore a
// withheld note at all. The page is rendered for a plain `member` on purpose.

// The access dialog and the share dialog belong to the spaces area and have their own tests.
vi.mock('@/features/spaces/components/SpaceAgentAccessDialog', () => ({
  SpaceAgentAccessDialog: (props: { agentRef: string | undefined }) => (
    <p data-testid="access-dialog">{props.agentRef ?? 'closed'}</p>
  ),
}))

const FEATURES = '/api/v1/spaces/features'
const AGENTS = '/api/v1/me/agents'
const PRIMARY = '/api/v1/me/primary-agent'

const AGENT_ROW = {
  agent_ref: 'agt_fake_1',
  subject_id: 'subj_fake_1',
  name: 'Min agent',
  status: 'active',
  role: 'owner',
  visibility: 'owner',
  add_policy: 'owner_only',
  revision: 1,
  rooms: [],
}
const MEMORY_ROW = {
  memory_id: 'mem_ordinary',
  topic: 'USER',
  content: 'Prefers dark mode',
  updated_at: '2026-10-02T10:00:00Z',
  provenance: 'stated',
}
const MEMORY_PROFILE = {
  profile: {
    content: '- Prefers dark mode',
    source_memory_ids: ['mem_ordinary'],
    origin: 'built',
    revision: 4,
    updated_at: '2026-11-03T10:00:00Z',
    sources_deleted_since_edit: 0,
  },
  skip_reason: '',
}
const MEMORY_ON = { state: 'on', reason: 'on', use_allowed: true, generation_allowed: true, hidden_rows: 0 }

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), { headers: { 'Content-Type': 'application/json' }, status })
}

function signInAsMember() {
  setSessionUser({ id: 'member_1', email: 'member@example.com', name: 'Member', emailVerified: true })
  flush()
  markSessionOnboardingComplete({ id: 'org_1', name: 'Acme', role: 'member' })
  flush()
}

/** The gateway as a member sees it. `features` is what `GET /api/v1/spaces/features` states. */
function stubGateway(features: Record<string, boolean>) {
  let primarySubjectId: string | null = null
  const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input)
    const method = init?.method ?? 'GET'
    if (url === FEATURES) return json({ data: { features } })
    if (url === AGENTS && method === 'GET') {
      return json({ data: { agents: [AGENT_ROW], primary_subject_id: primarySubjectId } })
    }
    if (url === PRIMARY && method === 'PUT') {
      primarySubjectId = (JSON.parse(String(init?.body)) as { agent_ref: string | null }).agent_ref ? 'subj_fake_1' : null
      return json({ data: { primary_subject_id: primarySubjectId } })
    }
    if (url === '/api/v1/memory/profile') return json(MEMORY_PROFILE)
    if (url === '/api/v1/memory/controls') return json(MEMORY_ON)
    if (url === '/api/v1/memory') return json({ memories: [MEMORY_ROW], degraded: false, degradation_reason: '' })
    return json({})
  })
  vi.stubGlobal('fetch', fetchMock)
  return fetchMock
}

function callsTo(fetchMock: ReturnType<typeof stubGateway>, method: string, url: string) {
  return fetchMock.mock.calls.filter(
    ([input, init]) => String(input) === url && ((init as RequestInit | undefined)?.method ?? 'GET') === method,
  )
}

beforeEach(signInAsMember)

afterEach(() => {
  cleanup()
  clearSession()
  vi.unstubAllGlobals()
})

describe('the account page for an ordinary member (WS17)', () => {
  it('shows their personal agents, with the controls to choose the primary agent and edit access', async () => {
    const fetchMock = stubGateway({ personal_agents: true })
    render(() => <AccountSettingsPage />)

    // The section appears once the gateway has said the feature is on; the agent row once the list has arrived.
    expect(await screen.findByText('Min agent')).toBeTruthy()
    const section = document.getElementById('personal-agents') as HTMLElement
    expect(section).not.toBeNull()
    expect(within(section).getByRole('heading', { name: 'Personlige agenter' })).toBeTruthy()
    // Creating an agent is there too.
    expect(within(section).getByRole('button', { name: 'Opprett agent' })).toBeTruthy()

    // Set as primary writes the choice and the list shows it.
    fireEvent.click(within(section).getByRole('button', { name: 'Sett som primær' }))
    await waitFor(() => expect(callsTo(fetchMock, 'PUT', PRIMARY)).toHaveLength(1))
    expect(JSON.parse(String(callsTo(fetchMock, 'PUT', PRIMARY)[0]?.[1]?.body))).toEqual({ agent_ref: 'agt_fake_1' })
    expect(await within(section).findByRole('button', { name: 'Fjern som primær' })).toBeTruthy()

    // Access opens the agent's access dialog.
    expect(screen.getByTestId('access-dialog').textContent).toBe('closed')
    fireEvent.click(within(section).getByRole('button', { name: 'Tilgang' }))
    flush()
    expect(screen.getByTestId('access-dialog').textContent).toBe('agt_fake_1')
  })

  it('shows their memory, with Share to a room offered while the gateway says declassify is on', async () => {
    stubGateway({ personal_agents: false, memory_declassify: true })
    render(() => <AccountSettingsPage />)

    expect(await screen.findByText('Prefers dark mode')).toBeTruthy()
    const section = document.getElementById('memory') as HTMLElement
    expect(within(section).getByRole('heading', { name: 'Minne' })).toBeTruthy()
    expect(await within(section).findByRole('button', { name: 'Del i et rom: Prefers dark mode' })).toBeTruthy()
  })

  // The memory card has a save button of its own. On this page the account's "Lagre profil" is the one that saves the
  // name and preferences, so two buttons with that label would not say which one they save.
  it('keeps the memory profile save button apart from the page save button', async () => {
    stubGateway({ personal_agents: false })
    render(() => <AccountSettingsPage />)

    const section = (await screen.findByText('Prefers dark mode')).closest('#memory') as HTMLElement
    expect(await within(section).findByRole('button', { name: 'Lagre minneprofil' })).toBeTruthy()
    expect(screen.getAllByRole('button', { name: 'Lagre profil' })).toHaveLength(1)
  })

  it('leaves out the personal agents section, and its gap, while the gateway does not report the feature', async () => {
    const fetchMock = stubGateway({ personal_agents: false })
    render(() => <AccountSettingsPage />)

    await waitFor(() => expect(callsTo(fetchMock, 'GET', FEATURES).length).toBeGreaterThan(0))
    // The memory section still answers: it is not behind the agents switch.
    expect(await screen.findByText('Prefers dark mode')).toBeTruthy()
    expect(document.getElementById('personal-agents')).toBeNull()
    expect(document.getElementById('personal-agent-name')).toBeNull()
    expect(callsTo(fetchMock, 'GET', AGENTS)).toHaveLength(0)
    expect(document.getElementById('memory')).not.toBeNull()
  })
})
