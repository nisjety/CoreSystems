// @vitest-environment jsdom

import { Route, Router } from '@solidjs/router'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@solidjs/testing-library'
import type { JSX } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { QueryProvider } from '@/app/providers/QueryProvider'
import { AgentsProvider } from '@/features/agents/lib/use-agent-selection'
import { readActiveChatThreadId, setActiveChatThreadId, upsertChatThreadHistory } from '@/features/chat/lib/chat-thread-history'
import { CoreSidebar } from '@/features/core/components/CoreSidebar'
import { demoWorkspaceIdentity, routeFromPath } from '@/features/core/lib/shell-data'
import { CoreNavbar } from '@/features/core/components/CoreNavbar'
import DashboardHome from '@/features/dashboard/home/DashboardHome'
import { I18nProvider } from '@/shared/i18n'
import { clearSession, markSessionOnboardingComplete, setSessionUser } from '@/shared/session/session-store'

function renderWithRouter(component: () => JSX.Element, path = '/dashboard') {
  window.history.pushState(null, '', path)
  return render(() => (
    <QueryProvider>
      <I18nProvider>
        <Router root={(props) => <>{props.children}</>}>
          <Route path="/*all" component={component} />
        </Router>
      </I18nProvider>
    </QueryProvider>
  ))
}

function createStorageMock(): Storage {
  const entries = new Map<string, string>()

  return {
    clear: vi.fn(() => entries.clear()),
    getItem: vi.fn((key: string) => entries.get(key) ?? null),
    key: vi.fn((index: number) => Array.from(entries.keys())[index] ?? null),
    get length() {
      return entries.size
    },
    removeItem: vi.fn((key: string) => entries.delete(key)),
    setItem: vi.fn((key: string, value: string) => entries.set(key, value)),
  }
}

afterEach(() => {
  clearSession()
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
    expect(screen.getByRole('navigation', { name: 'Arbeidsområdeseksjoner' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Chat' })).toBeTruthy()
    expect(screen.getByRole('heading', { name: /God (morgen|ettermiddag|kveld), Verevon/ })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Velg AI-modell' })).toBeTruthy()
    expect(screen.getByText('Opprett agent')).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Legg til filer' })).toBeTruthy()
    expect(screen.getAllByRole('button', { name: 'Dyp research' }).length).toBeGreaterThanOrEqual(1)
    expect(screen.getByText('Lokalt værbilde')).toBeTruthy()
    expect(screen.getByText('Trafikk rundt Oslo')).toBeTruthy()
    expect(screen.getByText('Norske nyheter')).toBeTruthy()
  })

  it('uses the requested original Chat icon and freeform Studio icon in the mini rail', () => {
    renderWithRouter(() => (
      <AgentsProvider>
        <CoreSidebar
          activeRoute="/dashboard"
          expanded={false}
          onExpandedChange={vi.fn()}
          onOpenSearch={vi.fn()}
        />
      </AgentsProvider>
    ))

    const workspaceNavigation = screen.getByRole('navigation', { name: 'Arbeidsområdeseksjoner' })
    const chatIcon = within(workspaceNavigation).getByRole('link', { name: 'Chat' }).querySelector('svg')
    const studioIcon = within(workspaceNavigation).getByRole('link', { name: 'Studio' }).querySelector('svg')

    expect(chatIcon?.getAttribute('class')).toContain('lucide-message-square')
    expect(chatIcon?.getAttribute('class')).not.toContain('lucide-bot-message-square')
    expect(studioIcon?.getAttribute('class')).toContain('lucide-layout-freeform')
  })

  it('renders v2-shaped dashboard search and knowledge tab panels', () => {
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input)
      if (url.endsWith('/api/v1/auth/session')) {
        return new Response(JSON.stringify({
          data: {
            user: {
              id: 'user-demo',
              email: 'verevon@example.com',
              name: 'Verevon Demo',
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
            orgs: [{ id: 'org-demo', name: 'Verevon', role: 'owner' }],
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
    expect(screen.getByRole('heading', { name: 'Søk på nett og i Verevon' })).toBeTruthy()
    expect(screen.getByRole('combobox', { name: 'Søk i selskapets kunnskap' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Send søkekontekst til chat' })).toBeTruthy()
    fireEvent.input(screen.getByRole('combobox', { name: 'Søk i selskapets kunnskap' }), {
      target: { value: 'agent status' },
    })
    expect(screen.queryByText('AI operations status')).toBeNull()

    fireEvent.click(screen.getByRole('button', { name: 'Innhent' }))
    // The unified browser chrome dropped the composer's decorative caption
    // block ("… Verevon indekserer alt."), so the panel heading now appears
    // exactly once and the caption text is gone by design.
    expect(screen.getByRole('heading', { name: 'Hent inn kunnskap' })).toBeTruthy()
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

    fireEvent.click(screen.getByRole('button', { name: 'AI-forbedre' }))
    expect((screen.getByRole('textbox', { name: 'Meld Verevon' }) as HTMLTextAreaElement).value)
      .toBe('Beskriv og analyser vedlagte fil(er): brief.pdf')

    fireEvent.click(screen.getByRole('button', { name: 'Fjern brief.pdf' }))
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
    await waitFor(() => expect(screen.getByText('Vis alle samtaler')).toBeTruthy())

    fireEvent.click(screen.getByRole('button', { name: 'Innstillinger' }))
    expect(screen.getByText('Stemmespråk')).toBeTruthy()
    expect(screen.getByText('Kortfattet')).toBeTruthy()
    expect(screen.getByText('Balansert')).toBeTruthy()
    expect(screen.getByText('Detaljert')).toBeTruthy()
    expect(screen.getByText('Legg til filer eller bilder')).toBeTruthy()
    expect(screen.getByText('Ta et skjermbilde')).toBeTruthy()
    // "Legg til i prosjekt" left DashboardComposer in eeb99146 and this
    // assertion did not go with it, so the test has been red since. Not
    // restored as a menu entry: its successor is attaching the composer's work
    // to a Space (`project` is a Space kind in ADR-0001) and that action does
    // not exist yet. A menu item naming a capability nothing implements is
    // worse than its absence.
    expect(screen.getByText('Ferdigheter')).toBeTruthy()
    expect(screen.getByText('Koblinger')).toBeTruthy()

    fireEvent.click(screen.getByText('Ferdigheter'))
    await waitFor(() => expect(screen.getByText('Support drafts')).toBeTruthy())
    expect(screen.getByText('Draft customer replies from connected workspace data.')).toBeTruthy()
    expect(screen.getByText('Administrer ferdigheter')).toBeTruthy()
    const fetchCalls = fetchMock.mock.calls as unknown as Array<[unknown, { credentials?: RequestCredentials } | undefined]>
    expect(fetchCalls.some(([url, init]) =>
      String(url).endsWith('/api/v1/skills') &&
      init?.credentials === 'include',
    )).toBe(true)
  })

  it('stores submitted dashboard composer turns in the v2-style history panel', () => {
    renderWithRouter(() => <DashboardHome workspace={demoWorkspaceIdentity} />)

    fireEvent.input(screen.getByRole('textbox', { name: 'Meld Verevon' }), {
      target: { value: 'Oppsummer kundesaker' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Send melding' }))

    expect(screen.getByText('Oppsummer kundesaker')).toBeTruthy()
    // The composer model selector defaults to the "Verevon Balance" intent mode
    // (resolved server-side, cost-aware); no concrete catalog model is shown until
    // the user picks one.
    expect(screen.getByText(/Verevon Balance · Auto/)).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: 'Historikk' }))
    expect(screen.getByText('I dag')).toBeTruthy()
    expect(screen.getAllByText('Oppsummer kundesaker').length).toBeGreaterThanOrEqual(2)
  })

  it('renders v2-style navbar dropdown panels instead of placeholder popovers', () => {
    renderWithRouter(() => <CoreNavbar activeRoute="/dashboard" workspace={demoWorkspaceIdentity} />)

    fireEvent.click(screen.getByRole('button', { name: '0 uleste meldinger' }))
    expect(screen.getByText('Vis alle meldinger')).toBeTruthy()
    expect(screen.getByText('Koble til Novu for å vise innboks og Verevon AI-chatmeldinger.')).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: 'Kalender' }))
    expect(screen.getByText('Ingen hendelser denne dagen')).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: 'Åpne profilmeny' }))
    expect(screen.getByText('Abonnement')).toBeTruthy()
  })

  it('lists and switches canonical Auth organizations instead of a local-only workspace toggle', async () => {
    setSessionUser({
      id: 'user_1',
      email: 'user@example.com',
      name: 'User',
      emailVerified: true,
    })
    markSessionOnboardingComplete({ id: 'org_1', name: 'Verevon', role: 'owner' })
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path === '/api/v1/orgs') {
        return new Response(JSON.stringify([
          { id: 'org_1', name: 'Verevon', slug: 'verevon', metadata: { plan: 'trial' } },
          { id: 'org_2', name: 'Acme', slug: 'acme', metadata: { plan: 'enterprise' } },
        ]), { headers: { 'Content-Type': 'application/json' }, status: 200 })
      }
      if (path === '/api/v1/orgs/switch-active') {
        expect(JSON.parse(String(init?.body))).toEqual({ organizationId: 'org_2' })
        return new Response(JSON.stringify({ organization: { id: 'org_2' } }), {
          headers: { 'Content-Type': 'application/json' },
          status: 200,
        })
      }
      if (path === '/api/v1/auth/session') {
        return new Response(JSON.stringify({
          user: { id: 'user_1', email: 'user@example.com', name: 'User', emailVerified: true },
        }), { headers: { 'Content-Type': 'application/json' }, status: 200 })
      }
      if (path === '/api/v1/session/current') {
        return new Response(JSON.stringify({
          user: { id: 'user_1', email: 'user@example.com', name: 'User', emailVerified: true },
          org: { id: 'org_2', name: 'Acme', role: 'admin' },
          permissions: [],
          onboardingStatus: 'COMPLETED',
          status: 'authenticated',
        }), { headers: { 'Content-Type': 'application/json' }, status: 200 })
      }
      throw new Error(`Unexpected request: ${path}`)
    })
    vi.stubGlobal('fetch', fetchMock)

    renderWithRouter(() => <CoreNavbar activeRoute="/dashboard" workspace={demoWorkspaceIdentity} />)
    fireEvent.click(screen.getByRole('button', { name: /Verevon.*Trial/ }))

    expect(await screen.findByText('Acme')).toBeTruthy()
    expect(screen.queryByText('Personlig arbeidsområde')).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: /Acme/ }))

    await waitFor(() => {
      expect(fetchMock.mock.calls.some(([path]) => path === '/api/v1/orgs/switch-active')).toBe(true)
    })
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
    const knowledgeNavigation = screen.getByRole('navigation', { name: 'Kunnskapsnavigasjon' })
    expect(knowledgeNavigation).toBeTruthy()
    expect(screen.getByText('Kunnskapsbase')).toBeTruthy()
    expect(screen.getByText('Festede kilder')).toBeTruthy()
    expect(within(knowledgeNavigation).getByRole('link', { name: 'Innsikt' })).toBeTruthy()

    renderSidebar('/support')
    const supportNavigation = screen.getByRole('navigation', { name: 'Supportnavigasjon' })
    expect(supportNavigation).toBeTruthy()
    expect(within(supportNavigation).queryByRole('tab', { name: 'Samtaler' })).toBeNull()
    expect(within(supportNavigation).getByRole('link', { name: 'Mine samtaler' }).getAttribute('href')).toBe('/support?view=mine')
    expect(within(supportNavigation).getByRole('link', { name: 'Alle åpne' }).getAttribute('href')).toBe('/support?view=all&status=open')
    expect(within(supportNavigation).queryByRole('link', { name: 'Spam' })).toBeNull()
    expect(within(supportNavigation).getByRole('link', { name: 'Slack' }).getAttribute('href')).toBe('/support?view=all&channel=slack')
    expect(within(supportNavigation).queryByRole('tab', { name: 'Saksbehandling' })).toBeNull()
    expect(within(supportNavigation).queryByText('Verevon AI-agent')).toBeNull()

    // Existing Inbox and Ticketing links retain their destination, but now
    // resolve to the same Support navigation system.
    renderSidebar('/inbox?view=mine')
    expect(screen.getByRole('navigation', { name: 'Supportnavigasjon' })).toBeTruthy()
    renderSidebar('/tickets?queue=suggested')
    expect(screen.getByRole('navigation', { name: 'Supportnavigasjon' })).toBeTruthy()

    renderSidebar('/support?surface=tickets')
    const ticketSupportNavigation = screen.getByRole('navigation', { name: 'Supportnavigasjon' })
    expect(within(ticketSupportNavigation).queryByRole('tab', { name: /^Saksbehandling$/ })).toBeNull()
    expect(within(ticketSupportNavigation).queryByRole('tab', { name: /^Samtaler$/ })).toBeNull()
    expect(within(ticketSupportNavigation).getByRole('link', { name: 'Alle saker' }).getAttribute('href')).toBe('/support?surface=tickets&queue=all')
    expect(within(ticketSupportNavigation).getByRole('link', { name: 'Foreslått av AI' }).getAttribute('href')).toBe('/support?surface=tickets&queue=suggested')
    expect(within(ticketSupportNavigation).getByRole('link', { name: 'Mine saker' })).toBeTruthy()
    expect(within(ticketSupportNavigation).getByRole('link', { name: 'SLA-risiko' })).toBeTruthy()

    renderSidebar('/support?surface=tickets&queue=sla-risk&sla_state=breached')
    const breachedSlaNavigation = screen.getByRole('navigation', { name: 'Supportnavigasjon' })
    expect(within(breachedSlaNavigation).getByRole('link', { name: 'Brutt SLA' }).className).toContain('core-sidebar-panel-link--active')
    expect(within(breachedSlaNavigation).getByRole('link', { name: 'SLA-risiko' }).className).not.toContain('core-sidebar-panel-link--active')

    renderSidebar('/studio/canvas')
    expect(screen.getByRole('navigation', { name: 'Studio navigasjon' })).toBeTruthy()
    expect(screen.getByText('Kampanjeplanlegger')).toBeTruthy()
    expect(screen.getByText('Sosiale utkast')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Slå sammen Opprett' }))
    expect(screen.queryByText('Kampanjeplanlegger')).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Utvid Opprett' }))
    expect(screen.getByText('Kampanjeplanlegger')).toBeTruthy()

    renderSidebar('/social/trends')
    expect(screen.getByRole('navigation', { name: 'Kalender navigasjon' })).toBeTruthy()
    expect(screen.getByText('Konkurrentovervåking')).toBeTruthy()
    expect(screen.getByText('Evergreen-kø')).toBeTruthy()

    // Closed-demo nav gate (sidebar-navigation.ts `applyDemoModeNavGate`):
    // Insights keeps only its Overview item — the other 5 (Social, Inbox,
    // Agents, Campaigns, Experiments) are design-prototype/no-backend-yet
    // surfaces and stay reachable by direct URL, just not from this panel.
    renderSidebar('/insights/social')
    const insightsNavigation = screen.getByRole('navigation', { name: 'Innsikt navigasjon' })
    expect(insightsNavigation).toBeTruthy()
    expect(within(insightsNavigation).getByRole('link', { name: 'Oversikt' })).toBeTruthy()
    expect(within(insightsNavigation).queryByRole('link', { name: 'Eksperimenter' })).toBeNull()
    expect(within(insightsNavigation).queryByRole('link', { name: 'Kampanjer' })).toBeNull()

    // Agents is available again as a dedicated sidebar surface.
    renderSidebar('/agents')
    expect(screen.getByRole('navigation', { name: 'Agentfaner' })).toBeTruthy()
    expect(screen.getByText('Alle roller')).toBeTruthy()

    renderSidebar('/settings/workspace')
    expect(screen.getByRole('navigation', { name: 'Innstillingsseksjoner' })).toBeTruthy()
    expect(screen.getByText('Medlemmer og roller')).toBeTruthy()

    renderSidebar('/account')
    expect(screen.getByRole('navigation', { name: 'Kontoseksjoner' })).toBeTruthy()
    expect(screen.getByText('Tilkoblede kontoer')).toBeTruthy()
  })

  it('uses the core Rom panel for a Space and does not render the generic open-room item', async () => {
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      const path = String(input)
      if (path === '/api/v1/spaces') {
        return new Response(JSON.stringify({
          data: {
            spaces: [{
              space_ref: 'space personal',
              name: 'Personal Space',
              kind: 'personal',
              lifecycle: 'active',
            }],
          },
        }), { headers: { 'Content-Type': 'application/json' }, status: 200 })
      }
      if (path === '/api/v1/spaces/space%20personal/threads') {
        return new Response(JSON.stringify({
          data: {
            space: {
              space_ref: 'space personal',
              name: 'Personal Space',
              kind: 'personal',
              lifecycle: 'active',
            },
            membership: {
              space_ref: 'space personal',
              org_id: 'org_1',
              subject_id: 'user_1',
              kind: 'personal',
              role: 'owner',
              revisions: { authority: 1, membership: 1, privacy: 1, recipient_audience: 1, entitlement: 1 },
            },
            threads: [{
              thread_id: 'thread / planning',
              space_id: 'space personal',
              title: 'Planning',
              preview: 'Release preparation',
              latest_run_status: 'running',
            }],
          },
        }), { headers: { 'Content-Type': 'application/json' }, status: 200 })
      }
      return new Response(JSON.stringify({ data: {} }), {
        headers: { 'Content-Type': 'application/json' },
        status: 200,
      })
    }))

    renderWithRouter(() => (
      <AgentsProvider>
        <CoreSidebar
          activeRoute="/spaces"
          expanded
          onExpandedChange={vi.fn()}
          onOpenSearch={vi.fn()}
        />
      </AgentsProvider>
    ), '/spaces/space%20personal')

    expect(screen.getByRole('navigation', { name: 'Romnavigasjon' })).toBeTruthy()
    expect(await screen.findByRole('link', { name: 'Personal Space' })).toBeTruthy()
    expect(screen.getByRole('textbox', { name: /search rooms and conversations|søk i rom og samtaler/i })).toBeTruthy()
    expect(screen.getByRole('link', { name: /(?:Open|Åpne) Planning/ }).getAttribute('href')).toBe('/chat?thread_id=thread%20%2F%20planning')
    expect(screen.queryByRole('link', { name: /Åpne rommet|open room/i })).toBeNull()
  })

  it('renders saved chat thread history in the expanded chat sidebar', () => {
    Object.defineProperty(window, 'localStorage', {
      configurable: true,
      value: createStorageMock(),
    })
    Object.defineProperty(window, 'sessionStorage', {
      configurable: true,
      value: createStorageMock(),
    })
    setActiveChatThreadId('thread-history-1')
    upsertChatThreadHistory({
      threadId: 'thread-history-1',
      title: 'history visible check',
      preview: 'Assistant reply preview',
      updatedAt: new Date().toISOString(),
    })

    renderWithRouter(() => (
      <AgentsProvider>
        <CoreSidebar
          activeRoute="/chat"
          expanded
          onExpandedChange={vi.fn()}
          onOpenSearch={vi.fn()}
        />
      </AgentsProvider>
    ), '/chat')

    const chatHistory = screen.getByRole('navigation', { name: 'Chat-samtaler' })
    expect(within(chatHistory).getByText('history visible check')).toBeTruthy()
    expect(within(chatHistory).getByText('Akkurat nå')).toBeTruthy()
    expect(within(chatHistory).queryByText('Assistant reply preview')).toBeNull()
    expect(within(chatHistory).queryByText('Gjeldende tråd')).toBeNull()
  })

  it('keeps and uploads local chat history when the server index is empty', async () => {
    Object.defineProperty(window, 'localStorage', {
      configurable: true,
      value: createStorageMock(),
    })
    Object.defineProperty(window, 'sessionStorage', {
      configurable: true,
      value: createStorageMock(),
    })
    upsertChatThreadHistory({
      threadId: 'thread-local-only',
      title: 'local only thread',
      preview: 'Assistant reply preview',
      updatedAt: '2026-06-17T10:00:00.000Z',
    })

    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input)
      if (url === '/api/v1/chat/threads' && init?.method !== 'DELETE') {
        return new Response(JSON.stringify({ data: { sessions: [] } }), {
          headers: { 'Content-Type': 'application/json' },
          status: 200,
        })
      }
      if (url === '/api/v1/chat/threads/thread-local-only' && init?.method === 'PUT') {
        return new Response(JSON.stringify({
          data: {
            session: {
              threadId: 'thread-local-only',
              title: 'local only thread',
              preview: 'Assistant reply preview',
              updatedAt: '2026-06-17T10:00:00.000Z',
            },
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
    })
    vi.stubGlobal('fetch', fetchMock)

    renderWithRouter(() => (
      <AgentsProvider>
        <CoreSidebar
          activeRoute="/chat"
          expanded
          onExpandedChange={vi.fn()}
          onOpenSearch={vi.fn()}
        />
      </AgentsProvider>
    ), '/chat')

    const chatHistory = screen.getByRole('navigation', { name: 'Chat-samtaler' })
    expect(within(chatHistory).getByText('local only thread')).toBeTruthy()

    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith(
      '/api/v1/chat/threads/thread-local-only',
      expect.objectContaining({ method: 'PUT', credentials: 'include' }),
    ))
    expect(within(chatHistory).getByText('local only thread')).toBeTruthy()
  })

  it('clears the active chat thread when starting a new sidebar conversation', () => {
    Object.defineProperty(window, 'localStorage', {
      configurable: true,
      value: createStorageMock(),
    })
    Object.defineProperty(window, 'sessionStorage', {
      configurable: true,
      value: createStorageMock(),
    })
    setActiveChatThreadId('thread-history-1')
    upsertChatThreadHistory({
      threadId: 'thread-history-1',
      title: 'history visible check',
      preview: 'Assistant reply preview',
      updatedAt: new Date().toISOString(),
    })

    renderWithRouter(() => (
      <AgentsProvider>
        <CoreSidebar
          activeRoute="/chat"
          expanded
          onExpandedChange={vi.fn()}
          onOpenSearch={vi.fn()}
        />
      </AgentsProvider>
    ), '/chat')

    fireEvent.click(screen.getByText('Ny samtale'))

    expect(readActiveChatThreadId()).toBeNull()
    expect(screen.getByText('history visible check')).toBeTruthy()
  })

  it('switches navbar and dashboard home copy to English', () => {
    renderWithRouter(() => (
      <>
        <CoreNavbar activeRoute="/dashboard" workspace={demoWorkspaceIdentity} />
        <DashboardHome workspace={demoWorkspaceIdentity} />
      </>
    ))

    fireEvent.click(screen.getByRole('button', { name: 'Bytt til English' }))

    expect(screen.getByText('Search knowledge base')).toBeTruthy()
    expect(screen.getByText('Create agent')).toBeTruthy()
    expect(screen.getByRole('textbox', { name: 'Message Verevon' })).toBeTruthy()
  })
})
