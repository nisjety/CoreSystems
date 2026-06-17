// @vitest-environment jsdom

import { Route, Router } from '@solidjs/router'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@solidjs/testing-library'
import type { JSX } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { QueryProvider } from '@/app/providers/QueryProvider'
import { AgentsProvider } from '@/features/agents/lib/use-agent-selection'
import { CoreSidebar } from '@/features/core/components/CoreSidebar'
import { demoWorkspaceIdentity, routeFromPath } from '@/features/core/lib/shell-data'
import { CoreNavbar } from '@/features/core/components/CoreNavbar'
import DashboardHome from '@/features/dashboard/home/DashboardHome'

function renderWithRouter(component: () => JSX.Element, path = '/dashboard') {
  window.history.pushState(null, '', path)
  return render(() => (
    <QueryProvider>
      <Router root={(props) => <>{props.children}</>}>
        <Route path="/*all" component={component} />
      </Router>
    </QueryProvider>
  ))
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('v2 dashboard shell port', () => {
  it('renders the v2-style navbar, sidebar, and dashboard home surfaces', () => {
    renderWithRouter(() => (
      <>
        <CoreNavbar activeRoute="/dashboard" workspace={demoWorkspaceIdentity} />
        <AgentsProvider>
          <CoreSidebar
            activeRoute="/dashboard"
            expanded
            onExpandedChange={vi.fn()}
            onOpenSearch={vi.fn()}
          />
        </AgentsProvider>
        <DashboardHome workspace={demoWorkspaceIdentity} />
      </>
    ))

    expect(screen.getByRole('banner')).toBeTruthy()
    expect(screen.getByRole('navigation', { name: 'Workspace sections' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Chat' })).toBeTruthy()
    expect(screen.getByRole('heading', { name: /God (morgen|ettermiddag|kveld), Velion/ })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Velg AI-modell' })).toBeTruthy()
    expect(screen.getByText('Create agent')).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Add files' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Deep search' })).toBeTruthy()
    expect(screen.getByText('Lokalt værbilde')).toBeTruthy()
    expect(screen.getByText('Trafikk rundt Oslo')).toBeTruthy()
    expect(screen.getByText('Norske nyheter')).toBeTruthy()
  })

  it('renders v2-shaped dashboard search and knowledge tab panels', () => {
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input)
      if (url.endsWith('/api/v1/auth/session')) {
        return new Response(JSON.stringify({
          data: {
            user: {
              id: 'user-demo',
              email: 'velion@example.com',
              name: 'Velion Demo',
              emailVerified: true,
            },
          },
        }), {
          headers: { 'Content-Type': 'application/json' },
          status: 200,
        })
      }
      if (url.endsWith('/api/v1/me/session-context')) {
        return new Response(JSON.stringify({
          data: {
            orgs: [{ id: 'org-demo', name: 'Velion', role: 'owner' }],
          },
        }), {
          headers: { 'Content-Type': 'application/json' },
          status: 200,
        })
      }

      return new Response(JSON.stringify({ data: {} }), {
        headers: { 'Content-Type': 'application/json' },
        status: 200,
      })
    }))

    renderWithRouter(() => <DashboardHome workspace={demoWorkspaceIdentity} />)

    fireEvent.click(screen.getByRole('button', { name: 'Søk' }))
    expect(screen.getByRole('heading', { name: 'Søk på nett og i Velion' })).toBeTruthy()
    expect(screen.getByRole('combobox', { name: 'Søk i selskapets kunnskap' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Send søkekontekst til chat' })).toBeTruthy()
    fireEvent.input(screen.getByRole('combobox', { name: 'Søk i selskapets kunnskap' }), {
      target: { value: 'agent status' },
    })
    expect(screen.queryByText('AI operations status')).toBeNull()

    fireEvent.click(screen.getByRole('button', { name: 'Crawl' }))
    expect(screen.getByRole('heading', { name: 'Crawl inn kunnskap' })).toBeTruthy()
    expect(screen.getAllByText('Crawl inn kunnskap').length).toBeGreaterThanOrEqual(2)
    expect(screen.getByText(/Velion indekserer alt/)).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Last opp filer' })).toBeTruthy()
    expect(screen.getByRole('link', { name: /Åpne kunnskapsbase/ })).toBeTruthy()
  })

  it('renders v2-style dashboard composer attachment previews', () => {
    const { container } = renderWithRouter(() => <DashboardHome workspace={demoWorkspaceIdentity} />)
    const input = container.querySelector('input[type="file"]') as HTMLInputElement
    const file = new File(['hello'], 'brief.pdf', { type: 'application/pdf' })

    fireEvent.change(input, { target: { files: [file] } })

    expect(screen.getByText('brief.pdf')).toBeTruthy()
    expect(screen.getByText('5 B')).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: 'AI enhance' }))
    expect((screen.getByRole('textbox', { name: 'Message Velion' }) as HTMLTextAreaElement).value)
      .toBe('Describe and analyze the attached file(s): brief.pdf')

    fireEvent.click(screen.getByRole('button', { name: 'Remove brief.pdf' }))
    expect(screen.queryByText('brief.pdf')).toBeNull()
  })

  it('renders v2-style dashboard composer history and gateway-backed settings panels', async () => {
    const fetchMock = vi.fn(async () => new Response(JSON.stringify({
      data: {
        skills: [
          {
            id: 'skill-support-drafts',
            name: 'Support drafts',
            description: 'Draft customer replies from connected workspace data.',
          },
        ],
      },
    }), {
      headers: { 'Content-Type': 'application/json' },
      status: 200,
    }))
    vi.stubGlobal('fetch', fetchMock)

    renderWithRouter(() => <DashboardHome workspace={demoWorkspaceIdentity} />)

    fireEvent.click(screen.getByRole('button', { name: 'Historikk' }))
    expect(screen.getByText('Open chat to load real conversation history.')).toBeTruthy()
    expect(screen.getByText('View all conversations')).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: 'Innstillinger' }))
    expect(screen.getByText('Voice language')).toBeTruthy()
    expect(screen.getByText('Concise')).toBeTruthy()
    expect(screen.getByText('Balanced')).toBeTruthy()
    expect(screen.getByText('Detailed')).toBeTruthy()
    expect(screen.getByText('Add files or photos')).toBeTruthy()
    expect(screen.getByText('Take a screenshot')).toBeTruthy()
    expect(screen.getByText('Add to project')).toBeTruthy()
    expect(screen.getByText('Skills')).toBeTruthy()
    expect(screen.getByText('Connectors')).toBeTruthy()

    fireEvent.click(screen.getByText('Skills'))
    await waitFor(() => expect(screen.getByText('Support drafts')).toBeTruthy())
    expect(screen.getByText('Draft customer replies from connected workspace data.')).toBeTruthy()
    expect(screen.getByText('Manage skills')).toBeTruthy()
    const fetchCalls = fetchMock.mock.calls as unknown as Array<[unknown, { credentials?: RequestCredentials } | undefined]>
    expect(fetchCalls.some(([url, init]) =>
      String(url).endsWith('/api/v1/skills') &&
      init?.credentials === 'include',
    )).toBe(true)
  })

  it('stores submitted dashboard composer turns in the v2-style history panel', () => {
    renderWithRouter(() => <DashboardHome workspace={demoWorkspaceIdentity} />)

    fireEvent.input(screen.getByRole('textbox', { name: 'Message Velion' }), {
      target: { value: 'Oppsummer kundesaker' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }))

    expect(screen.getByText('Oppsummer kundesaker')).toBeTruthy()
    // The composer model selector defaults to the "Velion Balance" intent mode
    // (resolved server-side, cost-aware); no concrete catalog model is shown until
    // the user picks one.
    expect(screen.getByText(/Velion Balance · auto/)).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: 'Historikk' }))
    expect(screen.getByText('Today')).toBeTruthy()
    expect(screen.getAllByText('Oppsummer kundesaker').length).toBeGreaterThanOrEqual(2)
  })

  it('renders v2-style navbar dropdown panels instead of placeholder popovers', () => {
    renderWithRouter(() => <CoreNavbar activeRoute="/dashboard" workspace={demoWorkspaceIdentity} />)

    fireEvent.click(screen.getByRole('button', { name: '0 unread messages' }))
    expect(screen.getByText('View all messages')).toBeTruthy()
    expect(screen.getByText('Connect Novu to show inbox and Velion AI chat messages.')).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: 'Calendar' }))
    expect(screen.getByText('No events for this day')).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: 'Open profile menu' }))
    expect(screen.getByText('Subscription')).toBeTruthy()
  })

  it('renders dedicated v2-style expanded sidebar panels for core sections', () => {
    const renderSidebar = (path: string) => {
      cleanup()
      return renderWithRouter(() => (
        <AgentsProvider>
          <CoreSidebar
            activeRoute={routeFromPath(path)}
            expanded
            onExpandedChange={vi.fn()}
            onOpenSearch={vi.fn()}
          />
        </AgentsProvider>
      ), path)
    }

    renderSidebar('/knowledge')
    const knowledgeNavigation = screen.getByRole('navigation', { name: 'Knowledge navigation' })
    expect(knowledgeNavigation).toBeTruthy()
    expect(screen.getByText('Knowledge Base')).toBeTruthy()
    expect(screen.getByText('Pinned sources')).toBeTruthy()
    expect(within(knowledgeNavigation).getByRole('link', { name: 'Insights' })).toBeTruthy()

    renderSidebar('/inbox?view=mine')
    expect(screen.getByRole('navigation', { name: 'Inbox navigation' })).toBeTruthy()
    expect(screen.getByText('Velion AI Agent')).toBeTruthy()
    expect(screen.getByText('Your inbox')).toBeTruthy()

    renderSidebar('/studio/canvas')
    expect(screen.getByRole('navigation', { name: 'Studio navigation' })).toBeTruthy()
    expect(screen.getByText('Campaign planner')).toBeTruthy()
    expect(screen.getByText('Social drafts')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Collapse Create' }))
    expect(screen.queryByText('Campaign planner')).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Expand Create' }))
    expect(screen.getByText('Campaign planner')).toBeTruthy()

    renderSidebar('/social/trends')
    expect(screen.getByRole('navigation', { name: 'Social navigation' })).toBeTruthy()
    expect(screen.getByText('Competitor watch')).toBeTruthy()
    expect(screen.getByText('Evergreen queue')).toBeTruthy()

    renderSidebar('/insights/social')
    expect(screen.getByRole('navigation', { name: 'Insights navigation' })).toBeTruthy()
    expect(screen.getByText('Experiments')).toBeTruthy()
    expect(screen.getByText('Campaigns')).toBeTruthy()

    renderSidebar('/agents')
    expect(screen.getByRole('navigation', { name: 'Agent feature tabs' })).toBeTruthy()
    expect(screen.getByText('All roles')).toBeTruthy()

    renderSidebar('/settings/workspace')
    expect(screen.getByRole('navigation', { name: 'Settings sections' })).toBeTruthy()
    expect(screen.getByText('Members & roles')).toBeTruthy()

    renderSidebar('/account')
    expect(screen.getByRole('navigation', { name: 'Account sections' })).toBeTruthy()
    expect(screen.getByText('Connected accounts')).toBeTruthy()
  })
})
