// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from '@tanstack/solid-query'
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import type { JSX } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import AgentsPage from '@/features/agents/components/AgentsPage'
import { AgentsProvider } from '@/features/agents/lib/use-agent-selection'

function renderWithProviders(component: () => JSX.Element, path = '/agents') {
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
  window.history.pushState(null, '', '/')
})

describe('AgentsPage', () => {
  it('shows the five v2 agent role entry points on the first screen', () => {
    renderWithProviders(() => <AgentsPage />)

    expect(screen.getByRole('heading', { name: /one agent system for the entire customer journey/i })).toBeTruthy()
    expect(screen.getByRole('button', { name: /service agent/i })).toBeTruthy()
    expect(screen.getByRole('button', { name: /sales agent/i })).toBeTruthy()
    expect(screen.getByRole('button', { name: /ecommerce agent/i })).toBeTruthy()
    expect(screen.getByRole('button', { name: /build your own chatbot/i })).toBeTruthy()
    expect(screen.getByRole('button', { name: /workflow builder/i })).toBeTruthy()
  })

  it('opens the chatbot builder studio from the chatbot card', async () => {
    stubRuntimeFetch()
    renderWithProviders(() => <AgentsPage />)

    fireEvent.click(screen.getByRole('button', { name: /build your own chatbot/i }))

    expect(screen.getByRole('heading', { name: /^playground$/i, level: 1 })).toBeTruthy()
    expect(screen.getByRole('heading', { name: /velion support agent/i })).toBeTruthy()
    expect(screen.getByRole('button', { name: /select update subscription add-on/i })).toBeTruthy()
    expect(screen.getByRole('textbox', { name: /instructions system prompt/i })).toBeTruthy()
    expect(screen.getByRole('combobox', { name: /model/i })).toBeTruthy()
    await waitFor(() => expect(screen.getByText('Live support actions connected')).toBeTruthy())
  })

  it('opens the workflow builder and configures a selected workflow node', () => {
    renderWithProviders(() => <AgentsPage />)

    fireEvent.click(screen.getByRole('button', { name: /workflow builder/i }))

    expect(screen.getByRole('heading', { name: /generate social media post/i })).toBeTruthy()
    expect(screen.getByRole('heading', { name: /generate caption/i })).toBeTruthy()
    expect(screen.getByRole('region', { name: /workflow canvas/i })).toBeTruthy()
    expect(screen.getByRole('textbox', { name: /workflow prompt/i })).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: /select post on instagram workflow node/i }))

    expect(screen.getByRole('heading', { name: /post on instagram/i })).toBeTruthy()
    expect(screen.getByRole('button', { name: /test run/i })).toBeTruthy()
  })
})
