import { LegacyOnboardingRedirect } from '@/components/auth/onboarding/LegacyOnboardingRedirect'

/**
 * Legacy /onboarding/team route — Phase 1 wizard skips the dedicated
 * team-invite step, so this slug forwards to the live `connect` step
 * per the legacy slug map.
 */
export default function TeamOnboardingPage() {
  return <LegacyOnboardingRedirect slug="team" />
}
