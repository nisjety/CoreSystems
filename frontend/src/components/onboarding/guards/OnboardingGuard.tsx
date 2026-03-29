'use client'

import { useEffect } from 'react'
import { useRouter } from 'next/navigation'
import { onboardingService } from '@/components/onboarding/services/onboarding-service'

interface OnboardingGuardProps {
  children: React.ReactNode
}

export function OnboardingGuard({ children }: OnboardingGuardProps) {
  const router = useRouter()

  useEffect(() => {
    const checkOnboarding = async () => {
      const needsOnboarding = await onboardingService.needsOnboarding()

      if (needsOnboarding) {
        const currentStep = onboardingService.getCurrentStep()
        if (!currentStep) {
          router.push('/onboarding/profile')
        } else if (currentStep !== 'complete') {
          router.push(`/onboarding/${currentStep}`)
        }
      }
    }

    checkOnboarding()
  }, [router])

  return <>{children}</>
}
