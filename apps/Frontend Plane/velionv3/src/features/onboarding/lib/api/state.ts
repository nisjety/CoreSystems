import { requestJson } from '@/shared/api/http'
import { withDevActor } from '@/features/onboarding/lib/api/actor'
import type {
  ActionActor,
  OnboardingStateSnapshot,
} from '@/features/onboarding/lib/api/contracts'

export async function loadOnboardingState<TState = Record<string, unknown>>(): Promise<OnboardingStateSnapshot<TState>> {
  return requestJson<OnboardingStateSnapshot<TState>>('/api/v1/onboarding/state')
}

export async function saveOnboardingState<TState extends object>(input: {
  actor: ActionActor
  step: string
  state: TState
}): Promise<{ success?: boolean }> {
  return requestJson('/api/v1/onboarding/state', {
    method: 'PUT',
    body: JSON.stringify(withDevActor({
      step: input.step,
      state: input.state,
    }, input.actor)),
  })
}
