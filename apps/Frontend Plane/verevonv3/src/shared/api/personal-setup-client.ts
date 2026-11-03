import { requestJson } from './http'

/**
 * The personal first-run step (EP-8-S8). `pending` is the only state the card shows. user-core stores
 * these three values and reads any other stored value as `pending`.
 */
export type PersonalStep = 'pending' | 'completed' | 'skipped'

export type PersonalSetupState = {
  /** Whether this deployment offers the card (user-core `PERSONAL_SETUP_VISIBLE`). */
  available: boolean
  step: PersonalStep
}

type AISettingsFlags = { personalSetupAvailable?: boolean }
type OnboardingResponse = { personalStep?: string }

/**
 * Reads whether the card is offered and what the person has already answered. Both reads must succeed:
 * a failed read rejects, and the card stays hidden, which is the safe reading.
 */
export async function getPersonalSetupState(): Promise<PersonalSetupState> {
  const [ai, onboarding] = await Promise.all([
    requestJson<AISettingsFlags>('/api/v1/settings/ai'),
    requestJson<OnboardingResponse>('/api/v1/settings/onboarding'),
  ])
  const step = onboarding.personalStep === 'completed' || onboarding.personalStep === 'skipped' ? onboarding.personalStep : 'pending'
  return { available: ai.personalSetupAvailable === true, step }
}
