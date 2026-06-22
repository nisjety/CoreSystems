// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from '@tanstack/solid-query'
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import type { JSX } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { ChatbotStudio } from '@/features/agents/components/ChatbotStudio'
import { AgentsProvider } from '@/features/agents/lib/use-agent-selection'

function renderWithProviders(component: () => JSX.Element, path: string) {
  window.history.pushState(null, '', path)
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
    },
  })

  return render(() => (
    <QueryClientProvider client={queryClient}>
      <AgentsProvider>
        {component()}
      </AgentsProvider>
    </QueryClientProvider>
  ))
}

function stubRuntimeFetch() {
  const fetchMock = vi.fn(async () => new Response(JSON.stringify({
    data: {
      support: {
        configured: true,
        connected: true,
        agents: 4,
        groups: 3,
        macros: 8,
        message: 'Support runtime connected.',
      },
    },
  }), {
    headers: { 'Content-Type': 'application/json' },
    status: 200,
  }))
  vi.stubGlobal('fetch', fetchMock)
  return fetchMock
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('ChatbotStudio', () => {
  it('renders the dedicated playground surface and reads runtime status from the gateway', async () => {
    const fetchMock = stubRuntimeFetch()

    renderWithProviders(() => <ChatbotStudio />, '/agents?agent=chatbot')

    expect(screen.getByRole('heading', { name: 'Playground' })).toBeTruthy()
    expect(screen.getByRole('combobox', { name: 'Model' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Select update subscription add-on' })).toBeTruthy()
    expect(screen.getByText('1 Tool Enabled')).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: 'Clear' }))
    expect(screen.getByText('Tool ready to enable')).toBeTruthy()
    expect(screen.queryByText('Remove selected tool')).toBeNull()

    await waitFor(() => expect(screen.getByText('Live support actions connected')).toBeTruthy())

    const fetchCalls = fetchMock.mock.calls as unknown as Array<[unknown, { credentials?: RequestCredentials } | undefined]>
    expect(fetchCalls.some(([url, init]) =>
      String(url).endsWith('/api/v1/agents/chatbot/runtime') &&
      init?.credentials === 'include',
    )).toBe(true)
  })

  it('renders the chatbot section selected by the shared URL state', () => {
    stubRuntimeFetch()

    renderWithProviders(() => <ChatbotStudio />, '/agents?agent=chatbot&view=analytics')

    expect(screen.getByRole('heading', { name: 'Analytics' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Chat count' })).toBeTruthy()
    expect(screen.queryByRole('heading', { name: 'Playground' })).toBeNull()
  })
})

// Phase 4 agents-studio honesty sweep: every not-yet-wired studio surface must
// carry a "Design preview" badge AND have its action-implying controls disabled,
// so nothing implies a backend that does not exist yet.
describe('ChatbotStudio honesty sweep (Phase 4)', () => {
  const surfaces: Array<{ view: string; heading: RegExp; deadAction: RegExp }> = [
    { view: 'analytics', heading: /^analytics$/i, deadAction: /live event window/i },
    { view: 'insights', heading: /^insights$/i, deadAction: /live event window/i },
    { view: 'actions', heading: /^tools$/i, deadAction: /create tool/i },
    { view: 'integrations', heading: /^integrations$/i, deadAction: /add integration/i },
    { view: 'leads', heading: /^leads$/i, deadAction: /export/i },
  ]

  for (const surface of surfaces) {
    it(`labels the ${surface.view} surface as a design preview and disables its primary action`, () => {
      stubRuntimeFetch()

      renderWithProviders(() => <ChatbotStudio />, `/agents?agent=chatbot&view=${surface.view}`)

      expect(screen.getByRole('heading', { name: surface.heading, level: 1 })).toBeTruthy()
      expect(screen.getAllByText('Design preview').length).toBeGreaterThan(0)

      const action = screen.getAllByRole('button', { name: surface.deadAction })[0] as HTMLButtonElement
      expect(action.disabled).toBe(true)
    })
  }

  it('labels the install and chat-logs surfaces and disables their controls', () => {
    stubRuntimeFetch()

    renderWithProviders(() => <ChatbotStudio />, '/agents?agent=chatbot&view=install')
    expect(screen.getAllByText('Design preview').length).toBeGreaterThan(0)
    expect((screen.getByRole('button', { name: 'Manage' }) as HTMLButtonElement).disabled).toBe(true)
  })

  it('labels the chat-logs surface and disables the download control', () => {
    stubRuntimeFetch()

    renderWithProviders(() => <ChatbotStudio />, '/agents?agent=chatbot&view=chat-logs')
    expect(screen.getAllByText('Design preview').length).toBeGreaterThan(0)
    expect((screen.getByRole('button', { name: /download chat logs/i }) as HTMLButtonElement).disabled).toBe(true)
  })

  it('labels the playground but keeps its real local controls interactive', () => {
    stubRuntimeFetch()

    renderWithProviders(() => <ChatbotStudio />, '/agents?agent=chatbot&view=playground')

    expect(screen.getAllByText('Design preview').length).toBeGreaterThan(0)
    // Dead affordance is neutralized...
    expect((screen.getByRole('button', { name: /^compare$/i }) as HTMLButtonElement).disabled).toBe(true)
    // ...but the genuinely-wired local tool-pool controls stay interactive.
    expect((screen.getByRole('button', { name: 'Clear' }) as HTMLButtonElement).disabled).toBe(false)
    expect((screen.getByRole('button', { name: 'Select update subscription add-on' }) as HTMLButtonElement).disabled).toBe(false)
  })
})
