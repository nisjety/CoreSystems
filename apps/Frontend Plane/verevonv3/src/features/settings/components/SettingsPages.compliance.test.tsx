// @vitest-environment jsdom

import { createRouter, memoryHistory } from '@solidjs/router'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import SettingsPage from '@/features/settings/components/SettingsPage'
import { CoreSidebar } from '@/features/core/components/CoreSidebar'
import { routeFromPath } from '@/features/core/lib/shell-data'
import {
  hasSettingsAccess,
  isComplianceOnlySettingsViewer,
  shouldShowSettingsNavigation,
} from '@/shared/session/access'
import { clearSession, getSession, markSessionOnboardingComplete, setSessionUser } from '@/shared/session/session-store'

// Owner decision after D21i: a compliance member may open workspace settings, and there sees the Trust Center's
// retention and legal holds only. Every section an owner or admin uses (billing, members, security ...) stays closed to
// them, whatever URL they type; the gateway and Control decide each call again.

function signIn(role: string) {
  setSessionUser({ id: 'user_1', email: 'user@example.com', name: 'User', emailVerified: true })
  flush()
  markSessionOnboardingComplete({ id: 'org_1', name: 'Acme', role })
  flush()
}

/** Answers the legal-hold reads; any other call is a 404, and is recorded so a test can assert none was made. */
function stubFetch() {
  const fetchMock = vi.fn(async (input: unknown) => {
    const url = String(input)
    if (url.endsWith('/api/v1/orgs/org_1/legal-holds')) {
      return new Response(JSON.stringify({ data: { holds: [] } }), { headers: { 'Content-Type': 'application/json' } })
    }
    return new Response(JSON.stringify({ error: { code: 'not_found', message: 'no' } }), {
      status: 404,
      headers: { 'Content-Type': 'application/json' },
    })
  })
  vi.stubGlobal('fetch', fetchMock)
  return fetchMock
}

function renderSettingsRoute(path: string) {
  const TestRouter = createRouter({
    routes: [
      { path: '/settings', component: () => <SettingsPage /> },
      { path: '/settings/:section', component: () => <SettingsPage /> },
    ],
    history: memoryHistory(path),
    explicitLinks: true,
  })
  return render(() => <TestRouter>{(props) => <>{props.children}</>}</TestRouter>)
}

afterEach(() => {
  cleanup()
  clearSession()
  vi.unstubAllGlobals()
})

describe('settings access rules (D21i)', () => {
  it('admits an owner, an admin and a compliance member, and narrows compliance alone', () => {
    for (const [role, opens, narrowed] of [
      ['owner', true, false],
      ['admin', true, false],
      ['compliance', true, true],
      ['member', false, false],
      ['viewer', false, false],
    ] as const) {
      signIn(role)
      expect(hasSettingsAccess(getSession()), role).toBe(opens)
      expect(shouldShowSettingsNavigation(getSession()), role).toBe(opens)
      expect(isComplianceOnlySettingsViewer(getSession()), role).toBe(narrowed)
      clearSession()
      flush()
    }
  })
})

describe('a compliance member in workspace settings (D21i)', () => {
  it('lands on the Trust Center whatever section the URL names, and sees the legal holds only', async () => {
    signIn('compliance')
    const fetchMock = stubFetch()
    renderSettingsRoute('/settings/billing')

    expect(await screen.findByRole('heading', { level: 1, name: /trust center|tillitssenter/i })).toBeTruthy()
    expect(await screen.findByText('Rettslig hold')).toBeTruthy()
    // Nothing an admin reads in the Trust Center, and no billing read at all.
    expect(screen.queryByText('Tilkoblede apper')).toBeNull()
    await waitFor(() => expect(fetchMock).toHaveBeenCalled())
    const urls = fetchMock.mock.calls.map(([input]) => String(input))
    expect(urls.some((url) => url.includes('billing'))).toBe(false)
    expect(urls.every((url) => url.includes('/legal-holds'))).toBe(true)
  })

  it('is offered no other section by the settings search', () => {
    signIn('compliance')
    stubFetch()
    renderSettingsRoute('/settings/trust')
    fireEvent.input(screen.getByRole('combobox', { name: /søk i innstillinger/i }), { target: { value: 'fakturering' } })
    flush()
    expect(screen.getByText(/ingen innstillinger samsvarer med søket/i)).toBeTruthy()
  })

  it('sees only the Trust Center in the settings sidebar', () => {
    signIn('compliance')
    stubFetch()
    const TestRouter = createRouter({
      routes: [
        {
          path: '/*all',
          component: () => (
            <CoreSidebar activeRoute={routeFromPath('/settings/trust')} expanded onExpandedChange={vi.fn()} onOpenSearch={vi.fn()} />
          ),
        },
      ],
      history: memoryHistory('/settings/trust'),
      explicitLinks: true,
    })
    render(() => <TestRouter>{(props) => <>{props.children}</>}</TestRouter>)

    const settingsNav = screen.getByRole('navigation', { name: /innstillingsseksjoner/i })
    const links = within(settingsNav).getAllByRole('link')
    expect(links.map((link) => link.getAttribute('href'))).toEqual(['/settings/trust'])
  })

  it('an owner still sees the whole Trust Center', async () => {
    signIn('owner')
    stubFetch()
    renderSettingsRoute('/settings/trust')
    expect(await screen.findByText('Tilkoblede apper')).toBeTruthy()
    expect(await screen.findByText('Rettslig hold')).toBeTruthy()
  })
})
