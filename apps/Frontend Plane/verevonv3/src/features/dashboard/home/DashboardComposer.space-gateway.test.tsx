// @vitest-environment jsdom

/**
 * A Space thread never takes the person's own gateway (DEC-V-R8). A Space's turns may be written under a customer gateway,
 * so with the BYO gateway switch on the "Your gateway" models are not offered in a Space, and a gateway choice the person
 * made before (remembered in the browser) is refused at submit, with the draft kept. The platform never takes the turn in
 * its place. Outside a Space the same choice is sent with its connection, as it was before.
 */

import { createRouter, memoryHistory } from '@solidjs/router'
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const { mockListModels, mockListCustomerGateways } = vi.hoisted(() => ({
  mockListModels: vi.fn(),
  mockListCustomerGateways: vi.fn(),
}))

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return {
    ...actual,
    listModels: mockListModels,
    listChatThreads: vi.fn().mockResolvedValue([]),
    saveChatThreadSnapshot: vi.fn().mockResolvedValue(null),
  }
})

vi.mock('@/shared/api/model-connections-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/model-connections-client')>()
  return { ...actual, listCustomerGateways: mockListCustomerGateways }
})

vi.mock('@/shared/api/chat-actions-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-actions-client')>()
  return { ...actual, loadSpecializedActions: vi.fn().mockResolvedValue(actual.BUILTIN_ACTIONS) }
})

vi.mock('@/shared/session/session-store', () => ({
  getSession: () => ({ activeOrg: { id: 'org-1', name: 'Test', role: 'admin' } }),
}))

import { DashboardComposer } from './DashboardComposer'

const catalogue = [{ id: 'gpt-5-codex', name: 'GPT-5 Codex', modality: 'chat', capabilities: [] }]

const gatewayConnection = {
  id: 'conn-gw-1',
  name: 'Egen gateway',
  baseUrl: 'https://gateway.example.test',
  status: 'active',
  modelIds: ['gpt-5-codex'],
}

const submitted: Array<Record<string, unknown>> = []

// The model choice the composer remembers for this organisation (`readAiModelSelection`): a model on the person's gateway.
const REMEMBERED_GATEWAY_CHOICE = {
  model: 'gpt-5-codex',
  label: 'gpt-5-codex',
  modelConnectionId: 'conn-gw-1',
}

function ComposerPage(props: { spaceScoped?: boolean }) {
  const [message, setMessage] = createSignal('Hva sendte Ada meg?')
  return (
    <DashboardComposer
      message={message()}
      onMessageChange={setMessage}
      spaceScoped={props.spaceScoped}
      onSubmit={(payload) => {
        submitted.push(payload as unknown as Record<string, unknown>)
      }}
    />
  )
}

function renderComposer(props: { spaceScoped?: boolean } = {}) {
  const TestRouter = createRouter({
    routes: [{ path: '/*all', component: () => <ComposerPage {...props} /> }],
    history: memoryHistory(),
    explicitLinks: true,
  })
  return render(() => <TestRouter>{(routerProps) => <>{routerProps.children}</>}</TestRouter>)
}

function submitComposerForm() {
  const form = screen.getByRole('textbox').closest('form')
  if (!form) throw new Error('composer form not found')
  fireEvent.submit(form)
}

/** Lets the catalogue and the gateway list settle, so the composer has read them before a send. */
async function settle() {
  await new Promise((resolve) => setTimeout(resolve, 20))
}

beforeEach(() => {
  mockListModels.mockReset().mockResolvedValue(catalogue)
  mockListCustomerGateways.mockReset().mockResolvedValue([gatewayConnection])
  window.localStorage.setItem('verevon.ai-model-selection.v1:org-1', JSON.stringify(REMEMBERED_GATEWAY_CHOICE))
})

afterEach(() => {
  submitted.length = 0
  window.localStorage.removeItem('verevon.ai-model-selection.v1:org-1')
  vi.unstubAllEnvs()
  vi.clearAllMocks()
})

/** Opens the model menu and its "Choose a model yourself" catalogue, where the "Your gateway" models are listed. */
async function openModelCatalogue() {
  fireEvent.click(screen.getByRole('button', { name: 'Velg AI-modell' }))
  fireEvent.click(await screen.findByText(/velg modell selv/i))
}

describe('composer on a Space thread with the BYO gateway switch on', () => {
  it('offers no "Your gateway" model in a Space', async () => {
    vi.stubEnv('VITE_BYO_GATEWAY', 'true')
    renderComposer({ spaceScoped: true })
    await settle()
    await openModelCatalogue()
    await settle()
    expect(screen.queryByText('Din gateway')).toBeNull()
  })

  it('offers the "Your gateway" models outside a Space, as before', async () => {
    vi.stubEnv('VITE_BYO_GATEWAY', 'true')
    renderComposer({ spaceScoped: false })
    await openModelCatalogue()
    await waitFor(() => expect(screen.getByText('Din gateway')).toBeTruthy())
  })

  it('refuses a turn on the person\'s own gateway in a Space, keeps the draft, and sends nothing', async () => {
    vi.stubEnv('VITE_BYO_GATEWAY', 'true')
    renderComposer({ spaceScoped: true })
    await settle()

    submitComposerForm()
    await waitFor(() => expect(screen.getByText(/kan ikke brukes i et delt Space/)).toBeTruthy())
    expect(submitted).toHaveLength(0)
    expect(screen.getByRole('textbox')).toHaveProperty('value', 'Hva sendte Ada meg?')
  })

  it('sends the same remembered choice with its connection outside a Space, as before', async () => {
    vi.stubEnv('VITE_BYO_GATEWAY', 'true')
    renderComposer({ spaceScoped: false })
    await settle()

    submitComposerForm()
    await waitFor(() => expect(submitted).toHaveLength(1))
    expect(submitted[0]).toMatchObject({ model: 'gpt-5-codex', modelConnectionId: 'conn-gw-1' })
  })
})
