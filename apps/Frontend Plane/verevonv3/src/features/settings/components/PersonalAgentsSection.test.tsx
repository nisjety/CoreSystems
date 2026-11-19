// @vitest-environment jsdom

import { cleanup, render, screen, waitFor, fireEvent } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { PersonalAgentsSection } from '@/features/settings/components/PersonalAgentsSection'
import { I18nProvider } from '@/shared/i18n'
import { clearSession, markSessionOnboardingComplete, setSessionUser } from '@/shared/session/session-store'

// The access dialog belongs to the spaces area and is not under test here. It is replaced by an empty
// stub, so this suite exercises only the section's own gate and create flow.
vi.mock('@/features/spaces/components/SpaceAgentAccessDialog', () => ({
  SpaceAgentAccessDialog: () => null,
}))

// WS17-S5: the caller's own agents. The whole section is hidden unless the gateway reports
// features.personal_agents. Each create attempt carries one client_request_id: a retry of the same
// request sends the same id, and an edited request (or a new attempt after success) gets a new one.

const FEATURES = '/api/v1/spaces/features'
const AGENTS = '/api/v1/me/agents'
const EMPTY_LIST = 'Du har ingen personlige agenter ennå.'
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

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

function signIn() {
  setSessionUser({ id: 'member_1', email: 'member@example.com', name: 'Member', emailVerified: true })
  flush()
  markSessionOnboardingComplete({ id: 'org_1', name: 'Acme', role: 'member' })
  flush()
}

type Handler = (url: string, init: RequestInit | undefined) => Response | Promise<Response> | undefined

function stubFetch(handler: Handler) {
  const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => (
    (await handler(String(input), init)) ?? new Response(null, { status: 404 })
  ))
  vi.stubGlobal('fetch', fetchMock)
  return fetchMock
}

function calls(fetchMock: ReturnType<typeof vi.fn>): string[] {
  return fetchMock.mock.calls.map((call) => String(call[0]))
}

/** The JSON bodies of every create request, in order. */
function createBodies(fetchMock: ReturnType<typeof vi.fn>): Array<Record<string, unknown>> {
  return fetchMock.mock.calls
    .filter((call) => String(call[0]) === AGENTS && (call[1] as RequestInit | undefined)?.method === 'POST')
    .map((call) => JSON.parse(String((call[1] as RequestInit).body)) as Record<string, unknown>)
}

function renderSection() {
  return render(() => (
    <I18nProvider>
      <PersonalAgentsSection />
    </I18nProvider>
  ))
}

function typeName(value: string) {
  fireEvent.input(document.getElementById('personal-agent-name') as HTMLInputElement, { target: { value } })
  flush()
}

function submitCreate() {
  const form = (document.getElementById('personal-agent-name') as HTMLInputElement).closest('form') as HTMLFormElement
  fireEvent.submit(form)
  flush()
}

afterEach(() => {
  cleanup()
  clearSession()
  vi.unstubAllGlobals()
})

describe('PersonalAgentsSection', () => {
  it('is hidden when features.personal_agents is false and asks for no agents', async () => {
    signIn()
    const fetchMock = stubFetch((url) => (
      url === FEATURES ? json({ data: { features: { personal_agents: false } } }) : undefined
    ))
    renderSection()

    await waitFor(() => expect(calls(fetchMock)).toContain(FEATURES))
    await new Promise((resolve) => setTimeout(resolve, 20))
    expect(screen.queryByText('Personlige agenter')).toBeNull()
    expect(document.getElementById('personal-agent-name')).toBeNull()
    expect(calls(fetchMock)).not.toContain(AGENTS)
  })

  it('is shown when features.personal_agents is true, with the agents the caller owns', async () => {
    signIn()
    stubFetch((url) => {
      if (url === FEATURES) return json({ data: { features: { personal_agents: true } } })
      if (url === AGENTS) return json({ data: { agents: [AGENT_ROW], primary_subject_id: null } })
      return undefined
    })
    renderSection()

    expect(await screen.findByText('Min agent')).toBeTruthy()
    expect(screen.getByText('Personlige agenter')).toBeTruthy()
  })

  it('sends one client_request_id per create attempt: a retry keeps it, an edited request or a new attempt gets a new one', async () => {
    signIn()
    let posts = 0
    const fetchMock = stubFetch((url, init) => {
      if (url === FEATURES) return json({ data: { features: { personal_agents: true } } })
      if (url === AGENTS && init?.method === 'POST') {
        posts += 1
        // The first two attempts fail on the network; the third and fourth succeed.
        if (posts <= 2) throw new TypeError('Failed to fetch')
        return json({ data: { agent_ref: `agt_fake_${posts}`, subject_id: `subj_fake_${posts}`, revision: 1 } }, 201)
      }
      if (url === AGENTS) return json({ data: { agents: [], primary_subject_id: null } })
      return undefined
    })
    renderSection()
    expect(await screen.findByText(EMPTY_LIST)).toBeTruthy()
    const ids = () => createBodies(fetchMock).map((body) => body.client_request_id)

    typeName('Ny agent')
    submitCreate()
    await waitFor(() => expect(ids()).toHaveLength(1))
    expect(createBodies(fetchMock)[0]?.name).toBe('Ny agent')
    expect(await screen.findByRole('alert')).toBeTruthy()

    // The same request again is the same attempt, so it carries the same id.
    submitCreate()
    await waitFor(() => expect(ids()).toHaveLength(2))
    expect(await screen.findByRole('alert')).toBeTruthy()
    expect(ids()[1]).toBe(ids()[0])

    // An edited request is a new attempt with a new id. It succeeds, so wait for the create to finish.
    typeName('Ny agent to')
    submitCreate()
    await waitFor(() => expect(ids()).toHaveLength(3))
    await waitFor(() => expect(screen.queryByText(/Oppretter/)).toBeNull())
    expect(ids()[2]).not.toBe(ids()[0])

    // After a success the next create is a new attempt too.
    typeName('Tredje agent')
    submitCreate()
    await waitFor(() => expect(ids()).toHaveLength(4))
    expect(ids()[3]).not.toBe(ids()[2])
  })
})

// WS17: an agent's owner takes it out of one room from here. The room card offers that to the room's owners and managers
// only, so an owner who is an editor in the room, or who has left it, had no way to do it. Control decides each call.
describe('PersonalAgentsSection: taking an agent out of one room', () => {
  const SPACES = '/api/v1/spaces'
  const WITHDRAW_ONE = '/api/v1/agents/agt_fake_1/rooms/space-1'
  const ROOM_ROWS = [
    { space_ref: 'space-1', state: 'active' },
    { space_ref: 'space-2', state: 'paused' },
  ]

  /** One agent in two rooms. The owner is a member of `space-1` only. `withdraw` answers the DELETE. */
  function stubRooms(withdraw: () => Response) {
    let rooms = ROOM_ROWS
    const fetchMock = stubFetch((url, init) => {
      if (url === FEATURES) return json({ data: { features: { personal_agents: true } } })
      if (url === AGENTS) return json({ data: { agents: [{ ...AGENT_ROW, rooms }], primary_subject_id: null } })
      if (url === SPACES) {
        return json({ data: { spaces: [{ space_ref: 'space-1', name: 'Leveranse', kind: 'room', lifecycle: 'active' }] } })
      }
      if (url === WITHDRAW_ONE && init?.method === 'DELETE') {
        const answer = withdraw()
        if (answer.ok) rooms = ROOM_ROWS.filter((room) => room.space_ref !== 'space-1')
        return answer
      }
      return undefined
    })
    return fetchMock
  }

  async function armWithdraw() {
    const buttons = await screen.findAllByRole('button', { name: 'Ta ut av rommet' })
    fireEvent.click(buttons[0] as HTMLElement)
    flush()
    return screen.getByRole('button', { name: /Bekreft at Min agent tas ut av Leveranse/ })
  }

  it('lists each room by name with its state, and a room the owner is not in without a name', async () => {
    signIn()
    stubRooms(() => json({ data: {} }))
    renderSection()

    expect(await screen.findByText('Leveranse')).toBeTruthy()
    expect(screen.getByText('Aktiv')).toBeTruthy()
    expect(screen.getByText('Et rom du ikke er medlem av')).toBeTruthy()
    expect(screen.getByText('På pause')).toBeTruthy()
    expect(screen.getAllByRole('button', { name: 'Ta ut av rommet' })).toHaveLength(2)
  })

  it('takes the agent out of that one room after a confirmation, and the room is gone from the list', async () => {
    signIn()
    const fetchMock = stubRooms(() => json({ data: { agent_ref: 'agt_fake_1', space_ref: 'space-1', state: 'withdrawn', changed: true } }))
    renderSection()

    // The first press only asks; nothing is sent until it is confirmed.
    const confirm = await armWithdraw()
    expect(calls(fetchMock)).not.toContain(WITHDRAW_ONE)
    expect(screen.getByText('Du beholder agenten og kan legge den til i rommet igjen senere.')).toBeTruthy()

    fireEvent.click(confirm)
    flush()

    await waitFor(() => expect(screen.queryByText('Leveranse')).toBeNull())
    const deletes = fetchMock.mock.calls.filter(
      (call) => String(call[0]) === WITHDRAW_ONE && (call[1] as RequestInit | undefined)?.method === 'DELETE',
    )
    expect(deletes).toHaveLength(1)
    // The other room is untouched.
    expect(screen.getByText('Et rom du ikke er medlem av')).toBeTruthy()
  })

  it('says it failed, and keeps the room listed, when Control refuses the withdrawal', async () => {
    signIn()
    stubRooms(() => json({ error: { code: 'room_role_forbidden', message: 'no' } }, 403))
    renderSection()

    fireEvent.click(await armWithdraw())
    flush()

    expect((await screen.findByRole('alert')).textContent).toBeTruthy()
    expect(screen.getByText('Leveranse')).toBeTruthy()
  })
})
