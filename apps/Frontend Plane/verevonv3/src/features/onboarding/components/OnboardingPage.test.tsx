// @vitest-environment jsdom

import { Route, Router } from '@solidjs/router'
import { QueryClient, QueryClientProvider } from '@tanstack/solid-query'
import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import type { JSX } from 'solid-js'
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
  window.history.pushState(null, '', path)
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })

  return render(() => (
    <Router root={(props) => <>{props.children}</>}>
      <Route
        path="/*all"
        component={() => (
          <QueryClientProvider client={queryClient}>
            {component()}
          </QueryClientProvider>
        )}
      />
    </Router>
  ))
}

afterEach(() => {
  cleanup()
  clearSession()
  window.localStorage.clear()
  vi.unstubAllGlobals()
})

describe('OnboardingPage paywall commit guard', () => {
  it('asks before the first-step back control deletes onboarding state and signs out', async () => {
    seedOnboardingState({ step: 'website' })
    const confirm = vi.fn(() => false)
    const fetchMock = vi.fn<typeof fetch>(async (input) => new Response(JSON.stringify(
      String(input).includes('/onboarding/theme')
        ? { persisted: true, mode: 'verevon', primaryColor: '#111111' }
        : {},
    ), {
      headers: { 'Content-Type': 'application/json' },
      status: 200,
    }))
    vi.stubGlobal('confirm', confirm)
    vi.stubGlobal('fetch', fetchMock)

    renderWithProviders(() => <OnboardingPage />)

    fireEvent.click(await screen.findByRole('button', { name: 'Tilbake' }))

    expect(confirm).toHaveBeenCalledTimes(1)
    expect(fetchMock.mock.calls.some(([url]) => String(url).includes('/auth/sign-out'))).toBe(false)
    expect(window.localStorage.getItem(storageKey)).not.toBeNull()
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
