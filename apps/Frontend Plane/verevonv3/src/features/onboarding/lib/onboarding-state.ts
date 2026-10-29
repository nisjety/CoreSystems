import { createStore } from 'solid-js/store'
import type { OnboardingState } from '@/features/onboarding/lib/model'
import { loadStoredOnboardingState } from '@/features/onboarding/lib/state'

export function createOnboardingState(storageKey: string) {
  const [state, setState] = createStore<OnboardingState>(loadStoredOnboardingState(storageKey))
  return [state, setState] as const
}
