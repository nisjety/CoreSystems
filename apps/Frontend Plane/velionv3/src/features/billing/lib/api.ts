import { requestJson } from '@/shared/api/http'

export type BillingAccount = {
  org_id: string
  plan: string
  subscription_state: string
  credits: number
  products?: Record<string, boolean>
  feature_flags?: Record<string, boolean>
  entitlements?: Record<string, boolean>
  quota_limits?: Record<string, number>
  provider_customer_id?: Record<string, string>
  metadata?: Record<string, unknown>
  trial_ends_at?: string
  updated_at?: string
  created_at?: string
}

export type CheckoutSession = {
  id?: string
  url?: string
  provider?: 'hyperswitch' | 'stripe' | string
  payment_id?: string
  client_secret?: string
  publishable_key?: string
  client_url?: string
  backend_url?: string
  status?: string
  amount_cents?: number
  currency?: string
}

export type CheckoutStatus = {
  provider?: 'hyperswitch' | 'stripe' | string
  payment_id?: string
  client_secret?: string
  status: string
  org_id?: string
  plan?: string
  amount_cents?: number
  currency?: string
}

export async function loadBillingAccount(signal?: AbortSignal): Promise<BillingAccount> {
  return requestJson('/api/v1/billing/account', {
    method: 'GET',
    signal,
  })
}

export async function startBillingCheckout(input: {
  cancelUrl: string
  plan: string
  successUrl: string
}): Promise<CheckoutSession> {
  return requestJson('/api/v1/billing/checkout', {
    method: 'POST',
    body: JSON.stringify({
      plan: input.plan,
      success_url: input.successUrl,
      cancel_url: input.cancelUrl,
    }),
  })
}

export async function confirmBillingCheckout(input: {
  clientSecret?: string
  paymentId?: string
  plan: string
}): Promise<CheckoutStatus> {
  return requestJson('/api/v1/billing/checkout/confirm', {
    method: 'POST',
    body: JSON.stringify({
      plan: input.plan,
      payment_id: input.paymentId,
      client_secret: input.clientSecret,
    }),
  })
}
