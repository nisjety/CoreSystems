import { LegacyOnboardingRedirect } from '@/components/auth/onboarding/LegacyOnboardingRedirect'

/**
 * Legacy /onboarding/website route — Phase 1 redirect bridge.
 */
export default function WebsiteOnboardingPage() {
  return <LegacyOnboardingRedirect slug="website" />
}
