// @vitest-environment jsdom

import { createRouter, memoryHistory } from '@solidjs/router'
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'

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

vi.mock('@/shared/session/session-store', () => ({
  getSession: () => ({ activeOrg: { id: 'org-1', name: 'Test', role: 'admin' } }),
}))

import { DashboardComposer } from './DashboardComposer'

const submitted: Array<Record<string, unknown>> = []

function ComposerPage() {
  const [message, setMessage] = createSignal('')
  return (
    <DashboardComposer
      message={message()}
      onMessageChange={setMessage}
      onSubmit={(payload) => {
        submitted.push(payload as unknown as Record<string, unknown>)
      }}
    />
  )
}

function renderComposer() {
  const TestRouter = createRouter({
    routes: [{ path: '/*all', component: ComposerPage }],
    history: memoryHistory(),
    explicitLinks: true,
  })
  return render(() => <TestRouter>{(props) => <>{props.children}</>}</TestRouter>)
}

afterEach(() => {
  submitted.length = 0
  vi.clearAllMocks()
})

describe('connected subscription model picker', () => {
  it('shows all plan models with the provider catalog and carries the selected connection route', async () => {
    mockListModels.mockResolvedValue([
      {
        id: 'gpt-6-astra',
        name: 'GPT 6 Astra Subscription',
        provider: 'openai-codex-subscription',
      },
      {
        id: 'gpt-5.6-luna',
        name: 'GPT 5.6 Luna Subscription',
        provider: 'openai-codex-subscription',
      },
    ])
    mockListSubscriptions.mockResolvedValue([{
      id: 'conn-subscription-1',
      providerKey: 'openai-codex-subscription',
      status: 'active',
    }])
    renderComposer()

    fireEvent.click(await screen.findByRole('button', { name: /choose ai model|velg ai-modell/i }))
    fireEvent.click(screen.getByText(/choose a model yourself|velg modell selv/i))
    expect(screen.getByRole('region', { name: /available ai models|tilgjengelige ai-modeller/i }).tabIndex).toBe(0)
    expect(await screen.findByText('GPT 6 Astra Subscription')).toBeTruthy()
    const choice = await screen.findByRole('button', { name: /gpt 5\.6 luna subscription/i })
    expect(choice.closest('details')).not.toBeNull()

    fireEvent.click(choice)
    const textbox = screen.getByRole('textbox')
    fireEvent.input(textbox, { target: { value: 'Use my subscription' } })
    flush()
    fireEvent.submit(textbox.closest('form')!)

    await waitFor(() => expect(submitted).toHaveLength(1))
    expect(submitted[0]).toMatchObject({
      model: 'gpt-5.6-luna',
      provider: 'openai-codex-subscription',
      subscriptionConnectionId: 'conn-subscription-1',
    })
  })
})
