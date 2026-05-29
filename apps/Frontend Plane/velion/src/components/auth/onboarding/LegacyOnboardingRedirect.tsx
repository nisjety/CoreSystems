'use client'

/**
 * Phase 1 onboarding · legacy-page bridge.
 *
 * The pre-wizard `/onboarding/<slug>` pages each render this client
 * component, passing the slug they used to own. On mount we
 *
 *   1. Read whatever wizard state is in `localStorage`.
 *   2. Map the legacy slug to the matching new step (see
 *      `LEGACY_SLUG_TO_STEP` in `state/types.ts`).
 *   3. Move that step into the cached state (or seed initial state if
 *      nothing was cached yet).
 *   4. Redirect to `/login` so the wizard picks the user up at the
 *      same logical step.
 *
 * Renders a tiny inline loader so users who deep-link the legacy URL
 * still see motion during the redirect.
 */

import { useEffect } from 'react'
import { useRouter } from 'next/navigation'

import {
  INITIAL_STATE,
  LEGACY_SLUG_TO_STEP,
  STORAGE_KEY,
  type OnboardingState,
} from './state/types'

interface LegacyOnboardingRedirectProps {
  /** Legacy slug from the old URL (e.g. `'organization'`). */
  slug: string
}

export function LegacyOnboardingRedirect({
  slug,
}: LegacyOnboardingRedirectProps) {
  const router = useRouter()

  useEffect(() => {
    const targetStep = LEGACY_SLUG_TO_STEP[slug] ?? 'post-signin'
    try {
      const raw = window.localStorage.getItem(STORAGE_KEY)
      const prev: OnboardingState | null = raw
        ? (JSON.parse(raw) as OnboardingState)
        : null
      const next: OnboardingState = {
        ...(prev ?? INITIAL_STATE),
        step: targetStep,
      }
      window.localStorage.setItem(STORAGE_KEY, JSON.stringify(next))
    } catch {
      // Safari private mode + Lockdown reject setItem; the wizard will
      // start from `post-signin` on landing, which is the right
      // fallback per the user-facing spec.
    }
    router.replace('/login')
  }, [router, slug])

  return (
    <div className="flex min-h-screen items-center justify-center bg-[#EDEBE7]">
      <div className="flex items-center gap-3">
        <span
          aria-hidden="true"
          className="block h-4 w-4 animate-spin rounded-full border-2 border-[#D6D2CB] border-t-[#1F1B17]"
        />
        <span className="font-inter text-[12px] text-[#6B6660]">
          Tar deg tilbake til onboarding …
        </span>
      </div>
    </div>
  )
}
