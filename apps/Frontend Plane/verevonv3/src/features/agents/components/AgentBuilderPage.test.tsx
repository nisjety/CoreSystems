// @vitest-environment jsdom

import { createRouter, memoryHistory } from '@solidjs/router'
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import AgentBuilderPage, { diffInstructionLines, seedDraft, type AgentDraftState } from './AgentBuilderPage'

const http = vi.hoisted(() => ({ requestJson: vi.fn() }))
const chat = vi.hoisted(() => ({ listModels: vi.fn() }))
const composer = vi.hoisted(() => ({ loadComposerSettingsItems: vi.fn() }))

vi.mock('@/shared/api/http', () => ({ requestJson: http.requestJson }))
vi.mock('@/shared/api/chat-client', () => ({ listModels: chat.listModels }))
vi.mock('@/shared/api/composer-settings-client', () => ({
  loadComposerSettingsItems: composer.loadComposerSettingsItems,
}))

const draftState: AgentDraftState = {
  agent_ref: 'agent_1',
  name: 'Driftsassistent',
  published_version: 1,
  draft: null,
  current: { system_prompt: 'Svar kort.', model: 'Verevon Balance' },
}

const versions = [
  {
    version: 2,
    digest: `sha256:${'b'.repeat(64)}`,
    name: 'Driftsassistent',
    system_prompt: 'Svar kort.\nBruk punktlister.',
    model: 'Verevon Balance',
    tool_capability_ids: [],
    autonomy_cap: 'ask_first',
    published_by: 'user-1',
    published_at: 1_700_000_100_000,
  },
  {
    version: 1,
    digest: `sha256:${'a'.repeat(64)}`,
    name: 'Driftsassistent',
    system_prompt: 'Svar kort.',
    model: 'Verevon Balance',
    tool_capability_ids: [],
    autonomy_cap: 'ask_first',
    published_by: 'user-1',
    published_at: 1_700_000_000_000,
  },
]

// The gateway answers by path and method, the way the BFF routes do.
function routeRequests(overrides: { draft?: unknown; publishFails?: boolean } = {}) {
  http.requestJson.mockImplementation(async (path: string, init?: RequestInit) => {
    const method = init?.method ?? 'GET'
    if (path === '/api/v1/agents/agent_1/draft' && method === 'GET') return overrides.draft ?? draftState
    if (path === '/api/v1/agents/agent_1/draft' && method === 'PUT') return { draft: JSON.parse(String(init?.body)) }
    if (path === '/api/v1/agents/agent_1/publish') {
      if (overrides.publishFails) throw new Error('agent_publish_unavailable')
      return { version: 3, digest: `sha256:${'c'.repeat(64)}` }
    }
    if (path === '/api/v1/agents/agent_1/versions') return { versions }
    throw new Error(`unexpected ${method} ${path}`)
  })
}

function renderPage() {
  const TestRouter = createRouter({
    routes: [{ path: '/agents/:agentId/edit', component: AgentBuilderPage }],
    history: memoryHistory('/agents/agent_1/edit'),
    explicitLinks: true,
  })
  return render(() => <TestRouter>{(props) => <>{props.children}</>}</TestRouter>)
}

// No I18nProvider wraps these renders, so tr() resolves the Norwegian text (defaultLocale 'no').
describe('AgentBuilderPage', () => {
  beforeEach(() => {
    http.requestJson.mockReset()
    chat.listModels.mockReset()
    composer.loadComposerSettingsItems.mockReset()
    chat.listModels.mockResolvedValue([{ id: 'Verevon Balance', name: 'Balance', modality: 'chat' }])
    composer.loadComposerSettingsItems.mockResolvedValue([
      { id: 'cap.tool.ticket.create', name: 'Opprett sak' },
      // A free tool name is not a capability id, so the picker never offers it.
      { id: 'Send Email', name: 'Send e-post' },
    ])
  })

  it('seeds a first draft from the agent as it is now and offers only capability ids as tools', async () => {
    routeRequests()
    renderPage()

    expect(await screen.findByDisplayValue('Driftsassistent')).toBeTruthy()
    expect(screen.getByDisplayValue('Svar kort.')).toBeTruthy()
    expect(screen.getByText(/ikke noe lagret utkast/)).toBeTruthy()
    expect(screen.getByLabelText('Opprett sak')).toBeTruthy()
    expect(screen.queryByText('Send e-post')).toBeNull()
  })

  it('saves what is on screen before it publishes, then shows the new version', async () => {
    routeRequests()
    renderPage()

    const instructions = await screen.findByDisplayValue('Svar kort.')
    fireEvent.input(instructions, { target: { value: 'Svar kort og presist.' } })
    fireEvent.click(screen.getByLabelText('Opprett sak'))
    fireEvent.click(screen.getByRole('button', { name: 'Publiser' }))

    expect(await screen.findByText('Publisert som versjon 3.')).toBeTruthy()
    const calls = http.requestJson.mock.calls.map(([path, init]) => `${(init as RequestInit | undefined)?.method ?? 'GET'} ${path}`)
    const put = calls.indexOf('PUT /api/v1/agents/agent_1/draft')
    expect(put).toBeGreaterThan(-1)
    expect(calls.indexOf('POST /api/v1/agents/agent_1/publish')).toBeGreaterThan(put)
    const [, init] = http.requestJson.mock.calls.find(([, init]) => (init as RequestInit | undefined)?.method === 'PUT')!
    expect(JSON.parse(String((init as RequestInit).body))).toEqual({
      name: 'Driftsassistent',
      system_prompt: 'Svar kort og presist.',
      model: 'Verevon Balance',
      tool_capability_ids: ['cap.tool.ticket.create'],
      autonomy_cap: 'ask_first',
    })
  })

  it('says the current version is unchanged when a publish fails', async () => {
    routeRequests({ publishFails: true })
    renderPage()

    fireEvent.click(await screen.findByRole('button', { name: 'Publiser' }))
    expect(await screen.findByText(/Gjeldende versjon er uendret/)).toBeTruthy()
  })

  it('shows each version with the lines its instructions added', async () => {
    routeRequests()
    renderPage()

    expect(await screen.findByText('Versjon 2')).toBeTruthy()
    expect(screen.getByText('Versjon 1')).toBeTruthy()
    fireEvent.click(screen.getAllByRole('button', { name: 'Vis endringer' })[0]!)
    await waitFor(() => expect(screen.getByText('+ Bruk punktlister.')).toBeTruthy())
  })

  it('shows the refusal when the caller may not edit this agent', async () => {
    http.requestJson.mockRejectedValue(new Error('agent_builder_forbidden'))
    renderPage()

    await waitFor(() => expect(screen.getByRole('alert')).toBeTruthy())
    expect(screen.queryByRole('button', { name: 'Publiser' })).toBeNull()
  })
})

describe('diffInstructionLines', () => {
  it('marks removed and added lines around the lines both texts share', () => {
    expect(diffInstructionLines('a\nb\nc', 'a\nc\nd')).toEqual([
      { kind: 'same', text: 'a' },
      { kind: 'removed', text: 'b' },
      { kind: 'same', text: 'c' },
      { kind: 'added', text: 'd' },
    ])
  })

  it('treats a first version as all added', () => {
    expect(diffInstructionLines('', 'a\nb')).toEqual([
      { kind: 'added', text: 'a' },
      { kind: 'added', text: 'b' },
    ])
  })
})

describe('seedDraft', () => {
  it('prefers a saved draft over the current fields', () => {
    const draft = {
      name: 'Utkast',
      system_prompt: 'Nytt.',
      model: 'Verevon Balance',
      tool_capability_ids: ['cap.tool.ticket.create'],
      autonomy_cap: 'read_only' as const,
    }
    expect(seedDraft({ ...draftState, draft })).toEqual(draft)
    expect(seedDraft(draftState)).toMatchObject({ name: 'Driftsassistent', system_prompt: 'Svar kort.', autonomy_cap: 'ask_first' })
  })
})
