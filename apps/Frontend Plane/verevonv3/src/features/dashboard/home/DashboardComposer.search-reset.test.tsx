// @vitest-environment jsdom

import { createRouter, memoryHistory } from '@solidjs/router'
import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { ModelInfo } from '@/shared/api/chat-client'

// The composer's model menu clears its search when it closes, so the next opening lists the whole
// catalogue again. Closing is the menu button, or choosing a model.

const { mockListModels, mockMeta } = vi.hoisted(() => ({
  mockListModels: vi.fn(),
  mockMeta: vi.fn(),
}))

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return {
    ...actual,
    listModels: mockListModels,
    getModelEntitlementMeta: mockMeta,
    listChatThreads: vi.fn().mockResolvedValue([]),
    saveChatThreadSnapshot: vi.fn().mockResolvedValue(null),
  }
})

vi.mock('@/shared/session/session-store', () => ({
  getSession: () => ({ activeOrg: { id: 'org-1', name: 'Test', role: 'member' } }),
}))

import { DashboardComposer } from './DashboardComposer'

const model = (id: string): ModelInfo => ({ id, name: id, provider: 'openai', modality: 'chat' })
const catalogue = ['gpt-mini', 'gpt-nano', 'gpt-large', 'gpt-vision', 'gpt-audio', 'gpt-turbo', 'gpt-omni'].map(model)

function ComposerPage() {
  const [message, setMessage] = createSignal('')
  return (
    <DashboardComposer
      appearance="chat"
      message={message()}
      onMessageChange={setMessage}
      onSubmit={() => undefined}
    />
  )
}

function renderComposer() {
  mockListModels.mockResolvedValue(catalogue)
  mockMeta.mockReturnValue(undefined)
  const TestRouter = createRouter({
    routes: [{ path: '/*all', component: ComposerPage }],
    history: memoryHistory(),
    explicitLinks: true,
  })
  return render(() => <TestRouter>{(props) => <>{props.children}</>}</TestRouter>)
}

async function searchField(): Promise<HTMLInputElement> {
  return (await screen.findByRole('searchbox', { name: /søk etter modell/i })) as HTMLInputElement
}

function searchValue(): string | undefined {
  return document.querySelector<HTMLInputElement>('input[type="search"]')?.value
}

afterEach(() => {
  cleanup()
  window.localStorage.clear()
  window.sessionStorage.clear()
  vi.clearAllMocks()
})

describe('composer model search resets with the menu', () => {
  it('lists the whole catalogue again after the menu is closed and reopened', async () => {
    renderComposer()
    const trigger = await screen.findByRole('button', { name: /velg ai-modell/i })

    fireEvent.click(trigger)
    fireEvent.input(await searchField(), { target: { value: 'claude' } })
    expect(await screen.findByText('Ingen modeller passer')).toBeTruthy()

    fireEvent.click(trigger)
    await waitFor(() => expect(trigger.getAttribute('aria-expanded')).toBe('false'))
    fireEvent.click(trigger)
    await waitFor(() => expect(searchValue()).toBe(''))
    expect(screen.queryByText('Ingen modeller passer')).toBeNull()
    expect(screen.getByText('gpt-nano')).toBeTruthy()
  })

  it('clears the query when a model is chosen', async () => {
    renderComposer()
    const trigger = await screen.findByRole('button', { name: /velg ai-modell/i })

    fireEvent.click(trigger)
    fireEvent.input(await searchField(), { target: { value: 'gpt-nano' } })
    await waitFor(() => expect(screen.queryByText('gpt-mini')).toBeNull())
    fireEvent.click(screen.getByText('gpt-nano'))
    await waitFor(() => expect(trigger.getAttribute('aria-expanded')).toBe('false'))

    fireEvent.click(trigger)
    await waitFor(() => expect(searchValue()).toBe(''))
    expect(screen.getByText('gpt-mini')).toBeTruthy()
  })
})
