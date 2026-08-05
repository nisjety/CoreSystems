import { createEffect, createSignal, onCleanup, type Accessor } from 'solid-js'
import type { Step, StepTransitionPhase } from '@/features/onboarding/lib/model'

export function createOnboardingStepTransition(currentStep: Accessor<Step>) {
  const [displayedStep, setDisplayedStep] = createSignal<Step>(currentStep())
  const [stepTransitionPhase, setStepTransitionPhase] = createSignal<StepTransitionPhase>('idle')
  let swapTimer: number | undefined
  let enterTimer: number | undefined

  createEffect(() => {
    const nextStep = currentStep()
    if (displayedStep() === nextStep) return

    window.clearTimeout(swapTimer)
    window.clearTimeout(enterTimer)
    setStepTransitionPhase('leaving')

    swapTimer = window.setTimeout(() => {
      setDisplayedStep(nextStep)
      setStepTransitionPhase('entering')
      enterTimer = window.setTimeout(() => setStepTransitionPhase('idle'), 420)
    }, 170)
  })

  onCleanup(() => {
    window.clearTimeout(swapTimer)
    window.clearTimeout(enterTimer)
  })

  return {
    displayedStep,
    stepTransitionPhase,
  }
}
