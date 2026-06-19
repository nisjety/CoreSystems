// @vitest-environment jsdom

import { fireEvent, render, screen } from '@solidjs/testing-library'
import { describe, expect, it, vi } from 'vitest'
import { PaywallStep } from '@/features/onboarding/components/steps/PaywallStep'

describe('PaywallStep', () => {
  it('uses shared button and switch primitives for plan selection and billing', () => {
    render(() => (
      <PaywallStep
        activePlanId="trial"
        checkoutReturnUrl="https://velion.test/onboarding"
        loadingRecommendation={false}
        onConfirmCheckout={vi.fn()}
        onCommitPlan={vi.fn()}
        onSelectPlan={vi.fn()}
      />
    ))

    expect(screen.getByLabelText('Billing period')).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Valgt' }).className).toContain('button--primary')
    expect(screen.getAllByRole('button', { name: 'Velg plan' })[0]?.className).toContain('button--secondary')
    expect(screen.getByRole('button', { name: 'Fortsett til oppsett' }).className).toContain('button--primary')
  })

  it('disables the shared commit button while saving the selected plan', () => {
    render(() => (
      <PaywallStep
        activePlanId="trial"
        checkoutReturnUrl="https://velion.test/onboarding"
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
        checkoutReturnUrl="https://velion.test/onboarding"
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

  it('surfaces the Velion recommendation and lets the user apply or refresh it', () => {
    const onSelectPlan = vi.fn()
    const onRefreshRecommendation = vi.fn()

    render(() => (
      <PaywallStep
        activePlanId="trial"
        checkoutReturnUrl="https://velion.test/onboarding"
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

    expect(screen.getByText('Velion AI-forslag')).toBeTruthy()
    expect(screen.getByText('Expert anbefales')).toBeTruthy()
    expect(screen.getByText('3 kilder og 48 ansatte gjør Expert til beste startpunkt.')).toBeTruthy()
    expect(screen.getByText('3 kilder er valgt i onboarding.')).toBeTruthy()
    expect(screen.getByText('Microsoft 365, Slack, GitHub')).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: 'Bruk anbefalingen' }))
    expect(onSelectPlan).toHaveBeenCalledWith('pro')

    fireEvent.click(screen.getByRole('button', { name: 'Oppdater forslag' }))
    expect(onRefreshRecommendation).toHaveBeenCalled()
  })
})
