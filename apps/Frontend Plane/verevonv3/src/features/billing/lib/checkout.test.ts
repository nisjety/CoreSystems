import { describe, expect, it } from 'vitest'
import { resolveCheckoutSurface } from '@/features/billing/lib/checkout'

describe('resolveCheckoutSurface', () => {
  it('prefers a complete Nexi embedded session over its hosted fallback URL', () => {
    expect(resolveCheckoutSurface({
      provider: 'nexi',
      id: 'payment-123',
      payment_id: 'payment-123',
      publishable_key: 'checkout-key',
      client_url: 'https://test.checkout.dibspayment.eu/v1/checkout.js?v=1',
      url: 'https://test.checkout.dibspayment.eu/payments/payment-123',
    })).toBe('nexi-embedded')
  })

  it('fails closed for the currently unsupported Hyperswitch embedded surface', () => {
    expect(resolveCheckoutSurface({
      provider: 'hyperswitch',
      client_secret: 'payment_secret',
      publishable_key: 'pk_test',
      client_url: 'https://beta.hyperswitch.io/v1/HyperLoader.js',
    })).toBe('invalid')
  })

  it('uses an explicit hosted URL for non-Nexi providers', () => {
    expect(resolveCheckoutSurface({
      provider: 'stripe',
      url: 'https://checkout.stripe.com/session/123',
    })).toBe('redirect')
  })

  it('rejects a non-HTTP hosted URL', () => {
    expect(resolveCheckoutSurface({
      provider: 'stripe',
      url: 'javascript:alert(document.domain)',
    })).toBe('invalid')
  })

  it('rejects unapproved embedded SDK and hosted checkout origins', () => {
    expect(resolveCheckoutSurface({
      provider: 'nexi',
      payment_id: 'payment-123',
      publishable_key: 'checkout-key',
      client_url: 'https://evil.example/v1/checkout.js',
    })).toBe('invalid')
    expect(resolveCheckoutSurface({
      provider: 'stripe',
      url: 'https://evil.example/checkout/session-123',
    })).toBe('invalid')
  })

  it('rejects credentialed, port-shifted, or wrong-path Nexi SDK URLs', () => {
    for (const clientUrl of [
      'https://user:pass@test.checkout.dibspayment.eu/v1/checkout.js',
      'https://test.checkout.dibspayment.eu:444/v1/checkout.js',
      'https://test.checkout.dibspayment.eu/v1/not-checkout.js',
    ]) {
      expect(resolveCheckoutSurface({
        provider: 'nexi',
        payment_id: 'payment-123',
        publishable_key: 'checkout-key',
        client_url: clientUrl,
      })).toBe('invalid')
    }
  })

  it('fails closed when Nexi does not provide its required embedded fields', () => {
    expect(resolveCheckoutSurface({
      provider: 'nexi',
      id: 'payment-123',
      url: 'https://test.checkout.dibspayment.eu/payments/payment-123',
    })).toBe('invalid')
  })
})
