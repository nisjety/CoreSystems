import type { CheckoutSession } from '@/features/billing/lib/api'

export type CheckoutSurface = 'nexi-embedded' | 'hyperswitch-embedded' | 'redirect' | 'invalid'

function hasValue(value?: string): boolean {
  return Boolean(value?.trim())
}

function checkoutUrl(value?: string): URL | null {
  if (!value) return null

  try {
    const url = new URL(value)
    if (url.username || url.password || url.port || url.hash) return null
    return url
  } catch {
    return null
  }
}

function isApprovedNexiClientUrl(value?: string): boolean {
  const url = checkoutUrl(value)
  if (!url || url.protocol !== 'https:') return false
  if (!['checkout.dibspayment.eu', 'test.checkout.dibspayment.eu'].includes(url.hostname)) {
    return false
  }
  if (url.pathname !== '/v1/checkout.js') return false
  return [...url.searchParams.keys()].every((key) => key === 'v')
}

function isApprovedStripeCheckoutUrl(value?: string): boolean {
  const url = checkoutUrl(value)
  return Boolean(
    url &&
    url.protocol === 'https:' &&
    url.hostname === 'checkout.stripe.com' &&
    url.pathname.startsWith('/'),
  )
}

export function resolveCheckoutSurface(session: CheckoutSession): CheckoutSurface {
  const provider = session.provider?.trim().toLowerCase()

  if (provider === 'nexi') {
    return (
      hasValue(session.payment_id || session.id) &&
      hasValue(session.publishable_key) &&
      isApprovedNexiClientUrl(session.client_url)
    ) ? 'nexi-embedded' : 'invalid'
  }

  // Hyperswitch is not part of the secure MVP: its executable SDK/backend
  // origins are not present in the reviewed production CSP.
  if (provider === 'hyperswitch') return 'invalid'

  return provider === 'stripe' && isApprovedStripeCheckoutUrl(session.url)
    ? 'redirect'
    : 'invalid'
}
