import { AuthOrOnboardingPage } from '@/components/auth/onboarding/AuthOrOnboardingPage';

type LoginPageProps = {
  searchParams?: Promise<{ redirect?: string }>;
};

/**
 * Phase 1 onboarding · entry. Renders the existing sign-in / sign-up
 * forms when the visitor is unauthenticated, and swaps to the in-auth-
 * page wizard (`OnboardingFrame`) once a session exists and the
 * onboarding cache says we're not done. All wizard UI lives under
 * `components/auth/onboarding/`.
 */
export default async function LoginPage({ searchParams }: LoginPageProps) {
  const resolvedSearchParams = searchParams ? await searchParams : undefined;

  return <AuthOrOnboardingPage redirectTo={resolvedSearchParams?.redirect} />;
}
