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
