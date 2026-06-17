// @vitest-environment jsdom

import { fireEvent, render, screen } from '@solidjs/testing-library'
import { describe, expect, it, vi } from 'vitest'
import { OnboardingTopbar } from '@/features/onboarding/components/shared/OnboardingTopbar'
import { onboardingSteps } from '@/features/onboarding/lib/model'

describe('OnboardingTopbar', () => {
  it('shows the current progress and routes step selection through one shared chrome component', () => {
    const onBack = vi.fn()
    const onSelectStep = vi.fn()

    render(() => (
      <OnboardingTopbar
        steps={onboardingSteps}
        currentStep="organization"
        currentStepIndex={2}
        visibleStepNumber={3}
        onBack={onBack}
        onSelectStep={onSelectStep}
      />
    ))

    expect(screen.getByText('Steg 3 av 6')).toBeTruthy()

    fireEvent.click(screen.getByLabelText('Step 1'))
    expect(onSelectStep).toHaveBeenCalledWith('website')

    fireEvent.click(screen.getByRole('button', { name: 'Tilbake' }))
    expect(onBack).toHaveBeenCalledTimes(1)
  })
})
