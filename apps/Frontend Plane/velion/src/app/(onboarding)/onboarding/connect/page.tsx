import { LegacyOnboardingRedirect } from '@/components/auth/onboarding/LegacyOnboardingRedirect'

/**
 * Legacy /onboarding/connect route. The Phase 1 wizard has a real
 * connector picker (see `ConnectStep` under `components/auth/
 * onboarding/steps/`) so this slug now bridges directly back into the
 * wizard at the matching step.
 */
export default function ConnectOnboardingPage() {
  return <LegacyOnboardingRedirect slug="connect" />
}
