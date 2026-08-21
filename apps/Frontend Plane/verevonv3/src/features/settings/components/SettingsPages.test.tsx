// @vitest-environment jsdom

import { createRouter, memoryHistory } from '@solidjs/router'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@solidjs/testing-library'
import type { JSX } from '@solidjs/web'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import AccountSettingsPage from '@/features/settings/components/AccountSettingsPage'
import { CoreSidebar } from '@/features/core/components/CoreSidebar'
import { routeFromPath } from '@/features/core/lib/shell-data'
import { VerevonWorkspaceSettingsPage } from '@/features/settings/components/WorkspaceSettingsPage'
import { workspaceSettingsSections } from '@/features/settings/lib/settings-sections'
import { clearSession, markSessionOnboardingComplete, setSessionUser } from '@/shared/session/session-store'

function renderWithRouter(component: () => JSX.Element, path: string) {
  const TestRouter = createRouter({
    routes: [{ path: '/*all', component }],
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

describe('settings profile page', () => {
  it('renders the account settings surface with user-core profile data', async () => {
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input)
      if (url.endsWith('/api/v1/me')) {
        if (init?.method === 'PATCH') {
          return new Response(JSON.stringify({
            user: {
              id: 'user_live',
              email: 'mae@example.com',
              name: 'Mae Jensen',
              display_name: 'Mae Jensen',
              avatar: null,
              email_verified: true,
              status: 'online',
              account_status: 'active',
              position: 'Support lead',
              department: 'Customer operations',
              first_name: 'Mae',
              last_name: 'Jensen',
              phone: '+47 400 00 111',
              location: 'Oslo',
              timezone: 'Europe/Oslo',
              updated_at: '2026-06-14T10:00:00Z',
            },
          }), {
            headers: { 'Content-Type': 'application/json' },
            status: 200,
          })
        }
        return new Response(JSON.stringify({
          user: {
            id: 'user_live',
            email: 'mae@example.com',
            name: 'Mae Jensen',
            display_name: 'Mae Jensen',
            avatar: null,
            email_verified: true,
            status: 'online',
            account_status: 'active',
            position: 'Support lead',
            department: 'Customer operations',
            first_name: 'Mae',
            last_name: 'Jensen',
            phone: '+47 400 00 111',
            location: 'Oslo',
            timezone: 'Europe/Oslo',
          },
        }), {
          headers: { 'Content-Type': 'application/json' },
          status: 200,
        })
      }
      if (url.endsWith('/api/v1/preferences')) {
        if (init?.method === 'PATCH') {
          return new Response(JSON.stringify({ message: 'Preferences updated successfully' }), {
            headers: { 'Content-Type': 'application/json' },
            status: 200,
          })
        }
        return new Response(JSON.stringify({
          preferences: {
            theme: 'dark',
            language: 'nb-NO',
            timezone: 'Europe/Oslo',
            notifications: { email: true, push: false },
          },
        }), {
          headers: { 'Content-Type': 'application/json' },
          status: 200,
        })
      }
      return new Response(JSON.stringify({}), {
        headers: { 'Content-Type': 'application/json' },
        status: 200,
      })
    }))

    render(() => <AccountSettingsPage />)

    expect(screen.getByRole('heading', { name: /profilinnstillinger/i, level: 1 })).toBeTruthy()
    expect(screen.queryByRole('navigation', { name: /settings sections/i })).toBeNull()
    expect(screen.getByTestId('settings-profile-content').className).toContain('verevon-settings-content--account')

    const displayName = screen.getByRole('textbox', { name: /visningsnavn/i }) as HTMLInputElement
    await waitFor(() => expect(displayName.value).toBe('Mae Jensen'))
    expect((screen.getByRole('textbox', { name: /fornavn/i }) as HTMLInputElement).value).toBe('Mae')
    expect((screen.getByRole('textbox', { name: /etternavn/i }) as HTMLInputElement).value).toBe('Jensen')
    expect((screen.getByRole('textbox', { name: /stillingstittel/i }) as HTMLInputElement).value).toBe('Support lead')
    expect((screen.getByRole('textbox', { name: /avdeling/i }) as HTMLInputElement).value).toBe('Customer operations')
    expect((screen.getByRole('textbox', { name: /primær e-post/i }) as HTMLInputElement).value).toBe('mae@example.com')
    expect((screen.getByRole('combobox', { name: /^språk$/i }) as HTMLSelectElement).value).toBe('nb-NO')
    expect((screen.getByRole('combobox', { name: /^tidssone$/i }) as HTMLSelectElement).value).toBe('Europe/Oslo')
    expect((screen.getByRole('combobox', { name: /^tema$/i }) as HTMLSelectElement).value).toBe('dark')
    expect((screen.getByRole('combobox', { name: /^tilgjengelighetsstatus$/i }) as HTMLSelectElement).value).toBe('online')
    expect(screen.getByRole('switch', { name: /e-postvarsler/i }).getAttribute('aria-checked')).toBe('true')
    expect(screen.getByRole('switch', { name: /push-varsler/i }).getAttribute('aria-checked')).toBe('false')
    expect(screen.getByText('Google')).toBeTruthy()
    expect(screen.getByRole('switch', { name: /profilsynlighet/i }).getAttribute('aria-checked')).toBe('true')
    expect(screen.getByText(/lagres på Verevon-kontoen din/i)).toBeTruthy()
    expect(screen.getByRole('button', { name: /lagre profil/i })).toBeTruthy()
    expect(screen.getByRole('heading', { name: /^security$/i })).toBeTruthy()
    expect(screen.queryByRole('heading', { name: /delete account/i })).toBeNull()
  })

  it('includes sticky scroll fade layers for the account settings surface', () => {
    render(() => <AccountSettingsPage />)

    expect(screen.getByTestId('settings-top-scroll-fade').className).toContain('verevon-settings-fade--top')
    expect(screen.getByTestId('settings-bottom-scroll-fade').className).toContain('verevon-settings-fade--bottom')
  })
})

describe('workspace settings page', () => {
  it('renders a single workspace settings page with mocked workspace controls', () => {
    render(() => <VerevonWorkspaceSettingsPage section="workspace" />)

    expect(screen.getByRole('heading', { name: /workspace settings/i, level: 1 })).toBeTruthy()
    expect(screen.queryByRole('heading', { name: /must have/i })).toBeNull()
    expect(screen.queryByText('Workspace name, URL, and primary domain')).toBeNull()
    expect(screen.getByRole('heading', { name: /verifiserte domener/i })).toBeTruthy()
    // De-faked: no fabricated verified domains; honest empty state instead.
    expect(screen.getByText(/ingen domener er verifisert/i)).toBeTruthy()
    expect(screen.queryByText('support.coresystem.no')).toBeNull()
    expect(screen.getByRole('button', { name: /rediger tidsplan/i })).toBeTruthy()
    expect((screen.getByRole('textbox', { name: /arbeidsområdenavn/i }) as HTMLInputElement).value).toBe('coresystem-as')
    expect(screen.queryByRole('textbox', { name: /invite by email/i })).toBeNull()
  })

  it('renders the members page as its own route-level section', () => {
    render(() => <VerevonWorkspaceSettingsPage section="members" />)

    expect(screen.getByRole('heading', { name: /members & roles/i, level: 1 })).toBeTruthy()
    expect(screen.getByRole('textbox', { name: /inviter via e-post/i })).toBeTruthy()
    expect(screen.getByRole('heading', { name: /innebygde roller/i })).toBeTruthy()
    expect(screen.queryByRole('button', { name: /create role/i })).toBeNull()
    expect(screen.queryByText('Agent')).toBeNull()
    expect(screen.queryByRole('textbox', { name: /workspace name/i })).toBeNull()
  })

  it('invites, changes roles, and removes members through the canonical Auth contract', async () => {
    setSessionUser({
      id: 'owner_1',
      email: 'owner@example.com',
      name: 'Owner',
      emailVerified: true,
    })
    flush()
    markSessionOnboardingComplete({ id: 'org_1', name: 'Acme', role: 'owner' })
    flush()
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path === '/api/v1/orgs/org_1/members' && !init?.method) {
        return new Response(JSON.stringify({
          members: [{
            id: 'membership_2',
            userId: 'user_2',
            role: 'member',
            user: { name: 'Teammate', email: 'teammate@example.com' },
          }],
        }), { headers: { 'Content-Type': 'application/json' }, status: 200 })
      }
      return new Response(JSON.stringify({ success: true }), {
        headers: { 'Content-Type': 'application/json' },
        status: 200,
      })
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => <VerevonWorkspaceSettingsPage section="members" />)
    expect(await screen.findByText('teammate@example.com')).toBeTruthy()

    fireEvent.input(screen.getByRole('textbox', { name: /inviter via e-post/i }), {
      target: { value: 'new@example.com' },
    })
    flush()
    fireEvent.change(screen.getByRole('combobox', { name: /^rolle$/i }), {
      target: { value: 'admin' },
    })
    flush()
    fireEvent.click(screen.getByRole('button', { name: /inviter medlem/i }))
    await waitFor(() => {
      const call = fetchMock.mock.calls.find(([path]) => path === '/api/v1/orgs/org_1/members/invite')
      expect(JSON.parse(String(call?.[1]?.body))).toEqual({ email: 'new@example.com', role: 'admin' })
    })

    await waitFor(() => expect(
      (screen.getByRole('combobox', { name: /rolle for teammate/i }) as HTMLSelectElement).disabled,
    ).toBe(false))
    fireEvent.change(screen.getByRole('combobox', { name: /rolle for teammate/i }), {
      target: { value: 'admin' },
    })
    flush()
    await waitFor(() => {
      expect(fetchMock.mock.calls.some(([path]) => path === '/api/v1/orgs/org_1/members/user_2/role')).toBe(true)
    })

    await waitFor(() => expect(
      (screen.getByRole('button', { name: /fjern teammate/i }) as HTMLButtonElement).disabled,
    ).toBe(false))
    fireEvent.click(screen.getByRole('button', { name: /fjern teammate/i }))
    flush()
    fireEvent.click(screen.getByRole('button', { name: /bekreft fjerning av teammate/i }))
    await waitFor(() => {
      expect(fetchMock.mock.calls.some(([path]) => path === '/api/v1/orgs/org_1/members/user_2')).toBe(true)
    })
  })

  it('reconciles a false-failure 502 by checking whether the invite was actually recorded', async () => {
    setSessionUser({
      id: 'owner_1',
      email: 'owner@example.com',
      name: 'Owner',
      emailVerified: true,
    })
    flush()
    markSessionOnboardingComplete({ id: 'org_1', name: 'Acme', role: 'owner' })
    flush()
    let invited = false
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path === '/api/v1/orgs/org_1/members' && !init?.method) {
        return new Response(JSON.stringify({
          members: invited
            ? [{ id: 'membership_new', userId: 'user_new', role: 'admin', status: 'invited', user: { email: 'new@example.com' } }]
            : [],
        }), { headers: { 'Content-Type': 'application/json' }, status: 200 })
      }
      if (path === '/api/v1/orgs/org_1/members/invite' && init?.method === 'POST') {
        // The invite reaches the backend and is durably recorded, but the
        // response itself is lost to a transient gateway error.
        invited = true
        return new Response(JSON.stringify({ message: 'Bad Gateway' }), {
          headers: { 'Content-Type': 'application/json' },
          status: 502,
        })
      }
      return new Response(JSON.stringify({ success: true }), {
        headers: { 'Content-Type': 'application/json' },
        status: 200,
      })
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => <VerevonWorkspaceSettingsPage section="members" />)
    fireEvent.input(screen.getByRole('textbox', { name: /inviter via e-post/i }), {
      target: { value: 'new@example.com' },
    })
    flush()
    fireEvent.click(screen.getByRole('button', { name: /inviter medlem/i }))

    expect(await screen.findAllByText('new@example.com')).not.toHaveLength(0)
    expect(screen.queryByRole('alert')).toBeNull()
    expect((screen.getByRole('textbox', { name: /inviter via e-post/i }) as HTMLInputElement).value).toBe('')
  })

  it('shows a real failure when inviting a member genuinely did not go through', async () => {
    setSessionUser({
      id: 'owner_1',
      email: 'owner@example.com',
      name: 'Owner',
      emailVerified: true,
    })
    flush()
    markSessionOnboardingComplete({ id: 'org_1', name: 'Acme', role: 'owner' })
    flush()
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path === '/api/v1/orgs/org_1/members' && !init?.method) {
        return new Response(JSON.stringify({ members: [] }), { headers: { 'Content-Type': 'application/json' }, status: 200 })
      }
      if (path === '/api/v1/orgs/org_1/members/invite' && init?.method === 'POST') {
        return new Response(JSON.stringify({ message: 'Bad Gateway' }), {
          headers: { 'Content-Type': 'application/json' },
          status: 502,
        })
      }
      return new Response(JSON.stringify({ success: true }), {
        headers: { 'Content-Type': 'application/json' },
        status: 200,
      })
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => <VerevonWorkspaceSettingsPage section="members" />)
    fireEvent.input(screen.getByRole('textbox', { name: /inviter via e-post/i }), {
      target: { value: 'new@example.com' },
    })
    fireEvent.click(screen.getByRole('button', { name: /inviter medlem/i }))

    expect(await screen.findByRole('alert')).toBeTruthy()
    expect(screen.queryByText('new@example.com')).toBeNull()
    expect((screen.getByRole('textbox', { name: /inviter via e-post/i }) as HTMLInputElement).value).toBe('new@example.com')
  })

  it('reconciles a false-failure 502 by checking whether the member was actually removed', async () => {
    setSessionUser({
      id: 'owner_1',
      email: 'owner@example.com',
      name: 'Owner',
      emailVerified: true,
    })
    flush()
    markSessionOnboardingComplete({ id: 'org_1', name: 'Acme', role: 'owner' })
    flush()
    let removed = false
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path === '/api/v1/orgs/org_1/members' && !init?.method) {
        return new Response(JSON.stringify({
          members: removed
            ? []
            : [{ id: 'membership_2', userId: 'user_2', role: 'member', user: { name: 'Teammate', email: 'teammate@example.com' } }],
        }), { headers: { 'Content-Type': 'application/json' }, status: 200 })
      }
      if (path === '/api/v1/orgs/org_1/members/user_2' && init?.method === 'DELETE') {
        // The removal reaches the backend and is durably recorded, but the
        // response itself is lost to a transient gateway error.
        removed = true
        return new Response(JSON.stringify({ message: 'Bad Gateway' }), {
          headers: { 'Content-Type': 'application/json' },
          status: 502,
        })
      }
      return new Response(JSON.stringify({ success: true }), {
        headers: { 'Content-Type': 'application/json' },
        status: 200,
      })
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => <VerevonWorkspaceSettingsPage section="members" />)
    expect(await screen.findByText('teammate@example.com')).toBeTruthy()

    await waitFor(() => expect(
      (screen.getByRole('button', { name: /fjern teammate/i }) as HTMLButtonElement).disabled,
    ).toBe(false))
    fireEvent.click(screen.getByRole('button', { name: /fjern teammate/i }))
    flush()
    fireEvent.click(screen.getByRole('button', { name: /bekreft fjerning av teammate/i }))

    await waitFor(() => expect(screen.queryByText('teammate@example.com')).toBeNull())
    expect(screen.queryByRole('alert')).toBeNull()
  })

  it('shows a real failure when removing a member genuinely did not go through', async () => {
    setSessionUser({
      id: 'owner_1',
      email: 'owner@example.com',
      name: 'Owner',
      emailVerified: true,
    })
    flush()
    markSessionOnboardingComplete({ id: 'org_1', name: 'Acme', role: 'owner' })
    flush()
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      if (path === '/api/v1/orgs/org_1/members' && !init?.method) {
        return new Response(JSON.stringify({
          members: [{ id: 'membership_2', userId: 'user_2', role: 'member', user: { name: 'Teammate', email: 'teammate@example.com' } }],
        }), { headers: { 'Content-Type': 'application/json' }, status: 200 })
      }
      if (path === '/api/v1/orgs/org_1/members/user_2' && init?.method === 'DELETE') {
        return new Response(JSON.stringify({ message: 'Bad Gateway' }), {
          headers: { 'Content-Type': 'application/json' },
          status: 502,
        })
      }
      return new Response(JSON.stringify({ success: true }), {
        headers: { 'Content-Type': 'application/json' },
        status: 200,
      })
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => <VerevonWorkspaceSettingsPage section="members" />)
    expect(await screen.findByText('teammate@example.com')).toBeTruthy()

    await waitFor(() => expect(
      (screen.getByRole('button', { name: /fjern teammate/i }) as HTMLButtonElement).disabled,
    ).toBe(false))
    fireEvent.click(screen.getByRole('button', { name: /fjern teammate/i }))
    flush()
    fireEvent.click(screen.getByRole('button', { name: /bekreft fjerning av teammate/i }))

    expect(await screen.findByRole('alert')).toBeTruthy()
    expect(screen.getByText('teammate@example.com')).toBeTruthy()
  })

  it('renders org-security controls and live Audit Core security events', async () => {
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      if (String(input).startsWith('/api/v1/audit')) {
        return new Response(JSON.stringify({
          data: [{
            id: 17,
            event: 'member.removed',
            occurred_at: '2026-07-14T18:00:00Z',
            user_id: 'user_admin',
            actor_role: 'admin',
            resource_id: 'user_member',
            outcome: 'ok',
            request_id: 'req_audit_17',
          }],
          meta: { count: 1 },
          error: null,
        }), {
          headers: { 'Content-Type': 'application/json' },
          status: 200,
        })
      }
      return new Response(JSON.stringify({}), {
        headers: { 'Content-Type': 'application/json' },
        status: 200,
      })
    }))

    render(() => <VerevonWorkspaceSettingsPage section="org-security" />)

    expect(screen.getByRole('heading', { name: /org security/i, level: 1 })).toBeTruthy()
    // Phase 4 PR-2 de-fake: no real org-security source is wired behind the
    // gateway, so the MFA control must render OFF and DISABLED — never shown
    // enabled from a literal (the prior `enabled: true` was a fabricated posture).
    const mfa = screen.getByRole('switch', { name: /krev mfa for administratorer/i })
    expect(mfa.getAttribute('aria-checked')).toBe('false')
    expect(
      mfa.hasAttribute('disabled')
      || mfa.getAttribute('aria-disabled') === 'true'
      || mfa.getAttribute('data-disabled') !== null,
    ).toBe(true)
    expect(screen.getByRole('combobox', { name: /øktvarighet/i })).toBeTruthy()
    expect(screen.getByRole('heading', { name: /nylige sikkerhetshendelser/i })).toBeTruthy()
    expect(await screen.findByText('member.removed - ok')).toBeTruthy()
    expect(screen.getByText('user_admin · admin · req_audit_17')).toBeTruthy()
  })

  it('defines one route-level page for every settings sidebar tab', () => {
    for (const section of workspaceSettingsSections) {
      expect(section.id).toBeTruthy()
      expect(section.title).toBeTruthy()
      expect(section.description).toBeTruthy()
      // Every tab must carry a non-empty save-action label. (Language-agnostic:
      // the product is mid-i18n migration — e.g. the MCP section ships a
      // Norwegian "Lagre …" label — so do not assume an English "Save " prefix.)
      expect(section.saveLabel.length).toBeGreaterThan(0)
    }
  })

  it('renders the social integration layer from integration-core providers', async () => {
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input)
      if (url.endsWith('/api/v1/integrations/providers')) {
        return new Response(JSON.stringify({
          providers: [
            {
              key: 'facebook',
              label: 'Facebook',
              category: 'social',
              configured: true,
              status: 'ready',
              missingConfig: [],
              directOAuthReady: true,
              capabilities: [
                { key: 'social.profile.read' },
                { key: 'social.post.write' },
                { key: 'social.inbox.read' },
              ],
            },
            {
              key: 'snapchat',
              label: 'Snapchat',
              category: 'social',
              configured: true,
              status: 'ready',
              missingConfig: [],
              directOAuthReady: true,
              capabilities: [
                { key: 'social.profile.read' },
                { key: 'social.ads.manage' },
                { key: 'social.analytics.read' },
              ],
            },
            {
              key: 'google-drive',
              label: 'Google Drive',
              category: 'documents',
              configured: true,
              status: 'ready',
              missingConfig: [],
              directOAuthReady: true,
              capabilities: [{ key: 'documents.read' }],
            },
          ],
        }), {
          headers: { 'Content-Type': 'application/json' },
          status: 200,
        })
      }
      if (url.endsWith('/api/v1/integrations/connections')) {
        return new Response(JSON.stringify({
          connections: [
            {
              id: 'conn_facebook',
              providerKey: 'facebook',
              providerLabel: 'Facebook',
              displayName: 'Verevon Page',
              status: 'connected',
              capabilities: ['social.profile.read', 'social.post.write', 'social.inbox.read'],
              scopeCount: 4,
              syncStatus: 'idle',
            },
          ],
        }), {
          headers: { 'Content-Type': 'application/json' },
          status: 200,
        })
      }
      return new Response(JSON.stringify({}), {
        headers: { 'Content-Type': 'application/json' },
        status: 200,
      })
    }))

    render(() => <VerevonWorkspaceSettingsPage section="integrations" />)

    expect(await screen.findByRole('heading', { name: /publisering, innboks og kampanjeadaptere/i })).toBeTruthy()
    expect((await screen.findAllByText('Facebook')).length).toBeGreaterThan(0)
    expect((await screen.findAllByText('Snapchat')).length).toBeGreaterThan(0)
    expect(screen.getByText(/Sidepublisering, kommentarer, innboks og analyse/i)).toBeTruthy()
    expect(screen.getByText(/Annonser, kreativt innhold, kampanje- og rapporteringsarbeidsflyt/i)).toBeTruthy()
    expect(screen.getByText('klar for publisering')).toBeTruthy()
    expect(screen.getByRole('link', { name: /åpne kalender/i }).getAttribute('href')).toBe('/social/calendar')
  })

  it('does not present soft-deleted provider connections as connected', async () => {
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input)
      if (url.endsWith('/api/v1/integrations/providers')) {
        return new Response(JSON.stringify({
          providers: [{
            key: 'slack',
            label: 'Slack',
            category: 'communications',
            configured: true,
            status: 'ready',
            missingConfig: [],
            directOAuthReady: true,
            capabilities: [{ key: 'channels.history' }, { key: 'message.send' }],
          }],
        }), { headers: { 'Content-Type': 'application/json' }, status: 200 })
      }
      if (url.endsWith('/api/v1/integrations/connections')) {
        return new Response(JSON.stringify({
          connections: [{
            id: 'conn_slack_deleted',
            providerKey: 'slack',
            displayName: 'Former Slack workspace',
            status: 'active',
            capabilities: ['channels.history', 'message.send'],
            deletedAt: '2026-07-18T09:00:00Z',
          }],
        }), { headers: { 'Content-Type': 'application/json' }, status: 200 })
      }
      return new Response(JSON.stringify({}), {
        headers: { 'Content-Type': 'application/json' },
        status: 200,
      })
    }))

    render(() => <VerevonWorkspaceSettingsPage section="integrations" />)

    expect(await screen.findByText('Slack')).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Koble til' })).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Koble fra' })).toBeNull()
  })

  it('shows provider-verified incomplete Meta authorization as needing reconnect', async () => {
	vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
	  const url = String(input)
	  if (url.endsWith('/api/v1/integrations/providers')) {
		return new Response(JSON.stringify({ providers: [{
		  key: 'meta', label: 'Meta', category: 'social', configured: true,
		  status: 'ready', missingConfig: [], directOAuthReady: true,
		  capabilities: [{ key: 'social.inbox.read' }],
		}] }), { headers: { 'Content-Type': 'application/json' }, status: 200 })
	  }
	  if (url.endsWith('/api/v1/integrations/connections')) {
		return new Response(JSON.stringify({ connections: [{
		  id: 'conn_meta', providerKey: 'meta', displayName: 'Ima DaCosta',
		  status: 'needs_refresh', capabilities: [], scopes: ['public_profile'],
		  lastSyncStatus: 'authorization_incomplete',
		}] }), { headers: { 'Content-Type': 'application/json' }, status: 200 })
	  }
	  return new Response(JSON.stringify({}), { headers: { 'Content-Type': 'application/json' }, status: 200 })
	}))

	render(() => <VerevonWorkspaceSettingsPage section="integrations" />)

	const attentionMetric = (await screen.findByText('Krever oppmerksomhet')).parentElement
	expect(attentionMetric).toBeTruthy()
	expect(within(attentionMetric as HTMLElement).getByText('1')).toBeTruthy()
	expect(screen.getAllByText('Krever ny tilkobling')).toHaveLength(2)
	expect(screen.getAllByRole('button', { name: 'Koble til på nytt' })).toHaveLength(2)
  })

  it('folds a connected standalone Instagram inbox into the unified Meta card', async () => {
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input)
      if (url.endsWith('/api/v1/integrations/providers')) {
        return new Response(JSON.stringify({ providers: [
          {
            key: 'meta', label: 'Meta', category: 'social', configured: true,
            status: 'ready', missingConfig: [], directOAuthReady: true,
            capabilities: [{ key: 'social.messenger.manage' }],
          },
          {
            key: 'instagram', label: 'Instagram', category: 'social', configured: true,
            status: 'ready', missingConfig: [], directOAuthReady: true, supersededBy: 'meta',
            capabilities: [{ key: 'social.inbox.read' }],
          },
        ] }), { headers: { 'Content-Type': 'application/json' }, status: 200 })
      }
      if (url.endsWith('/api/v1/integrations/connections')) {
        return new Response(JSON.stringify({ connections: [
          {
            id: 'conn_meta', providerKey: 'meta', displayName: 'Meta workspace',
            status: 'active', capabilities: ['social.messenger.manage'], scopes: ['pages_messaging'],
          },
          {
            id: 'conn_instagram', providerKey: 'instagram', displayName: 'Instagram professional account',
            status: 'active', capabilities: ['social.inbox.read'], scopes: ['instagram_business_manage_messages'],
          },
        ] }), { headers: { 'Content-Type': 'application/json' }, status: 200 })
      }
      return new Response(JSON.stringify({}), { headers: { 'Content-Type': 'application/json' }, status: 200 })
    }))

    const view = render(() => <VerevonWorkspaceSettingsPage section="integrations" />)

    await screen.findAllByText(/Meta workspace/)
    const socialCards = [...view.container.querySelectorAll('.verevon-settings-social-provider')]
    const integrationRows = [...view.container.querySelectorAll('.verevon-settings-integration-row')]
    expect(socialCards.some((card) => card.textContent?.includes('Instagram professional account'))).toBe(false)
    expect(integrationRows.some((row) => row.textContent?.includes('Instagram professional account'))).toBe(false)
    expect(screen.queryByRole('button', { name: /enable instagram inbox|aktiver instagram-innboks/i })).toBeNull()
  })

  it('starts a connected provider sync through its canonical connection route', async () => {
    setSessionUser({
      id: 'owner_google',
      email: 'owner@example.com',
      name: 'Owner',
      emailVerified: true,
    })
    flush()
    markSessionOnboardingComplete({ id: 'org_google', name: 'Google org', role: 'owner' })
    flush()
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      void init
      const url = String(input)
      if (url.endsWith('/api/v1/integrations/providers')) {
        return new Response(JSON.stringify({ providers: [{
          key: 'google', label: 'Google Workspace', category: 'communications', configured: true,
          status: 'ready', missingConfig: [], directOAuthReady: true,
          capabilities: [{ key: 'gmail.read', direction: 'read' }, { key: 'gmail.send', direction: 'write' }],
        }] }), { headers: { 'Content-Type': 'application/json' }, status: 200 })
      }
      if (url.endsWith('/api/v1/integrations/connections')) {
        return new Response(JSON.stringify({ connections: [{
          id: 'conn_google', providerKey: 'google', displayName: 'Ima Dacosta', status: 'active',
          capabilities: ['gmail.read', 'gmail.send'], syncStatus: 'idle',
        }] }), { headers: { 'Content-Type': 'application/json' }, status: 200 })
      }
      if (url.endsWith('/api/v1/integrations/connections/conn_google/sync')) {
        return new Response(JSON.stringify({ data: { syncJob: { status: 'queued' } } }), {
          headers: { 'Content-Type': 'application/json' }, status: 202,
        })
      }
      return new Response(JSON.stringify({ error: { code: 'method_not_allowed', message: 'Request failed (405)' } }), {
        headers: { 'Content-Type': 'application/json' }, status: 405,
      })
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => <VerevonWorkspaceSettingsPage section="integrations" />)

    fireEvent.click(await screen.findByRole('button', { name: 'Synkroniser' }))

    await waitFor(() => expect(fetchMock.mock.calls.some(([input, init]) =>
      String(input).endsWith('/api/v1/integrations/connections/conn_google/sync')
      && init?.method === 'POST',
    )).toBe(true))
    expect(fetchMock.mock.calls.some(([input, init]) =>
      String(input).endsWith('/api/v1/integrations/sync-jobs')
      && init?.method === 'POST',
    )).toBe(false)
  })

  it('renders every connected account and offers another-account authorization', async () => {
    setSessionUser({
      id: 'owner_multi',
      email: 'owner@example.com',
      name: 'Owner',
      emailVerified: true,
    })
    flush()
    markSessionOnboardingComplete({ id: 'org_multi', name: 'Multi account org', role: 'owner' })
    flush()

    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input)
      if (url.endsWith('/api/v1/integrations/providers')) {
        return new Response(JSON.stringify({ providers: [{
          key: 'google', label: 'Google Workspace', category: 'communications', configured: true,
          status: 'ready', missingConfig: [], directOAuthReady: true,
          capabilities: [{ key: 'gmail.read', direction: 'read' }],
        }] }), { headers: { 'Content-Type': 'application/json' }, status: 200 })
      }
      if (url.endsWith('/api/v1/integrations/connections')) {
        return new Response(JSON.stringify({ connections: [
          {
            id: 'conn_google_a', providerKey: 'google', displayName: 'Ima DaCosta',
            providerEmail: 'ima@example.com', providerAccountId: 'google-a',
            status: 'active', capabilities: ['gmail.read'], syncStatus: 'idle',
          },
          {
            id: 'conn_google_b', providerKey: 'google', displayName: 'Ima DaCosta',
            providerEmail: 'second@example.com', providerAccountId: 'google-b',
            status: 'active', capabilities: ['gmail.read'], syncStatus: 'idle',
          },
        ] }), { headers: { 'Content-Type': 'application/json' }, status: 200 })
      }
      return new Response(JSON.stringify({}), { headers: { 'Content-Type': 'application/json' }, status: 200 })
    }))

    render(() => <VerevonWorkspaceSettingsPage section="integrations" />)

    // Each connection renders as its own provider row whose detail line is one
    // combined string (email · display name · account id · …), not an isolated
    // email-only text node. Assert per row so this also proves the two accounts
    // are two separate Google Workspace rows, not one row mentioning both.
    const workspaceRows = await screen.findAllByText('Google Workspace')
    const imaRow = workspaceRows.find((row) => row.closest('.verevon-settings-integration-row')?.textContent?.includes('ima@example.com') ?? false)
    expect(imaRow).toBeTruthy()
    expect(workspaceRows.find((row) => row.closest('.verevon-settings-integration-row')?.textContent?.includes('second@example.com'))).toBeTruthy()
    expect(screen.getAllByRole('button', { name: 'Koble til annen konto' })).toHaveLength(2)
  })

  it('clears connection state and reloads integrations when the active organization changes', async () => {
    setSessionUser({
      id: 'owner_1',
      email: 'owner@example.com',
      name: 'Owner',
      emailVerified: true,
    })
    flush()
    markSessionOnboardingComplete({ id: 'org_one', name: 'One', role: 'owner' })
    flush()

    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input)
      if (url.endsWith('/api/v1/integrations/providers')) {
        return new Response(JSON.stringify({
          providers: [{
            key: 'slack',
            label: 'Slack',
            category: 'communications',
            configured: true,
            status: 'ready',
            missingConfig: [],
            directOAuthReady: true,
            capabilities: [{ key: 'channels.history' }, { key: 'message.send' }],
          }],
        }), { headers: { 'Content-Type': 'application/json' }, status: 200 })
      }
      if (url.endsWith('/api/v1/integrations/connections')) {
        const activeOrg = new Headers(init?.headers).get('x-verevon-org-id')
        return new Response(JSON.stringify({
          connections: activeOrg === 'org_one'
            ? [{
              id: 'conn_slack_one',
              providerKey: 'slack',
              displayName: 'Workspace One Slack',
              status: 'active',
              capabilities: ['channels.history', 'message.send'],
            }]
            : [],
        }), { headers: { 'Content-Type': 'application/json' }, status: 200 })
      }
      return new Response(JSON.stringify({}), {
        headers: { 'Content-Type': 'application/json' },
        status: 200,
      })
    }))

    render(() => <VerevonWorkspaceSettingsPage section="integrations" />)

    expect(await screen.findByRole('button', { name: 'Koble fra' })).toBeTruthy()

    markSessionOnboardingComplete({ id: 'org_two', name: 'Two', role: 'owner' })

    await waitFor(() => expect(screen.getByRole('button', { name: 'Koble til' })).toBeTruthy())
    expect(screen.queryByRole('button', { name: 'Koble fra' })).toBeNull()
  })

  it('renders billing with live account data and prefers embedded Nexi checkout over its hosted URL', async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input)
      if (url.endsWith('/api/v1/billing/account')) {
        return new Response(JSON.stringify({
          org_id: 'org_settings',
          plan: 'trial',
          subscription_state: 'trialing',
          credits: 250,
          quota_limits: { users: 5, api_calls: 1000 },
        }), {
          headers: { 'Content-Type': 'application/json' },
          status: 200,
        })
      }
      if (url.endsWith('/api/v1/billing/checkout')) {
        return new Response(JSON.stringify({
          provider: 'nexi',
          id: 'pay_settings_123',
          payment_id: 'pay_settings_123',
          publishable_key: 'checkout_test_settings',
          client_url: 'https://test.checkout.dibspayment.eu/v1/checkout.js?v=1',
          url: 'https://test.checkout.dibspayment.eu/payments/pay_settings_123',
          status: 'created',
          amount_cents: 99900,
          currency: 'NOK',
        }), {
          headers: { 'Content-Type': 'application/json' },
          status: 201,
        })
      }
      return new Response(JSON.stringify({}), {
        headers: { 'Content-Type': 'application/json' },
        status: 200,
      })
    })
    vi.stubGlobal('fetch', fetchMock)

    const { unmount } = render(() => <VerevonWorkspaceSettingsPage section="billing" />)

    expect(screen.getByRole('heading', { name: /billing/i, level: 1 })).toBeTruthy()
    expect((await screen.findAllByText(/trialing/i)).length).toBeGreaterThan(0)
    expect(screen.getByRole('heading', { name: /velg plan/i })).toBeTruthy()
    expect(screen.getByRole('button', { name: /aktiver advanced/i })).toBeTruthy()
    expect(screen.queryByText('Current plan, renewal date, and upgrade path')).toBeNull()

    fireEvent.click(screen.getByRole('button', { name: /aktiver advanced/i }))

    expect(await screen.findByRole('region', { name: /payment checkout/i })).toBeTruthy()
    expect(fetchMock).toHaveBeenCalledWith(expect.stringMatching('/api/v1/billing/checkout$'), expect.objectContaining({
      method: 'POST',
    }))

    unmount()
    render(() => <VerevonWorkspaceSettingsPage section="sso" />)

    expect(screen.getByRole('heading', { name: /sso/i, level: 1 })).toBeTruthy()
    // De-faked: SSO is honestly "Not configured" — the fabricated provider form,
    // connection test, and attribute mapping panels were removed.
    expect(screen.getByText(/ikke konfigurert for denne organisasjonen/i)).toBeTruthy()
    expect(screen.queryByRole('heading', { name: /connection test/i })).toBeNull()
    expect(screen.queryByRole('heading', { name: /attribute mapping/i })).toBeNull()
    expect(screen.queryByText('SSO provider selection and verified domain')).toBeNull()
  })
})

describe('settings sidebars', () => {
  it('renders profile anchors in the account sidebar', () => {
    renderWithRouter(() => (
      <CoreSidebar
        activeRoute="/account"
        expanded
        onExpandedChange={vi.fn()}
        onOpenSearch={vi.fn()}
      />
    ), '/account')

    const accountNav = screen.getByRole('navigation', { name: /kontoseksjoner/i })
    expect(within(accountNav).getByRole('link', { name: /^profil$/i }).getAttribute('href')).toBe('#profile')
    expect(within(accountNav).getByRole('link', { name: /^kontakt$/i }).getAttribute('href')).toBe('#contact')
    expect(within(accountNav).getByRole('link', { name: /^preferanser$/i }).getAttribute('href')).toBe('#preferences')
    expect(within(accountNav).getByRole('link', { name: /^tilgjengelighet$/i }).getAttribute('href')).toBe('#availability')
    expect(within(accountNav).getByRole('link', { name: /^tilkoblede kontoer$/i }).getAttribute('href')).toBe('#connected-accounts')
    expect(within(accountNav).getByRole('link', { name: /^personvern$/i }).getAttribute('href')).toBe('#privacy')
  })

  it('renders workspace settings routes in the settings sidebar', () => {
    renderWithRouter(() => (
      <CoreSidebar
        activeRoute={routeFromPath('/settings/sso')}
        expanded
        onExpandedChange={vi.fn()}
        onOpenSearch={vi.fn()}
      />
    ), '/settings/sso')

    const settingsNav = screen.getByRole('navigation', { name: /innstillingsseksjoner/i })
    expect(within(settingsNav).getByRole('link', { name: /^arbeidsområde$/i }).getAttribute('href')).toBe('/settings/workspace')
    expect(within(settingsNav).getByRole('link', { name: /^medlemmer og roller$/i }).getAttribute('href')).toBe('/settings/members')
    expect(within(settingsNav).getByRole('link', { name: /^fakturering$/i }).getAttribute('href')).toBe('/settings/billing')
    expect(within(settingsNav).getByRole('link', { name: /^sso$/i }).getAttribute('href')).toBe('/settings/sso')
    expect(within(settingsNav).getByRole('link', { name: /^organisasjonssikkerhet$/i }).getAttribute('href')).toBe('/settings/org-security')
    expect(within(settingsNav).getByRole('link', { name: /^integrasjoner$/i }).getAttribute('href')).toBe('/settings/integrations')
  })
})
