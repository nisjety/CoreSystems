import { LegacyOnboardingRedirect } from '@/components/auth/onboarding/LegacyOnboardingRedirect'

/**
 * Legacy /onboarding/profile route. The Phase 1 wizard skips a
 * dedicated profile step (we already have name + email from the auth
 * session), so this slug routes the user to the next live step
 * (organization) per the legacy slug map.
 */
export default function ProfileOnboardingPage() {
  return <LegacyOnboardingRedirect slug="profile" />
}
