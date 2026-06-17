import { requestJson } from '@/shared/api/http'
import { withDevActor } from '@/features/onboarding/lib/api/actor'
import type { ActionActor } from '@/features/onboarding/lib/api/contracts'
import type { CheckoutSession, CheckoutStatus } from '@/features/billing/lib/api'

export type { CheckoutSession, CheckoutStatus } from '@/features/billing/lib/api'

export async function setPlan(input: {
  actor: ActionActor
  orgId: string
  plan: string
  onboarding?: Record<string, unknown>
}): Promise<{ id: string; plan: string }> {
  return requestJson('/api/v1/onboarding/actions/set-plan', {
    method: 'POST',
    body: JSON.stringify(withDevActor({
      orgId: input.orgId,
      plan: input.plan,
      reason: 'onboarding',
      onboarding: input.onboarding,
    }, input.actor)),
  })
}

export async function startCheckout(input: {
  actor: ActionActor
  orgId: string
  plan: string
  successUrl: string
  cancelUrl: string
}): Promise<CheckoutSession> {
  return requestJson('/api/v1/onboarding/actions/start-checkout', {
    method: 'POST',
    body: JSON.stringify(withDevActor({
      orgId: input.orgId,
      plan: input.plan,
      successUrl: input.successUrl,
      cancelUrl: input.cancelUrl,
    }, input.actor)),
  })
}

export async function confirmCheckout(input: {
  actor: ActionActor
  orgId: string
  plan: string
  paymentId?: string
  clientSecret?: string
}): Promise<CheckoutStatus> {
  return requestJson('/api/v1/onboarding/actions/confirm-checkout', {
    method: 'POST',
    body: JSON.stringify(withDevActor({
      orgId: input.orgId,
      plan: input.plan,
      paymentId: input.paymentId,
      clientSecret: input.clientSecret,
    }, input.actor)),
  })
}
