// @vitest-environment jsdom

/**
 * A Space thread never takes the ChatGPT subscription route while the BYO gateway switch is on (DEC-V-R8). A Space's turns
 * may be written under one member's subscription, so the subscription models are not offered in a Space, and a subscription
 * choice the person made before (remembered in the browser) is refused at submit, with the draft kept. The platform never
 * takes the turn in its place. Outside a Space, and with the switch off, the same choice is sent with its connection, as it
 * was before.
 */

import { createRouter, memoryHistory } from '@solidjs/router'
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const { mockListModels, mockListSubscriptions } = vi.hoisted(() => ({
  mockListModels: vi.fn(),
  mockListSubscriptions: vi.fn(),
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

vi.mock('@/shared/api/chatgpt-subscription-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chatgpt-subscription-client')>()
  return { ...actual, listChatGptSubscriptions: mockListSubscriptions }
})

vi.mock('@/shared/api/model-connections-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/model-connections-client')>()
  return { ...actual, listCustomerGateways: vi.fn().mockResolvedValue([]) }
})

vi.mock('@/shared/api/chat-actions-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-actions-client')>()
  return { ...actual, loadSpecializedActions: vi.fn().mockResolvedValue(actual.BUILTIN_ACTIONS) }
})

vi.mock('@/shared/session/session-store', () => ({
  getSession: () => ({ activeOrg: { id: 'org-1', name: 'Test', role: 'admin' } }),
}))

import { DashboardComposer } from './DashboardComposer'

const catalogue = [{ id: 'gpt-5.6-terra', name: 'GPT 5.6 Terra Subscription', provider: 'openai-codex-subscription' }]

const submitted: Array<Record<string, unknown>> = []

// The model choice the composer remembers for this organisation (`readAiModelSelection`): a ChatGPT subscription model.
const REMEMBERED_SUBSCRIPTION_CHOICE = {
  model: 'gpt-5.6-terra',
  label: 'GPT 5.6 Terra Subscription',
  provider: 'openai-codex-subscription',
  subscriptionConnectionId: 'conn-sub-1',
}

const DRAFT = 'Hva sendte Ada meg?'

function ComposerPage(props: { spaceScoped?: boolean }) {
  const [message, setMessage] = createSignal(DRAFT)
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

/** Lets the catalogue and the subscription lookup settle, so the composer has read them before a send. */
async function settle() {
  await waitFor(() => expect(mockListSubscriptions).toHaveBeenCalled())
  await new Promise((resolve) => setTimeout(resolve, 20))
}

/** Opens the model menu and its "Choose a model yourself" catalogue, where the subscription group is listed. */
async function openModelCatalogue() {
  fireEvent.click(screen.getByRole('button', { name: 'Velg AI-modell' }))
  fireEvent.click(await screen.findByText(/velg modell selv/i))
}

beforeEach(() => {
  mockListModels.mockReset().mockResolvedValue(catalogue)
  mockListSubscriptions.mockReset().mockResolvedValue([
    { id: 'conn-sub-1', providerKey: 'openai-codex-subscription', status: 'active' },
  ])
  window.localStorage.setItem('verevon.ai-model-selection.v1:org-1', JSON.stringify(REMEMBERED_SUBSCRIPTION_CHOICE))
})

afterEach(() => {
  submitted.length = 0
  window.localStorage.removeItem('verevon.ai-model-selection.v1:org-1')
  vi.unstubAllEnvs()
  vi.clearAllMocks()
})

describe('composer on a Space thread with the BYO gateway switch on', () => {
  it('offers no subscription model in a Space', async () => {
    vi.stubEnv('VITE_BYO_GATEWAY', 'true')
    renderComposer({ spaceScoped: true })
    await settle()
    await openModelCatalogue()
    await settle()
    expect(screen.queryByText('Subscription')).toBeNull()
  })

  it('offers the subscription models outside a Space, as before', async () => {
    vi.stubEnv('VITE_BYO_GATEWAY', 'true')
    renderComposer({ spaceScoped: false })
    await settle()
    await openModelCatalogue()
    expect(await screen.findByText('Subscription')).toBeTruthy()
  })

  it('refuses a turn on the subscription route in a Space, keeps the draft, and sends nothing', async () => {
    vi.stubEnv('VITE_BYO_GATEWAY', 'true')
    renderComposer({ spaceScoped: true })
    await settle()

    submitComposerForm()
    await waitFor(() => expect(screen.getByText(/kan ikke brukes i et delt Space/)).toBeTruthy())
    expect(submitted).toHaveLength(0)
    expect(screen.getByRole('textbox')).toHaveProperty('value', DRAFT)
  })

  it('sends the same remembered choice with its connection outside a Space, as before', async () => {
    vi.stubEnv('VITE_BYO_GATEWAY', 'true')
    renderComposer({ spaceScoped: false })
    await settle()

    submitComposerForm()
    await waitFor(() => expect(submitted).toHaveLength(1))
    expect(submitted[0]).toMatchObject({
      model: 'gpt-5.6-terra',
      provider: 'openai-codex-subscription',
      subscriptionConnectionId: 'conn-sub-1',
    })
  })
})

describe('composer on a Space thread with the BYO gateway switch off', () => {
  it('offers the subscription models in a Space, as before', async () => {
    vi.stubEnv('VITE_BYO_GATEWAY', '')
    renderComposer({ spaceScoped: true })
    await settle()
    await openModelCatalogue()
    expect(await screen.findByText('Subscription')).toBeTruthy()
  })

  it('sends the remembered subscription choice in a Space, as before', async () => {
    vi.stubEnv('VITE_BYO_GATEWAY', '')
    renderComposer({ spaceScoped: true })
    await settle()

    submitComposerForm()
    await waitFor(() => expect(submitted).toHaveLength(1))
    expect(submitted[0]).toMatchObject({
      model: 'gpt-5.6-terra',
      provider: 'openai-codex-subscription',
      subscriptionConnectionId: 'conn-sub-1',
    })
  })
})
