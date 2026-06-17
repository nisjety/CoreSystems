// @vitest-environment jsdom

import { render, screen } from '@solidjs/testing-library'
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
})
