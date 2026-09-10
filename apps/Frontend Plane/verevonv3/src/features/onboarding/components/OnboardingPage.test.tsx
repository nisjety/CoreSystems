// @vitest-environment jsdom

import { createRouter, memoryHistory } from '@solidjs/router'
import { QueryClient, QueryClientProvider } from '@tanstack/solid-query'
import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import type { JSX } from '@solidjs/web'
import { afterEach, describe, expect, it, vi } from 'vitest'
import OnboardingPage from '@/features/onboarding/components/OnboardingPage'
import type { OnboardingState } from '@/features/onboarding/lib/model'
import { createInitialOnboardingState } from '@/features/onboarding/lib/state'
import { clearSession } from '@/shared/session/session-store'

const storageKey = 'verevonv3.onboarding.state.v1'

function seedOnboardingState(overrides: Partial<OnboardingState>) {
  const state: OnboardingState = {
    ...createInitialOnboardingState(),
    ...overrides,
  }
  window.localStorage.setItem(storageKey, JSON.stringify(state))
}

function renderWithProviders(component: () => JSX.Element, path = '/onboarding') {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })

  const TestRouter = createRouter({
    explicitLinks: true,
    routes: [{
      path: '/*all',
      component: () => (
        <QueryClientProvider client={queryClient}>
          {component()}
        </QueryClientProvider>
      ),
    }],
    history: memoryHistory(path),
  })

  return render(() => <TestRouter>{(props) => <>{props.children}</>}</TestRouter>)
}

afterEach(() => {
  cleanup()
  clearSession()
  window.localStorage.clear()
  vi.unstubAllGlobals()
})

describe('OnboardingPage connect step reads integration-core truth', () => {
  // The connect step used to show only what the user had clicked
  // (`state.connectors`). It now reads GET /api/v1/integrations/connections
  // and shows provider, account, granted capabilities, per-lane sync health
  // and the concrete next step — and folds the truth back into the graph.
  it('shows the connected Microsoft account, its grants and the library next step', async () => {
    seedOnboardingState({
      step: 'connect',
      organization: { name: 'Aquatiq AS', id: 'org_aquatiq', zeroDataRetention: false },
      connectors: [],
    })
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input)
      if (url.includes('/onboarding/theme')) {
        return new Response(JSON.stringify({ persisted: true, mode: 'verevon', primaryColor: '#111111' }), {
          headers: { 'Content-Type': 'application/json' },
          status: 200,
        })
      }
      if (url.endsWith('/api/v1/integrations/connections')) {
        return new Response(JSON.stringify({
          data: {
            connections: [{
              id: 'conn_f019f69a',
              providerKey: 'microsoft',
              connectorType: 'microsoft-graph',
              organizationId: 'org_aquatiq',
              userEmail: 'ima.dacosta@aquatiq.com',
              displayName: 'Ima Fernandes Da Costa',
              status: 'active',
              capabilities: ['profile.read', 'sharepoint.read', 'teams.read', 'teams.messages.read', 'mail.read', 'mail.send'],
              scopes: ['Files.Read.All', 'Sites.Read.All', 'Mail.Read', 'ChannelMessage.Read.All'],
              lastSyncStatus: 'failed',
              createdAt: '2026-09-04T01:00:00.000Z',
              syncLanes: {
                mail: { status: 'synced', source: 'email-worker', lastSyncAt: '2026-09-04T02:00:00.000Z' },
                collaboration: { status: 'pending', source: 'email-worker' },
                documents: {
                  status: 'failed',
                  source: 'finspo-core',
                  failureCode: 'no_sources_registered',
                  lastError: 'no SharePoint or OneDrive library is registered for this organization yet',
                },
              },
            }],
          },
        }), { headers: { 'Content-Type': 'application/json' }, status: 200 })
      }
      return new Response(JSON.stringify({}), { headers: { 'Content-Type': 'application/json' }, status: 200 })
    })
    vi.stubGlobal('fetch', fetchMock)

    renderWithProviders(() => <OnboardingPage />)

    const panel = await screen.findByRole('region', { name: 'Tilkoblede kontoer' })
    await waitFor(() => expect(panel.textContent).toContain('ima.dacosta@aquatiq.com'))
    expect(panel.textContent).toContain('Microsoft 365')
    const grants = screen.getByRole('list', { name: 'Tilganger gitt' })
    expect(Array.from(grants.querySelectorAll('li')).map((item) => item.textContent)).toEqual(['Outlook', 'Teams', 'SharePoint', 'OneDrive'])
    // Mailbox health is the mail lane (synced) — not the connection-level
    // lastSyncStatus that carried the SharePoint failure.
    expect(panel.textContent).toContain('Synkronisert')
    expect(panel.textContent).toContain('Mangler bibliotek')
    expect(panel.textContent).toContain('Velg hvilket SharePoint- eller OneDrive-bibliotek')

    // The truth is folded into the UI record so the catalogue row and the
    // graph agree with integration-core.
    const stored = JSON.parse(window.localStorage.getItem(storageKey) ?? '{}') as OnboardingState
    await waitFor(() => {
      const latest = JSON.parse(window.localStorage.getItem(storageKey) ?? '{}') as OnboardingState
      expect(latest.connectors.some((connector) => connector.id === 'microsoft365' && connector.status === 'connected')).toBe(true)
    })
    expect(stored).toBeTruthy()
  })
})

describe('OnboardingPage paywall commit guard', () => {
  // The first-step back control exits onboarding (sign out + wipe state), so
  // it must ask first -- through an in-app dialog, not `window.confirm`, which
  // embedded webviews and automation-driven browsers auto-dismiss with `false`
  // without rendering (that made the button look dead in the Claude desktop
  // browser pane). Nothing destructive may happen until the dialog is confirmed.
  it('asks before the first-step back control deletes onboarding state and signs out', async () => {
    seedOnboardingState({ step: 'website' })
    const fetchMock = vi.fn<typeof fetch>(async (input) => new Response(JSON.stringify(
      String(input).includes('/onboarding/theme')
        ? { persisted: true, mode: 'verevon', primaryColor: '#111111' }
        : {},
    ), {
      headers: { 'Content-Type': 'application/json' },
      status: 200,
    }))
    vi.stubGlobal('fetch', fetchMock)

    renderWithProviders(() => <OnboardingPage />)

    fireEvent.click(await screen.findByRole('button', { name: 'Tilbake' }))

    const dialog = await screen.findByRole('dialog')
    expect(dialog.getAttribute('aria-labelledby')).toBe('leave-onboarding-dialog-title')
    expect(fetchMock.mock.calls.some(([url]) => String(url).includes('/auth/sign-out'))).toBe(false)
    expect(window.localStorage.getItem(storageKey)).not.toBeNull()

    fireEvent.click(screen.getByRole('button', { name: 'Avbryt' }))

    await waitFor(() => {
      expect(screen.queryByRole('dialog')).toBeNull()
    })
    expect(fetchMock.mock.calls.some(([url]) => String(url).includes('/auth/sign-out'))).toBe(false)
    expect(window.localStorage.getItem(storageKey)).not.toBeNull()
  })

  it('confirming the leave dialog wipes onboarding state and signs out', async () => {
    seedOnboardingState({ step: 'website' })
    const fetchMock = vi.fn<typeof fetch>(async (input) => new Response(JSON.stringify(
      String(input).includes('/onboarding/theme')
        ? { persisted: true, mode: 'verevon', primaryColor: '#111111' }
        : {},
    ), {
      headers: { 'Content-Type': 'application/json' },
      status: 200,
    }))
    vi.stubGlobal('fetch', fetchMock)

    renderWithProviders(() => <OnboardingPage />)

    fireEvent.click(await screen.findByRole('button', { name: 'Tilbake' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Logg ut og slett oppsett' }))

    await waitFor(() => {
      expect(fetchMock.mock.calls.some(([url]) => String(url).includes('/auth/sign-out'))).toBe(true)
    })
    expect(window.localStorage.getItem(storageKey)).toBeNull()
    expect(screen.queryByRole('dialog')).toBeNull()
  })

  // Regression test: onboardingPlanCards.checkoutEnabled (model.ts) must gate
  // commitPlan() the same way billingPlans.checkoutEnabled gates
  // WorkspaceSettingsPage.startPlanCheckout — the enterprise/"Custom" tier's
  // real price (2499 kr/mnd in billing-core) is never shown in this UI, so
  // committing it must never reach actions.startCheckout (a real Nexi/
  // Hyperswitch checkout session).
  it('opens the contact-sales modal for the enterprise/Custom tier instead of starting a real checkout', async () => {
    seedOnboardingState({
      step: 'paywall',
      organization: { name: 'Aquatiq AS', id: 'org_test_enterprise', zeroDataRetention: false },
      plan: 'enterprise',
    })

    const fetchMock = vi.fn<typeof fetch>(async (input) => new Response(JSON.stringify(
      String(input).includes('/onboarding/theme')
        ? { persisted: true, mode: 'verevon', primaryColor: '#111111' }
        : {},
    ), {
      headers: { 'Content-Type': 'application/json' },
      status: 200,
    }))
    vi.stubGlobal('fetch', fetchMock)

    renderWithProviders(() => <OnboardingPage />)

    const commitButton = await screen.findByRole('button', { name: 'Fortsett til betaling' })
    fireEvent.click(commitButton)

    await waitFor(() => {
      expect(screen.getByRole('dialog')).toBeTruthy()
    })
    const mailLink = screen.getByRole('link', { name: /e-post|email/i })
    expect(mailLink.getAttribute('href')).toContain('mailto:hei@verevon.ai')
    expect(mailLink.getAttribute('href')).toContain('Aquatiq')

    const calledUrls = fetchMock.mock.calls.map((call) => String(call[0]))
    expect(calledUrls.some((url) => url.includes('/onboarding/actions/start-checkout'))).toBe(false)
  })

  it('still starts a real checkout for a plan that has checkoutEnabled', async () => {
    seedOnboardingState({
      step: 'paywall',
      organization: { name: 'Aquatiq AS', id: 'org_test_standard', zeroDataRetention: false },
      plan: 'standard',
    })

    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input)
      if (url.includes('/onboarding/theme')) {
        return new Response(JSON.stringify({
          persisted: true,
          mode: 'verevon',
          primaryColor: '#111111',
        }), {
          headers: { 'Content-Type': 'application/json' },
          status: 200,
        })
      }
      if (url.includes('/onboarding/actions/start-checkout')) {
        return new Response(JSON.stringify({
          provider: 'nexi',
          id: 'pay_test_123',
          payment_id: 'pay_test_123',
          publishable_key: 'checkout_test_123',
          client_url: 'https://test.checkout.dibspayment.eu/v1/checkout.js?v=1',
          url: 'https://test.checkout.dibspayment.eu/payments/pay_test_123',
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

    renderWithProviders(() => <OnboardingPage />)

    const commitButton = await screen.findByRole('button', { name: 'Fortsett til betaling' })
    fireEvent.click(commitButton)

    await waitFor(() => {
      const calledUrls = fetchMock.mock.calls.map((call) => String(call[0]))
      expect(calledUrls.some((url) => url.includes('/onboarding/actions/start-checkout'))).toBe(true)
    })
  })
})
