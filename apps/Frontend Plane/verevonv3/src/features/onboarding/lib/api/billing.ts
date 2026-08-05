import { requestJson } from '@/shared/api/http'
import { withDevActor } from '@/features/onboarding/lib/api/actor'
import type { ActionActor } from '@/features/onboarding/lib/api/contracts'
import type { CheckoutSession, CheckoutStatus } from '@/features/billing/lib/api'
import {
  checkoutSessionSchema,
  checkoutStatusSchema,
  parseOnboardingResponse,
  planSetSchema,
} from './response-schemas'

export type { CheckoutSession, CheckoutStatus } from '@/features/billing/lib/api'

export async function setPlan(input: {
  actor: ActionActor
  orgId: string
  plan: string
  onboarding?: Record<string, unknown>
}): Promise<{ id: string; plan: string }> {
  const endpoint = '/api/v1/onboarding/actions/set-plan'
  return parseOnboardingResponse(planSetSchema, await requestJson<unknown>(endpoint, {
    method: 'POST',
    body: JSON.stringify(withDevActor({
      orgId: input.orgId,
      plan: input.plan,
      reason: 'onboarding',
      onboarding: input.onboarding,
    }, input.actor)),
  }), endpoint)
}

export async function startCheckout(input: {
  actor: ActionActor
  orgId: string
  plan: string
  successUrl: string
  cancelUrl: string
}): Promise<CheckoutSession> {
  const endpoint = '/api/v1/onboarding/actions/start-checkout'
  return parseOnboardingResponse(checkoutSessionSchema, await requestJson<unknown>(endpoint, {
    method: 'POST',
    body: JSON.stringify(withDevActor({
      orgId: input.orgId,
      plan: input.plan,
      successUrl: input.successUrl,
      cancelUrl: input.cancelUrl,
    }, input.actor)),
  }), endpoint)
}

export async function confirmCheckout(input: {
  actor: ActionActor
  orgId: string
  plan: string
  paymentId?: string
  clientSecret?: string
}): Promise<CheckoutStatus> {
  const endpoint = '/api/v1/onboarding/actions/confirm-checkout'
  return parseOnboardingResponse(checkoutStatusSchema, await requestJson<unknown>(endpoint, {
    method: 'POST',
    body: JSON.stringify(withDevActor({
      orgId: input.orgId,
      plan: input.plan,
      paymentId: input.paymentId,
      clientSecret: input.clientSecret,
    }, input.actor)),
  }), endpoint)
}
