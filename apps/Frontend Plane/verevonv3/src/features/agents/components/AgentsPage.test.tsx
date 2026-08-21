// @vitest-environment jsdom

import { createRouter, memoryHistory } from '@solidjs/router'
import { QueryClient, QueryClientProvider } from '@tanstack/solid-query'
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import type { JSX } from '@solidjs/web'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import AgentsPage from '@/features/agents/components/AgentsPage'
import { AgentsProvider } from '@/features/agents/lib/use-agent-selection'

function renderWithProviders(component: () => JSX.Element, path = '/agents') {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
    },
  })

  // AgentsProvider seeds its store from `window.location` on construction
  // (readLocationState()), independent of the TestRouter's own in-memory
  // history below — so the requested `path` must land on `window.location`
  // itself before render, or the provider's initial agent/feature selection
  // won't reflect it.
  window.history.pushState(null, '', path)

  const TestRouter = createRouter({
    explicitLinks: true,
    routes: [{
      path: '/*all',
      component: () => (
        <QueryClientProvider client={queryClient}>
          <AgentsProvider>
            {component()}
          </AgentsProvider>
        </QueryClientProvider>
      ),
    }],
    history: memoryHistory(path),
  })

  return render(() => <TestRouter>{(props) => <>{props.children}</>}</TestRouter>)
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

    // AgentsPage now renders this heading through i18n.tr(no, en); the test
    // environment's default locale is Norwegian ('no'), so the Norwegian
    // string is what actually renders.
    expect(screen.getByRole('heading', { name: /ett agentsystem for hele kundereisen/i })).toBeTruthy()
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
    flush()

    expect(screen.getByRole('heading', { name: /^playground$/i, level: 1 })).toBeTruthy()
    expect(screen.getByRole('heading', { name: /verevon support agent/i })).toBeTruthy()
    // These aria-labels/text are now i18n.tr(no, en) calls that render the
    // Norwegian string under the test environment's default 'no' locale.
    expect(screen.getByRole('button', { name: /velg tillegg for abonnementsoppdatering/i })).toBeTruthy()
    expect(screen.getByRole('textbox', { name: /systemprompt for instruksjoner/i })).toBeTruthy()
    expect(screen.getByRole('combobox', { name: /model/i })).toBeTruthy()
    await waitFor(() => expect(screen.getByText('Live supporthandlinger tilkoblet')).toBeTruthy())
  })

  it('opens the workflow builder and configures a selected workflow node', () => {
    renderWithProviders(() => <AgentsPage />)

    fireEvent.click(screen.getByRole('button', { name: /workflow builder/i }))
    flush()

    // WorkflowCanvas's top-bar heading and the canvas/prompt aria-labels are
    // i18n.tr(no, en) calls; the test environment's default 'no' locale renders
    // the Norwegian string. "Generate Caption" / "Post on Instagram" come from
    // the hardcoded (untranslated) workflow node/inspector data, so those stay
    // in English.
    expect(screen.getByRole('heading', { name: /generer innlegg til sosiale medier/i })).toBeTruthy()
    expect(screen.getByRole('heading', { name: /generate caption/i })).toBeTruthy()
    expect(screen.getByRole('region', { name: /arbeidsflyt-lerret/i })).toBeTruthy()
    expect(screen.getByRole('textbox', { name: /arbeidsflyt-prompt/i })).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: /velg arbeidsflytnoden post on instagram/i }))
    flush()

    expect(screen.getByRole('heading', { name: /post on instagram/i })).toBeTruthy()
    // Phase 3 PR-1 removed the dead Test Run / Publish controls and labelled the
    // WorkflowBuilder a design preview; assert that honesty label (not the
    // now-removed Test Run button) here. DesignPreviewBadge's default label is
    // now i18n.tr('Designforhåndsvisning', 'Design preview'), which renders the
    // Norwegian string under the test environment's default 'no' locale.
    expect(screen.getAllByText('Designforhåndsvisning').length).toBeGreaterThan(0)
  })
})

// Phase 4 agents-studio honesty sweep: the role operating-model workspaces are
// static design previews (no agent activation/deploy/booking backend yet), so
// their action controls must be disabled and the surfaces labelled.
describe('AgentsPage honesty sweep (Phase 4)', () => {
  it('labels the ecommerce store workspace as a design preview and disables Review/Apply', () => {
    renderWithProviders(() => <AgentsPage />, '/agents?agent=ecommerce&feature=commerce-store')

    // DesignPreviewBadge's default label and the Review/Apply button copy on
    // the commerce-store surface are now i18n.tr(no, en) calls, rendering the
    // Norwegian strings under the test environment's default 'no' locale.
    expect(screen.getAllByText('Designforhåndsvisning').length).toBeGreaterThan(0)
    expect((screen.getByRole('button', { name: 'Vurder' }) as HTMLButtonElement).disabled).toBe(true)
    expect((screen.getByRole('button', { name: 'Bruk' }) as HTMLButtonElement).disabled).toBe(true)
  })

  it('disables the preview-only meeting-slot picker on the sales booking workspace', () => {
    renderWithProviders(() => <AgentsPage />, '/agents?agent=sales&feature=sales-booking')

    expect(screen.getAllByText('Designforhåndsvisning').length).toBeGreaterThan(0)
    const disabledButtons = screen.getAllByRole('button').filter((button) => (button as HTMLButtonElement).disabled)
    expect(disabledButtons.length).toBeGreaterThan(1)
  })
})
