'use client'

/**
 * Phase 1 onboarding · entry shell rendered by `/login`.
 *
 * Decides between the existing `<AuthPage>` (un-authenticated) and the
 * new `<OnboardingFrame>` (authenticated + not done with the wizard).
 *
 * Detection rules (cheap, no extra round-trip):
 *
 *   1. If we have a fresh `__velion_session_loaded` cookie OR the
 *      `useAuth()` provider reports a user, the user is signed in.
 *   2. If localStorage carries a wizard state, resume there.
 *   3. If nothing is stored, start from `post-signin`.
 *
 * The "verify email" modal sits inside `<AuthPage>` and is untouched —
 * it still renders for users who just signed up and need to confirm
 * their email before they can hit /dashboard. The onboarding wizard
 * only kicks in once the verification step is satisfied (i.e. the user
 * has an active session).
 */

import React from 'react'

import { useAuth } from '../hooks/use-auth'
import { AuthPage } from '../AuthPage'

import { OnboardingFrame } from './OnboardingFrame'
import { AuthShellPlaceholder } from './AuthShellPlaceholder'
import { useOnboardingMachine } from './state/useOnboardingMachine'

interface AuthOrOnboardingPageProps {
  redirectTo?: string
}

export function AuthOrOnboardingPage({ redirectTo }: AuthOrOnboardingPageProps) {
  const auth = useAuth()
  const machine = useOnboardingMachine()

  // Keep all auth hydration and post-callback waiting inside the same
  // auth/onboarding shell. Rendering AuthPage during this window can
  // trigger its standalone redirect behavior before the wizard mounts.
  if (auth.isLoading) {
    return <HydrationPlaceholder />
  }

  // Not signed in → regular sign-in / sign-up forms.
  if (!auth.user) {
    return <AuthPage redirectTo={redirectTo} />
  }

  // Signed in but the wizard cache is still hydrating → keep the
  // chrome visible but avoid AuthPage's post-OAuth callback modal.
  if (!machine.hydrated) {
    return <HydrationPlaceholder />
  }

  return <OnboardingFrame machine={machine} />
}

/**
 * Minimal stand-in shown during hydration windows. Uses the same outer
 * card so the visual handoff to OnboardingFrame is seamless.
 */
function HydrationPlaceholder() {
  return <AuthShellPlaceholder />
}
