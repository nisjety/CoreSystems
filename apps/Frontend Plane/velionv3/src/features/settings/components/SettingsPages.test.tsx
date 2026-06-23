// @vitest-environment jsdom

import { Route, Router } from '@solidjs/router'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@solidjs/testing-library'
import type { JSX } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import AccountSettingsPage from '@/features/settings/components/AccountSettingsPage'
import { CoreSidebar } from '@/features/core/components/CoreSidebar'
import { routeFromPath } from '@/features/core/lib/shell-data'
import { VelionWorkspaceSettingsPage } from '@/features/settings/components/WorkspaceSettingsPage'
import { workspaceSettingsSections } from '@/features/settings/lib/settings-sections'

function renderWithRouter(component: () => JSX.Element, path: string) {
  window.history.pushState(null, '', path)
  return render(() => (
    <Router root={(props) => <>{props.children}</>}>
      <Route path="/*all" component={component} />
    </Router>
  ))
}

afterEach(() => {
  cleanup()
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

    expect(screen.getByRole('heading', { name: /profile settings/i, level: 1 })).toBeTruthy()
    expect(screen.queryByRole('navigation', { name: /settings sections/i })).toBeNull()
    expect(screen.getByTestId('settings-profile-content').className).toContain('velion-settings-content--account')

    const displayName = screen.getByRole('textbox', { name: /display name/i }) as HTMLInputElement
    await waitFor(() => expect(displayName.value).toBe('Mae Jensen'))
    expect((screen.getByRole('textbox', { name: /first name/i }) as HTMLInputElement).value).toBe('Mae')
    expect((screen.getByRole('textbox', { name: /last name/i }) as HTMLInputElement).value).toBe('Jensen')
    expect((screen.getByRole('textbox', { name: /job title/i }) as HTMLInputElement).value).toBe('Support lead')
    expect((screen.getByRole('textbox', { name: /department/i }) as HTMLInputElement).value).toBe('Customer operations')
    expect((screen.getByRole('textbox', { name: /primary email/i }) as HTMLInputElement).value).toBe('mae@example.com')
    expect((screen.getByRole('combobox', { name: /^language$/i }) as HTMLSelectElement).value).toBe('nb-NO')
    expect((screen.getByRole('combobox', { name: /^time zone$/i }) as HTMLSelectElement).value).toBe('Europe/Oslo')
    expect((screen.getByRole('combobox', { name: /^theme$/i }) as HTMLSelectElement).value).toBe('dark')
    expect((screen.getByRole('combobox', { name: /^availability status$/i }) as HTMLSelectElement).value).toBe('online')
    expect(screen.getByRole('switch', { name: /email notifications/i }).getAttribute('aria-checked')).toBe('true')
    expect(screen.getByRole('switch', { name: /push notifications/i }).getAttribute('aria-checked')).toBe('false')
    expect(screen.getByText('Google')).toBeTruthy()
    expect(screen.getByRole('switch', { name: /profile visibility/i }).getAttribute('aria-checked')).toBe('true')
    expect(screen.getByText(/saved to your Velion account/i)).toBeTruthy()
    expect(screen.getByRole('button', { name: /save profile/i })).toBeTruthy()
    expect(screen.getByRole('heading', { name: /^security$/i })).toBeTruthy()
    expect(screen.queryByRole('heading', { name: /delete account/i })).toBeNull()
  })

  it('includes sticky scroll fade layers for the account settings surface', () => {
    render(() => <AccountSettingsPage />)

    expect(screen.getByTestId('settings-top-scroll-fade').className).toContain('velion-settings-fade--top')
    expect(screen.getByTestId('settings-bottom-scroll-fade').className).toContain('velion-settings-fade--bottom')
  })
})

describe('workspace settings page', () => {
  it('renders a single workspace settings page with mocked workspace controls', () => {
    render(() => <VelionWorkspaceSettingsPage section="workspace" />)

    expect(screen.getByRole('heading', { name: /workspace settings/i, level: 1 })).toBeTruthy()
    expect(screen.queryByRole('heading', { name: /must have/i })).toBeNull()
    expect(screen.queryByText('Workspace name, URL, and primary domain')).toBeNull()
    expect(screen.getByRole('heading', { name: /verified domains/i })).toBeTruthy()
    // De-faked: no fabricated verified domains; honest empty state instead.
    expect(screen.getByText(/no domains have been verified/i)).toBeTruthy()
    expect(screen.queryByText('support.aquatiq.no')).toBeNull()
    expect(screen.getByRole('button', { name: /edit schedule/i })).toBeTruthy()
    expect((screen.getByRole('textbox', { name: /workspace name/i }) as HTMLInputElement).value).toBe('aquatiq-as')
    expect(screen.queryByRole('textbox', { name: /invite by email/i })).toBeNull()
  })

  it('renders the members page as its own route-level section', () => {
    render(() => <VelionWorkspaceSettingsPage section="members" />)

    expect(screen.getByRole('heading', { name: /members & roles/i, level: 1 })).toBeTruthy()
    expect(screen.getByRole('textbox', { name: /invite by email/i })).toBeTruthy()
    expect(screen.getByRole('heading', { name: /role templates/i })).toBeTruthy()
    expect(screen.getByText('Full workspace, billing, and security')).toBeTruthy()
    expect(screen.queryByRole('textbox', { name: /workspace name/i })).toBeNull()
  })

  it('renders org-security controls as honest unconfigured, never enabled from a literal', () => {
    render(() => <VelionWorkspaceSettingsPage section="org-security" />)

    expect(screen.getByRole('heading', { name: /org security/i, level: 1 })).toBeTruthy()
    // Phase 4 PR-2 de-fake: no real org-security source is wired behind the
    // gateway, so the MFA control must render OFF and DISABLED — never shown
    // enabled from a literal (the prior `enabled: true` was a fabricated posture).
    const mfa = screen.getByRole('switch', { name: /require mfa for admins/i })
    expect(mfa.getAttribute('aria-checked')).toBe('false')
    expect(
      mfa.hasAttribute('disabled')
      || mfa.getAttribute('aria-disabled') === 'true'
      || mfa.getAttribute('data-disabled') !== null,
    ).toBe(true)
    expect(screen.getByRole('combobox', { name: /session duration/i })).toBeTruthy()
    expect(screen.getByRole('heading', { name: /recent security events/i })).toBeTruthy()
    expect(screen.getByText(/loading security events/i)).toBeTruthy()
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
              displayName: 'Velion Page',
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

    render(() => <VelionWorkspaceSettingsPage section="integrations" />)

    expect(await screen.findByRole('heading', { name: /publishing, inbox, and campaign adapters/i })).toBeTruthy()
    expect((await screen.findAllByText('Facebook')).length).toBeGreaterThan(0)
    expect((await screen.findAllByText('Snapchat')).length).toBeGreaterThan(0)
    expect(screen.getByText(/Page publishing, comments, inbox, and analytics/i)).toBeTruthy()
    expect(screen.getByText(/Ads, creative, campaign, and reporting workflows/i)).toBeTruthy()
    expect(screen.getByText('publish-ready')).toBeTruthy()
    expect(screen.getByRole('link', { name: /open calendar/i }).getAttribute('href')).toBe('/social/calendar')
  })

  it('renders billing with live account data and starts embedded checkout', async () => {
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
          provider: 'hyperswitch',
          payment_id: 'pay_settings_123',
          client_secret: 'pay_settings_123_secret',
          publishable_key: 'pk_test_settings',
          client_url: 'https://beta.hyperswitch.io/v1/HyperLoader.js',
          backend_url: 'https://sandbox.hyperswitch.io',
          status: 'requires_payment_method',
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

    const { unmount } = render(() => <VelionWorkspaceSettingsPage section="billing" />)

    expect(screen.getByRole('heading', { name: /billing/i, level: 1 })).toBeTruthy()
    expect((await screen.findAllByText(/trialing/i)).length).toBeGreaterThan(0)
    expect(screen.getByRole('heading', { name: /choose plan/i })).toBeTruthy()
    expect(screen.getByRole('button', { name: /activate advanced/i })).toBeTruthy()
    expect(screen.queryByText('Current plan, renewal date, and upgrade path')).toBeNull()

    fireEvent.click(screen.getByRole('button', { name: /activate advanced/i }))

    expect(await screen.findByRole('region', { name: /payment checkout/i })).toBeTruthy()
    expect(fetchMock).toHaveBeenCalledWith(expect.stringMatching('/api/v1/billing/checkout$'), expect.objectContaining({
      method: 'POST',
    }))

    unmount()
    render(() => <VelionWorkspaceSettingsPage section="sso" />)

    expect(screen.getByRole('heading', { name: /sso/i, level: 1 })).toBeTruthy()
    // De-faked: SSO is honestly "Not configured" — the fabricated provider form,
    // connection test, and attribute mapping panels were removed.
    expect(screen.getByText(/single sign-on is not configured/i)).toBeTruthy()
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
