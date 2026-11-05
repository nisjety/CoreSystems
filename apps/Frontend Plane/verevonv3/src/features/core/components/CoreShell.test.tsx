// @vitest-environment jsdom

import { createRouter, memoryHistory, useLocation, useNavigate } from '@solidjs/router'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@solidjs/testing-library'
import type { JSX } from '@solidjs/web'
import { createSignal, flush, onCleanup } from 'solid-js'
import { DashboardComposer } from '@/features/dashboard/home/DashboardComposer'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { QueryProvider } from '@/app/providers/QueryProvider'
import { AgentsProvider } from '@/features/agents/lib/use-agent-selection'
import { readActiveChatThreadId, setActiveChatThreadId, upsertChatThreadHistory } from '@/features/chat/lib/chat-thread-history'
import { CoreSidebar } from '@/features/core/components/CoreSidebar'
import { CoreShell } from '@/features/core/components/CoreShell'
import type { AssistantAgentView } from '@/features/core/components/GlobalAssistant'
import { createAssistantBrowserAgent } from '@/features/core/assistant-agent/assistant-browser-agent'
import { useAssistantBrowser } from '@/features/core/assistant-agent/assistant-browser-context'
import { createAssistantNavigationAgent } from '@/features/core/assistant-agent/assistant-navigation-agent'
import { demoWorkspaceIdentity, routeFromPath } from '@/features/core/lib/shell-data'
import { CoreNavbar } from '@/features/core/components/CoreNavbar'
import DashboardHome from '@/features/dashboard/home/DashboardHome'
import { I18nProvider } from '@/shared/i18n'
import { clearSession, markSessionOnboardingComplete, setSessionUser } from '@/shared/session/session-store'

vi.mock('@/features/core/assistant-agent/assistant-browser-agent', async (importOriginal) => {
  const module = await importOriginal<typeof import('@/features/core/assistant-agent/assistant-browser-agent')>()
  return { ...module, createAssistantBrowserAgent: vi.fn(module.createAssistantBrowserAgent) }
})

vi.mock('@/features/core/assistant-agent/assistant-navigation-agent', async (importOriginal) => {
  const module = await importOriginal<typeof import('@/features/core/assistant-agent/assistant-navigation-agent')>()
  return { ...module, createAssistantNavigationAgent: vi.fn(module.createAssistantNavigationAgent) }
})

function renderWithRouter(component: () => JSX.Element, path = '/dashboard') {
  const TestRouter = createRouter({
    routes: [{ path: '/*all', component }],
    history: memoryHistory(path),
    explicitLinks: true,
  })
  return render(() => (
    <QueryProvider>
      <I18nProvider>
        <TestRouter>{(props) => <>{props.children}</>}</TestRouter>
      </I18nProvider>
    </QueryProvider>
  ))
}

function TestComposer() {
  const [message, setMessage] = createSignal('')
  return <DashboardComposer message={message()} onMessageChange={setMessage} />
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
  it('opens Nett once the browser task starts and preserves the assistant result if the user leaves Nett', async () => {
    const starting: AssistantAgentView = { available: true, kind: 'browser', status: 'starting', goal: 'Read example.com', error: null, action: null, sessionId: 'session-1', runId: null, receipt: null }
    let publish!: (task: AssistantAgentView) => void
    vi.mocked(createAssistantBrowserAgent).mockImplementationOnce(({ onChange }) => {
      publish = onChange
      const noop = () => undefined
      const resolve = async () => undefined
      return { send: resolve, stop: noop, retry: resolve, allowSiteAccess: resolve, declineSiteAccess: noop, decide: resolve, reset: noop, dispose: noop, claimRetained: () => false, state: () => starting }
    })
    const LocationProbe = () => {
      const location = useLocation()
      const navigate = useNavigate()
      return <><output data-testid="agent-route">{location.pathname}{location.search}</output><button type="button" onClick={() => navigate('/dashboard?ref=mail')}>Leave Nett</button></>
    }
    renderWithRouter(() => <CoreShell><LocationProbe /></CoreShell>, '/dashboard?ref=mail&overview=knowledge')

    publish(starting); flush()
    expect(screen.getByTestId('agent-route').textContent).toBe('/dashboard?ref=mail&overview=knowledge')

    const accepted = { ...starting, runId: 'run-1' }
    publish(accepted); flush()
    await waitFor(() => expect(screen.getByTestId('agent-route').textContent).toBe('/dashboard?ref=mail&tab=web'))

    fireEvent.click(screen.getByRole('button', { name: 'Leave Nett' }))
    await waitFor(() => expect(screen.getByTestId('agent-route').textContent).toBe('/dashboard?ref=mail'))
    publish({ ...accepted, status: 'completed', result: 'The page title is Example Domain.' }); flush()
    expect(screen.getByTestId('agent-route').textContent).toBe('/dashboard?ref=mail')
    expect(screen.getByText('The page title is Example Domain.')).toBeTruthy()
    expect(screen.queryByLabelText('Agentens nettleser')).toBeNull()
  })

  it('reads only the current live Nett tab and discards a late owner response after the workspace changes', async () => {
    setSessionUser({ id: 'user-1', email: 'user@example.com', name: 'User', emailVerified: true })
    flush()
    markSessionOnboardingComplete({ id: 'org-1', name: 'Verevon', role: 'owner' })
    flush()
    let resolveTarget!: NonNullable<Parameters<typeof createAssistantNavigationAgent>[0]['currentBrowserTarget']>
    vi.mocked(createAssistantNavigationAgent).mockImplementationOnce((options) => {
      resolveTarget = options.currentBrowserTarget!
      const noop = () => undefined
      return { send: async () => undefined, stop: noop, reset: noop, dispose: noop, state: () => ({ status: 'idle', error: null }) }
    })
    const projection = (live: boolean) => new Response(JSON.stringify({ data: {
      runId: 'net-session', live, controlMode: 'human_takeover', controlEpoch: 1,
      viewport: { width: 1280, height: 800 }, zdr: false, profileScope: 'ephemeral', profileStorage: 'ephemeral',
      currentUrl: 'https://example.org/old', tabs: [
        { tabId: 'old', active: false, url: 'https://example.org/old' },
        { tabId: 'active', active: true, url: 'https://example.org/current' },
      ],
    } }), { headers: { 'Content-Type': 'application/json' } })
    const ownerRead = vi.fn<() => Promise<Response>>()
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      if (String(input).endsWith('/api/v1/browser/sessions/net-session')) return ownerRead()
      return new Response(null, { status: 404 })
    }))
    const [visible, setVisible] = createSignal(true)
    const NettProbe = () => {
      const browser = useAssistantBrowser()
      onCleanup(browser.registerNettBrowser!(() => visible() ? { userId: 'user-1', orgId: 'org-1', sessionId: 'net-session' } : null))
      return <span>Nett probe</span>
    }
    renderWithRouter(() => <CoreShell><NettProbe /></CoreShell>, '/dashboard?tab=web')
    const signal = new AbortController().signal

    ownerRead.mockResolvedValueOnce(projection(true)).mockResolvedValueOnce(projection(false))
    const initialTarget = await resolveTarget(signal)
    expect(ownerRead).toHaveBeenCalledTimes(1)
    expect(initialTarget).toBe('https://example.org/current')
    expect(await resolveTarget(signal)).toBeNull()
    setVisible(false)
    flush()
    expect(await resolveTarget(signal)).toBeNull()
    expect(ownerRead).toHaveBeenCalledTimes(2)

    setVisible(true)
    flush()
    let reply!: (value: Response) => void
    ownerRead.mockImplementationOnce(() => new Promise(resolve => { reply = resolve }))
    const pending = resolveTarget(signal)
    await waitFor(() => expect(ownerRead).toHaveBeenCalledTimes(3))
    markSessionOnboardingComplete({ id: 'org-2', name: 'Another workspace', role: 'owner' })
    flush()
    reply(projection(true))
    expect(await pending).toBeNull()
  })

  it('requires a fresh matching owner tab before sending a continuation', async () => {
    setSessionUser({ id: 'user-1', email: 'user@example.com', name: 'User', emailVerified: true })
    flush()
    markSessionOnboardingComplete({ id: 'org-1', name: 'Verevon', role: 'owner' })
    flush()
    const send = vi.fn(async () => undefined)
    vi.mocked(createAssistantBrowserAgent).mockImplementationOnce((options) => ({
      ...vi.mocked(createAssistantBrowserAgent).getMockImplementation()!(options),
      send,
    }))
    let tabEpoch = 6
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      if (!String(input).endsWith('/api/v1/browser/sessions/net-session')) return new Response(null, { status: 404 })
      return Response.json({ data: {
        runId: 'net-session', live: true, controlMode: 'human_takeover', controlEpoch: 1,
        viewport: { width: 1280, height: 800 }, zdr: false, profileScope: 'ephemeral', profileStorage: 'ephemeral',
        currentUrl: 'https://example.org/page', tabs: [
          { tabId: 'selected', active: true, url: 'https://example.org/page', tabEpoch, holder: 'human' },
        ],
      } })
    }))
    const NettProbe = () => {
      const browser = useAssistantBrowser()
      onCleanup(browser.registerNettBrowser!(() => ({ userId: 'user-1', orgId: 'org-1', sessionId: 'net-session' })))
      return <button type="button" onClick={() => browser.prepareContinuation?.({ sessionId: 'net-session', tabId: 'selected', origin: 'https://example.org', tabEpoch: 7 })}>Choose tab</button>
    }
    renderWithRouter(() => <CoreShell><NettProbe /></CoreShell>, '/dashboard?tab=web')
    fireEvent.click(screen.getByRole('button', { name: 'Choose tab' }))
    flush()
    expect(await screen.findByText('Fortsett i denne fanen · example.org')).toBeTruthy()
    const input = screen.getByRole('textbox', { name: 'Oppgave til Verevon' }) as HTMLTextAreaElement
    fireEvent.input(input, { target: { value: 'Les overskriften' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Start oppgave' }))
    await screen.findByText(/Fanen er endret/)
    expect(input.value).toBe('Les overskriften')
    expect(send).not.toHaveBeenCalled()

    tabEpoch = 7
    fireEvent.click(screen.getByRole('button', { name: 'Choose tab' }))
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Start oppgave' }))
    await waitFor(() => expect(send).toHaveBeenCalledWith('Les overskriften', 'https://example.org', {
      sessionId: 'net-session', tabId: 'selected', expectedEpoch: 7, expectedOrigin: 'https://example.org',
    }))
    expect(screen.queryByText('Fortsett i denne fanen · example.org')).toBeNull()
  })

  it('renders the v2-style navbar, sidebar, and dashboard overview surfaces', () => {
    const { container } = renderWithRouter(() => (
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

    expect(container.querySelector('header.core-navbar')).toBeTruthy()
    expect(screen.getByRole('navigation', { name: 'Arbeidsområdeseksjoner' })).toBeTruthy()
    expect(screen.getByRole('navigation', { name: 'Snarveier i arbeidsområdet' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Assistent' })).toBeTruthy()
    expect(screen.getByRole('heading', { name: /God (morgen|ettermiddag|kveld), Verevon/ })).toBeTruthy()
    expect(screen.getByRole('heading', { name: 'Fortsett der du slapp' })).toBeTruthy()
    expect(screen.getByText('Lokalt værbilde')).toBeTruthy()
    expect(screen.getByRole('heading', { name: 'Norske nyheter' })).toBeTruthy()
    expect(screen.getByRole('heading', { name: 'Klar for neste steg' })).toBeTruthy()

    // The single assistant is mounted by CoreShell, not a second Home composer.
    expect(screen.queryByRole('region', { name: 'Spør Verevon' })).toBeNull()
  })

  it('keeps the engagement icon consistent and omits Studio and Calendar from the mini rail', () => {
    const first = renderWithRouter(() => (
      <AgentsProvider>
        <CoreSidebar
          activeRoute="/dashboard"
          expanded={false}
          onExpandedChange={vi.fn()}
          onOpenSearch={vi.fn()}
        />
      </AgentsProvider>
    ))

    const workspaceNavigation = screen.getByRole('navigation', { name: /arbeidsområ|arbeidsomr/i })
    const chatIcon = within(workspaceNavigation).getByRole('link', { name: 'Chat' }).querySelector('svg')
    const engagementIcon = within(workspaceNavigation).getByRole('link', { name: 'Engasjement' }).querySelector('svg')

    expect(chatIcon?.getAttribute('class')).toContain('lucide-message-circle-more')
    expect(chatIcon?.getAttribute('class')).not.toContain('lucide-bot-message-square')
    expect(engagementIcon?.getAttribute('class')).toContain('lucide-target')
    expect(within(workspaceNavigation).queryByRole('link', { name: 'Studio' })).toBeNull()
    expect(within(workspaceNavigation).queryByRole('link', { name: 'Kalender' })).toBeNull()
    first.unmount()

    renderWithRouter(() => (
      <AgentsProvider>
        <CoreSidebar activeRoute="/engagement" expanded={false} onExpandedChange={vi.fn()} onOpenSearch={vi.fn()} />
      </AgentsProvider>
    ), '/engagement')
    const activeIcon = within(screen.getByRole('navigation', { name: /arbeidsområ|arbeidsomr/i }))
      .getByRole('link', { name: 'Engasjement' }).querySelector('svg')
    expect(activeIcon?.getAttribute('class')).toContain('core-rail-filled-icon')
    expect(activeIcon?.querySelector('path')?.getAttribute('d')).toContain('a7 7')
  })

  it('renders v2-shaped dashboard search and knowledge tab panels', async () => {
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

    // The tabs are named Chat / Nett / Hent ut and live in the URL (?tab=), so
    // a click resolves one router tick later.
    fireEvent.click(screen.getByRole('button', { name: 'Nett' }))
    flush()
    expect(await screen.findByRole('heading', { name: 'Søk på nett og i Verevon' })).toBeTruthy()
    expect(screen.getByRole('combobox', { name: 'Søk på nettet eller spør' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Send søkekontekst til chat' })).toBeTruthy()
    fireEvent.input(screen.getByRole('combobox', { name: 'Søk på nettet eller spør' }), {
      target: { value: 'agent status' },
    })
    flush()
    expect(screen.queryByText('AI operations status')).toBeNull()

    fireEvent.click(screen.getByRole('button', { name: 'Hent ut' }))
    flush()
    // The unified browser chrome dropped the composer's decorative caption
    // block ("… Verevon indekserer alt."), so the panel heading now appears
    // exactly once and the caption text is gone by design.
    expect(await screen.findByRole('heading', { name: 'Hent inn kunnskap' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Last opp filer' })).toBeTruthy()
    expect(screen.getByRole('link', { name: /Åpne kunnskapsbase/ })).toBeTruthy()
  })

  it('renders v2-style dashboard composer attachment previews', () => {
    const { container } = renderWithRouter(() => <TestComposer />)
    const input = container.querySelector('input[type="file"]') as HTMLInputElement
    const file = new File(['hello'], 'brief.pdf', { type: 'application/pdf' })

    fireEvent.change(input, { target: { files: [file] } })
    flush()

    expect(screen.getByText('brief.pdf')).toBeTruthy()
    expect(screen.getByText('5 B')).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: 'AI-forbedre' }))
    flush()
    expect((screen.getByRole('textbox', { name: 'Meld Verevon' }) as HTMLTextAreaElement).value)
      .toBe('Beskriv og analyser vedlagte fil(er): brief.pdf')

    fireEvent.click(screen.getByRole('button', { name: 'Fjern brief.pdf' }))
    flush()
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

    renderWithRouter(() => <TestComposer />)

    fireEvent.click(screen.getByRole('button', { name: 'Historikk' }))
    await waitFor(() => expect(screen.getByText('Vis alle samtaler')).toBeTruthy())

    fireEvent.click(screen.getByRole('button', { name: 'Innstillinger' }))
    flush()
    expect(screen.getByText('Stemmespråk')).toBeTruthy()
    expect(screen.getByText('Kortfattet')).toBeTruthy()
    expect(screen.getByText('Balansert')).toBeTruthy()
    expect(screen.getByText('Detaljert')).toBeTruthy()
    expect(screen.getByText('Legg til filer eller bilder')).toBeTruthy()
    expect(screen.getByText('Ta et skjermbilde')).toBeTruthy()
    // "Legg til i prosjekt" left DashboardComposer in eeb99146 ("checkpoint
    // cross-plane space authority and hardening") and this assertion did not go
    // with it, so the test has been red since. Not restored as a menu entry:
    // its successor is attaching the composer's work to a Space (`project` is a
    // Space kind in ADR-0001) and that action does not exist yet. A menu item
    // naming a capability nothing implements is worse than its absence.
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

  it('stores submitted dashboard composer turns in the v2-style history panel', async () => {
    renderWithRouter(() => <TestComposer />)

    fireEvent.input(screen.getByRole('textbox', { name: 'Meld Verevon' }), {
      target: { value: 'Oppsummer kundesaker' },
    })
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Send melding' }))
    flush()

    // The send is async: the turn is recorded once the pending-chat handoff
    // (`writePendingChatLaunch`) resolves, a tick after the click. Verevon
    // Balance is an intent mode, so there is no catalogue tier wait here.
    expect(await screen.findByText('Oppsummer kundesaker')).toBeTruthy()
    // The composer model selector defaults to the "Verevon Balance" intent mode
    // (resolved server-side, cost-aware); no concrete catalog model is shown until
    // the user picks one.
    expect(screen.getByText(/Verevon Balance · Auto/)).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: 'Historikk' }))
    flush()
    expect(screen.getByText('I dag')).toBeTruthy()
    expect(screen.getAllByText('Oppsummer kundesaker').length).toBeGreaterThanOrEqual(2)
  })

  it('renders v2-style navbar dropdown panels instead of placeholder popovers', () => {
    renderWithRouter(() => <CoreNavbar activeRoute="/dashboard" workspace={demoWorkspaceIdentity} />)

    fireEvent.click(screen.getByRole('button', { name: '0 uleste meldinger' }))
    flush()
    expect(screen.getByText('Vis alle meldinger')).toBeTruthy()
    expect(screen.getByText('Koble til Novu for å vise innboks og Verevon AI-chatmeldinger.')).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: 'Kalender' }))
    flush()
    expect(screen.getByText('Ingen hendelser denne dagen')).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: 'Åpne profilmeny' }))
    flush()
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
    flush()
    expect(screen.queryByText('Kampanjeplanlegger')).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Utvid Opprett' }))
    flush()
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
    expect(screen.getByRole('navigation', { name: /innstill|seksjoner/i })).toBeTruthy()

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
    expect(await screen.findByRole('link', { name: 'Personlig rom' })).toBeTruthy()
    expect(screen.getByRole('textbox', { name: /search rooms and conversations|søk i rom og samtaler/i })).toBeTruthy()
    // A Space conversation opens in its room, on that thread (`spaceThreadHref`); the shared record
    // never routes out to /chat from the sidebar.
    expect(screen.getByRole('link', { name: /(?:Open|Åpne) Planning/ }).getAttribute('href')).toBe(
      '/spaces/space%20personal?thread_id=thread%20%2F%20planning#chat',
    )
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

  // Regression coverage: pin used to be two disconnected systems -- this
  // sidebar's `togglePinnedChatThread` only ever touched local storage, while
  // DashboardComposer's own history panel wrote the SAME thread's pin through
  // `saveChatThreadSnapshot` (the server-owned `ChatThreadSession.pinned`).
  // Toggling here without a write-through meant the very next
  // `refreshServerSessions` (any remount) silently reverted it back to
  // whatever the server still had. The fix makes this panel write through too.
  it('persists a pin toggle to the server, not only to local storage', async () => {
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

    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input)
      if (url === '/api/v1/chat/threads' && init?.method !== 'DELETE') {
        return new Response(JSON.stringify({ data: { sessions: [] } }), {
          headers: { 'Content-Type': 'application/json' },
          status: 200,
        })
      }
      if (url === '/api/v1/chat/threads/thread-history-1' && init?.method === 'PUT') {
        return new Response(JSON.stringify({
          data: {
            session: {
              threadId: 'thread-history-1',
              title: 'history visible check',
              preview: 'Assistant reply preview',
              updatedAt: new Date().toISOString(),
              pinned: true,
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

    fireEvent.click(await screen.findByRole('button', { name: /Fest til topp/i }))

    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith(
      '/api/v1/chat/threads/thread-history-1',
      expect.objectContaining({ method: 'PUT', body: JSON.stringify({ pinned: true }) }),
    ))
    expect(await screen.findByRole('button', { name: /Løsne fra topp/i })).toBeTruthy()
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
    flush()

    expect(screen.getByText('Search knowledge base')).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Assistant' })).toBeTruthy()
  })
})
