import { LegacyOnboardingRedirect } from '@/components/auth/onboarding/LegacyOnboardingRedirect'

/**
 * Legacy /onboarding/complete route — Phase 1 redirect bridge.
 * Maps to the new `assembly` step so users who deep-link the old
 * completion page land on the new dashboard-assembly finale.
 */
export default function CompleteOnboardingPage() {
  return <LegacyOnboardingRedirect slug="complete" />
}
