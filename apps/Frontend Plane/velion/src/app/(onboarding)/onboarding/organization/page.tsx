import { LegacyOnboardingRedirect } from '@/components/auth/onboarding/LegacyOnboardingRedirect'

/**
 * Legacy /onboarding/organization route — kept so any cached URL or
 * bookmark from the pre-Phase-1 wizard still works. Hands off to the
 * new auth-embedded wizard via `localStorage` and redirects to /login.
 */
export default function OrganizationOnboardingPage() {
  return <LegacyOnboardingRedirect slug="organization" />
}
