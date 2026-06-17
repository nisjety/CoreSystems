'use client'

import { useEffect } from 'react'
import { useRouter } from 'next/navigation'

import {
  INITIAL_STATE,
  LEGACY_SLUG_TO_STEP,
  STORAGE_KEY,
  type OnboardingState,
  type OnboardingStep,
} from '@/components/auth/onboarding/state/types'
import { onboardingService } from '@/components/onboarding/services/onboarding-service'

interface OnboardingGuardProps {
  children: React.ReactNode
}

function seedEmbeddedWizard(step: OnboardingStep): void {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY)
    const prev: OnboardingState | null = raw
      ? (JSON.parse(raw) as OnboardingState)
      : null
    const next: OnboardingState = {
      ...(prev ?? INITIAL_STATE),
      step,
      startedAt: prev?.startedAt ?? Date.now(),
    }
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(next))
  } catch {
    // Non-blocking: if localStorage is unavailable, /login still opens
    // the embedded wizard from its default post-sign-in state.
  }
}

export function OnboardingGuard({ children }: OnboardingGuardProps) {
  const router = useRouter()

  useEffect(() => {
    const checkOnboarding = async () => {
      const needsOnboarding = await onboardingService.needsOnboarding()

      if (needsOnboarding) {
        const serverStep = await onboardingService.restoreStepFromServer()
        const currentStep = serverStep ?? onboardingService.getCurrentStep()
        if (currentStep !== 'complete') {
          const targetStep = currentStep
            ? (LEGACY_SLUG_TO_STEP[currentStep] ?? 'organization')
            : 'organization'
          seedEmbeddedWizard(targetStep)
          router.push('/login')
        }
      }
    }

    checkOnboarding()
  }, [router])

  return <>{children}</>
}
