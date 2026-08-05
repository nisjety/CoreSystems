// @vitest-environment jsdom

import { fireEvent, render, screen } from '@solidjs/testing-library'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { PaywallStep } from '@/features/onboarding/components/steps/PaywallStep'
import { I18nProvider, localeStorageKey } from '@/shared/i18n'

describe('PaywallStep', () => {
  beforeEach(() => {
    installMemoryStorage()
  })

  it('uses shared button and switch primitives for plan selection and billing', () => {
    render(() => (
      <PaywallStep
        activePlanId="trial"
        checkoutReturnUrl="https://verevon.test/onboarding"
        loadingRecommendation={false}
        onConfirmCheckout={vi.fn()}
        onCommitPlan={vi.fn()}
        onSelectPlan={vi.fn()}
      />
    ))

    expect(screen.getByLabelText('Faktureringsperiode')).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Valgt' }).className).toContain('button--primary')
    expect(screen.getAllByRole('button', { name: 'Velg plan' })[0]?.className).toContain('button--secondary')
    expect(screen.getByRole('button', { name: 'Fortsett til oppsett' }).className).toContain('button--primary')
  })

  it('disables the shared commit button while saving the selected plan', () => {
    render(() => (
      <PaywallStep
        activePlanId="trial"
        checkoutReturnUrl="https://verevon.test/onboarding"
        committing
        loadingRecommendation={false}
        onConfirmCheckout={vi.fn()}
        onCommitPlan={vi.fn()}
        onSelectPlan={vi.fn()}
      />
    ))

    const button = screen.getByRole('button', { name: 'Lagrer...' }) as HTMLButtonElement
    expect(button.disabled).toBe(true)
    expect(button.className).toContain('button--primary')
  })

  it('renders the Hyperswitch checkout surface after a paid session is created', () => {
    render(() => (
      <PaywallStep
        activePlanId="standard"
        checkoutReturnUrl="https://verevon.test/onboarding"
        checkoutSession={{
          provider: 'hyperswitch',
          payment_id: 'pay_testpaymentid12345678901234',
          client_secret: 'pay_testpaymentid12345678901234_secret_abc',
          publishable_key: 'pk_snd_123',
          client_url: 'https://beta.hyperswitch.io/v1/HyperLoader.js',
          backend_url: 'https://sandbox.hyperswitch.io',
          amount_cents: 99900,
          currency: 'NOK',
          status: 'requires_payment_method',
        }}
        loadingRecommendation={false}
        onConfirmCheckout={vi.fn()}
        onCommitPlan={vi.fn()}
        onSelectPlan={vi.fn()}
      />
    ))

    expect(screen.getByLabelText('Payment checkout')).toBeTruthy()
    expect(screen.getByText('Sikker betaling')).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Betal og aktiver' })).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Fortsett til betaling' })).toBeNull()
  })

  it('surfaces the Verevon recommendation and lets the user apply or refresh it', () => {
    const onSelectPlan = vi.fn()
    const onRefreshRecommendation = vi.fn()

    render(() => (
      <PaywallStep
        activePlanId="trial"
        checkoutReturnUrl="https://verevon.test/onboarding"
        loadingRecommendation={false}
        recommendation={{
          planId: 'pro',
          reason: 'Flere kilder og høyere operasjonell kompleksitet peker mot Expert.',
          summary: '3 kilder og 48 ansatte gjør Expert til beste startpunkt.',
          proofPoints: ['3 kilder er valgt i onboarding.', 'Brreg eller brukeren oppga 48 ansatte.'],
          scopeSignals: ['Microsoft 365, Slack, GitHub'],
          opportunities: ['Start med de vanligste spørsmålene fra nettsted og dokumentkilder.'],
          generatedAt: '2026-06-18T10:00:00.000Z',
          source: 'model',
        }}
        onConfirmCheckout={vi.fn()}
        onCommitPlan={vi.fn()}
        onRefreshRecommendation={onRefreshRecommendation}
        onSelectPlan={onSelectPlan}
      />
    ))

    expect(screen.getByText('Verevon AI-forslag')).toBeTruthy()
    expect(screen.getByText('Expert anbefales')).toBeTruthy()
    expect(screen.getByText('3 kilder og 48 ansatte gjør Expert til beste startpunkt.')).toBeTruthy()
    expect(screen.getByText('3 kilder er valgt i onboarding.')).toBeTruthy()
    // Scope signals + opportunities now render on the assembly step (to keep the
    // non-scrolling paywall concise), so they must NOT appear here.
    expect(screen.queryByText('Microsoft 365, Slack, GitHub')).toBeNull()

    fireEvent.click(screen.getByRole('button', { name: 'Bruk anbefalingen' }))
    expect(onSelectPlan).toHaveBeenCalledWith('pro')

    fireEvent.click(screen.getByRole('button', { name: 'Oppdater forslag' }))
    expect(onRefreshRecommendation).toHaveBeenCalled()
  })

  it('renders the canonical connected source count from onboarding identity', () => {
    render(() => (
      <PaywallStep
        activePlanId="standard"
        checkoutReturnUrl="https://verevon.test/onboarding"
        identity={{
          orgName: 'AQUATIQ AS',
          websiteUrl: 'https://aquatiq.com',
          websitePages: 6,
          connectedSourceCount: 10,
        }}
        loadingRecommendation={false}
        onConfirmCheckout={vi.fn()}
        onCommitPlan={vi.fn()}
        onSelectPlan={vi.fn()}
      />
    ))

    expect(screen.getByText('6 sider fra aquatiq.com')).toBeTruthy()
    expect(screen.getByText('10 tilkoblede kilder')).toBeTruthy()
  })

  it('keeps enterprise recommendation copy aligned with the Custom plan and source counts', () => {
    render(() => (
      <PaywallStep
        activePlanId="enterprise"
        checkoutReturnUrl="https://verevon.test/onboarding"
        identity={{
          orgName: 'AQUATIQ AS',
          connectedSourceCount: 10,
          sourceCount: 11,
          employeeCount: 93,
        }}
        loadingRecommendation={false}
        recommendation={{
          planId: 'enterprise',
          reason: 'Kompleksitet, volum eller governance-signaler peker mot Enterprise.',
          summary: '11 kilder og 93 ansatte gir best start med Enterprise.',
          proofPoints: ['10 tilkoblede kilder valgt.', 'Brreg eller brukeren oppga 93 ansatte.'],
          scopeSignals: [],
          opportunities: [],
          generatedAt: '2026-07-10T10:00:00.000Z',
          source: 'local',
        }}
        onConfirmCheckout={vi.fn()}
        onCommitPlan={vi.fn()}
        onSelectPlan={vi.fn()}
      />
    ))

    expect(screen.getByText('Custom anbefales')).toBeTruthy()
    expect(screen.getByText('11 kilder totalt (10 tilkoblede) og 93 ansatte gir best start med Custom.')).toBeTruthy()
    expect(screen.getByText('Kompleksitet, volum eller governance-signaler peker mot Custom.')).toBeTruthy()
    expect(screen.queryByText(/Enterprise/)).toBeNull()
  })

  it('keeps source counts explicit when employee count is unknown', () => {
    render(() => (
      <PaywallStep
        activePlanId="enterprise"
        checkoutReturnUrl="https://verevon.test/onboarding"
        identity={{ connectedSourceCount: 10, sourceCount: 11 }}
        loadingRecommendation={false}
        recommendation={{
          planId: 'enterprise',
          reason: 'Complexity points to Enterprise.',
          summary: '11 sources and 0 employees make Enterprise the best starting point.',
          proofPoints: [],
          scopeSignals: [],
          opportunities: [],
          generatedAt: '2026-07-10T10:00:00.000Z',
          source: 'model',
        }}
        onConfirmCheckout={vi.fn()}
        onCommitPlan={vi.fn()}
        onSelectPlan={vi.fn()}
      />
    ))

    expect(screen.getByText('11 kilder totalt (10 tilkoblede) gir best start med Custom.')).toBeTruthy()
    expect(screen.queryByText(/0 employees/)).toBeNull()
  })

  it('uses singular Norwegian recommendation counts', () => {
    render(() => (
      <PaywallStep
        activePlanId="enterprise"
        checkoutReturnUrl="https://verevon.test/onboarding"
        identity={{ connectedSourceCount: 1, sourceCount: 1, employeeCount: 1 }}
        loadingRecommendation={false}
        recommendation={{
          planId: 'enterprise',
          reason: 'Kompleksitet peker mot Enterprise.',
          summary: '1 kilde og 1 ansatt gir best start med Enterprise.',
          proofPoints: [],
          scopeSignals: [],
          opportunities: [],
          generatedAt: '2026-07-10T10:00:00.000Z',
          source: 'local',
        }}
        onConfirmCheckout={vi.fn()}
        onCommitPlan={vi.fn()}
        onSelectPlan={vi.fn()}
      />
    ))

    expect(screen.getByText('1 kilde og 1 ansatt gir best start med Custom.')).toBeTruthy()
  })

  it('uses singular English recommendation counts', () => {
    window.localStorage.setItem(localeStorageKey, 'en')

    render(() => (
      <I18nProvider>
        <PaywallStep
          activePlanId="enterprise"
          checkoutReturnUrl="https://verevon.test/onboarding"
          identity={{ connectedSourceCount: 1, sourceCount: 1, employeeCount: 1 }}
          loadingRecommendation={false}
          recommendation={{
            planId: 'enterprise',
            reason: 'Complexity points to Enterprise.',
            summary: '1 source and 1 employee make Enterprise the best starting point.',
            proofPoints: [],
            scopeSignals: [],
            opportunities: [],
            generatedAt: '2026-07-10T10:00:00.000Z',
            source: 'model',
          }}
          onConfirmCheckout={vi.fn()}
          onCommitPlan={vi.fn()}
          onSelectPlan={vi.fn()}
        />
      </I18nProvider>
    ))

    expect(screen.getByText('1 source and 1 employee make Custom the best starting point.')).toBeTruthy()
  })

  it('uses the connected count for mixed-count grammar', () => {
    render(() => (
      <PaywallStep
        activePlanId="enterprise"
        checkoutReturnUrl="https://verevon.test/onboarding"
        identity={{ connectedSourceCount: 1, sourceCount: 2, employeeCount: 1 }}
        loadingRecommendation={false}
        recommendation={{
          planId: 'enterprise',
          reason: 'Kompleksitet peker mot Enterprise.',
          summary: '2 kilder og 1 ansatt gir best start med Enterprise.',
          proofPoints: [],
          scopeSignals: [],
          opportunities: [],
          generatedAt: '2026-07-10T10:00:00.000Z',
          source: 'local',
        }}
        onConfirmCheckout={vi.fn()}
        onCommitPlan={vi.fn()}
        onSelectPlan={vi.fn()}
      />
    ))

    expect(screen.getByText('2 kilder totalt (1 tilkoblet) og 1 ansatt gir best start med Custom.')).toBeTruthy()
  })

  it('uses singular English grammar when only source counts are known', () => {
    window.localStorage.setItem(localeStorageKey, 'en')

    render(() => (
      <I18nProvider>
        <PaywallStep
          activePlanId="enterprise"
          checkoutReturnUrl="https://verevon.test/onboarding"
          identity={{ connectedSourceCount: 1, sourceCount: 1 }}
          loadingRecommendation={false}
          recommendation={{
            planId: 'enterprise',
            reason: 'Complexity points to Enterprise.',
            summary: '1 source makes Enterprise the best starting point.',
            proofPoints: [],
            scopeSignals: [],
            opportunities: [],
            generatedAt: '2026-07-10T10:00:00.000Z',
            source: 'model',
          }}
          onConfirmCheckout={vi.fn()}
          onCommitPlan={vi.fn()}
          onSelectPlan={vi.fn()}
        />
      </I18nProvider>
    ))

    expect(screen.getByText('1 source makes Custom the best starting point.')).toBeTruthy()
  })

  it('renders paywall chrome and plan cards in English when the shared locale is English', () => {
    window.localStorage.setItem(localeStorageKey, 'en')

    render(() => (
      <I18nProvider>
        <PaywallStep
          activePlanId="trial"
          checkoutReturnUrl="https://verevon.test/onboarding"
          identity={{
            orgName: 'AQUATIQ AS',
            websiteUrl: 'https://aquatiq.com',
            websitePages: 6,
            connectedSourceCount: 10,
          }}
          loadingRecommendation={false}
          onConfirmCheckout={vi.fn()}
          onCommitPlan={vi.fn()}
          onSelectPlan={vi.fn()}
        />
      </I18nProvider>
    ))

    expect(screen.getByText('Best match for AQUATIQ AS')).toBeTruthy()
    expect(screen.getByText('6 pages from aquatiq.com')).toBeTruthy()
    expect(screen.getByText('10 connected sources')).toBeTruthy()
    expect(screen.getByText('For larger support teams with reporting and controls.')).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Continue to setup' })).toBeTruthy()
  })
})

function installMemoryStorage() {
  const values = new Map<string, string>()
  Object.defineProperty(window, 'localStorage', {
    configurable: true,
    value: {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => {
        values.set(key, value)
      },
      removeItem: (key: string) => {
        values.delete(key)
      },
      clear: () => values.clear(),
      key: (index: number) => Array.from(values.keys())[index] ?? null,
      get length() {
        return values.size
      },
    } satisfies Storage,
  })
}
