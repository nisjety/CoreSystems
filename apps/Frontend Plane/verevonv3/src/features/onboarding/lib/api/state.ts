import { requestJson } from '@/shared/api/http'
import { withDevActor } from '@/features/onboarding/lib/api/actor'
import type {
  ActionActor,
  OnboardingStateSnapshot,
} from '@/features/onboarding/lib/api/contracts'
import {
  onboardingStateSavedSchema,
  onboardingStateSchema,
  parseOnboardingResponse,
} from './response-schemas'

export async function loadOnboardingState<TState = Record<string, unknown>>(): Promise<OnboardingStateSnapshot<TState>> {
  const endpoint = '/api/v1/onboarding/state'
  return parseOnboardingResponse(onboardingStateSchema, await requestJson<unknown>(endpoint), endpoint) as OnboardingStateSnapshot<TState>
}

export async function saveOnboardingState<TState extends object>(input: {
  actor: ActionActor
  step: string
  state: TState
}): Promise<{ success?: boolean }> {
  const endpoint = '/api/v1/onboarding/state'
  return parseOnboardingResponse(onboardingStateSavedSchema, await requestJson<unknown>(endpoint, {
    method: 'PUT',
    body: JSON.stringify(withDevActor({
      step: input.step,
      state: input.state,
    }, input.actor)),
  }), endpoint)
}
