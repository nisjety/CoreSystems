import type { OnboardingStateSnapshot } from '@/features/onboarding/lib/api'
import type { OnboardingState, Step } from '@/features/onboarding/lib/model'
import { onboardingSteps } from '@/features/onboarding/lib/model'

export function createInitialOnboardingState(): OnboardingState {
  return {
    step: 'post-signin',
    introPlayed: false,
    websiteSkipped: false,
    website: {
      url: '',
      brief: '',
      snippets: [],
      pages: 0,
      elements: 0,
      status: 'idle',
    },
    organization: {
      name: '',
      zeroDataRetention: false,
    },
    connectors: [],
    themeMode: 'brand',
  }
}

export function loadStoredOnboardingState(storageKey: string): OnboardingState {
  if (typeof window === 'undefined') return createInitialOnboardingState()

  try {
    const raw = window.localStorage.getItem(storageKey)
    return raw
      ? reconcileOnboardingState({ step: '', state: JSON.parse(raw) as Partial<OnboardingState> })
      : createInitialOnboardingState()
  } catch {
    return createInitialOnboardingState()
  }
}

export function reconcileOnboardingState(
  snapshot?: OnboardingStateSnapshot<Partial<OnboardingState>>,
): OnboardingState {
  const base = createInitialOnboardingState()
  const raw = snapshot?.state

  return {
    ...base,
    ...raw,
    step: isStep(snapshot?.step) ? snapshot.step : isStep(raw?.step) ? raw.step : base.step,
    websiteSkipped: raw?.websiteSkipped ?? false,
    website: {
      ...base.website,
      ...raw?.website,
      snippets: Array.isArray(raw?.website?.snippets) ? raw.website.snippets : base.website.snippets,
    },
    organization: {
      ...base.organization,
      ...raw?.organization,
    },
    connectors: Array.isArray(raw?.connectors) ? raw.connectors : base.connectors,
    themeMode: raw?.themeMode === 'verevon' ? 'verevon' : 'brand',
  }
}

export function cloneOnboardingState<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T
}

export function createPersistedOnboardingState(value: OnboardingState): OnboardingState {
  const snapshot = cloneOnboardingState(value)
  return {
    ...snapshot,
    website: {
      ...snapshot.website,
      snippets: snapshot.website.snippets.slice(-6),
    },
  }
}

function isStep(value: unknown): value is Step {
  return typeof value === 'string' && (onboardingSteps as readonly string[]).includes(value)
}
