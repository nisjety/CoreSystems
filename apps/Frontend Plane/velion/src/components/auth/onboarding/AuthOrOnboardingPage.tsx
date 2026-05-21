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
import { useOnboardingMachine } from './state/useOnboardingMachine'
import { STORAGE_KEY } from './state/types'

interface AuthOrOnboardingPageProps {
  redirectTo?: string
}

/**
 * Once-only check (no hook, no state) for "did this browser previously
 * start the wizard?". Used to skip AuthPage rendering during the auth
 * context's hydration window so the OAuth-callback success modal
 * (`CallbackModal` inside AuthPage) doesn't flash before we swap to
 * the wizard. SSR returns `false`; the very first client render reads
 * localStorage directly and gets the real answer.
 */
function hasCachedWizardState(): boolean {
  if (typeof window === 'undefined') return false
  try {
    return window.localStorage.getItem(STORAGE_KEY) !== null
  } catch {
    return false
  }
}

export function AuthOrOnboardingPage({ redirectTo }: AuthOrOnboardingPageProps) {
  const auth = useAuth()
  const machine = useOnboardingMachine()

  // Skip AuthPage entirely while hydrating IF we already know the
  // user started onboarding here (cached wizard state). Without this
  // the post-social-signin CallbackModal ("Innlogging vellykket!")
  // flashes for ~800ms before the swap because AuthPage was the
  // hydration fallback. The minimal placeholder keeps the cream
  // chrome but renders nothing interactive.
  if (auth.isLoading) {
    if (hasCachedWizardState()) {
      return <HydrationPlaceholder />
    }
    return <AuthPage redirectTo={redirectTo} />
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
 * Minimal stand-in shown during hydration windows where AuthPage's
 * CallbackModal would otherwise flash. Uses the same outer card so the
 * visual handoff to OnboardingFrame is seamless.
 */
function HydrationPlaceholder() {
  return (
    <div className="relative z-[120] flex w-full max-w-[70.5rem] items-center justify-center overflow-visible rounded-[24px] border border-[#D6D2CB] bg-[#EDEBE7] py-32 shadow-[0_20px_50px_rgba(0,0,0,0.14)] xl:max-w-[72rem]">
      <span
        aria-hidden="true"
        className="block h-5 w-5 animate-spin rounded-full border-2 border-[#D6D2CB] border-t-[#1F1B17]"
      />
    </div>
  )
}
