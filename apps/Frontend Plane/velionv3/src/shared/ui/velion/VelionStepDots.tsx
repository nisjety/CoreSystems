import { For } from 'solid-js'

type VelionStepDotsProps<TStep extends string> = {
  steps: readonly TStep[]
  currentStep: TStep
  ariaLabel: string
  onSelectStep: (step: TStep) => void
}

export function VelionStepDots<TStep extends string>(props: VelionStepDotsProps<TStep>) {
  return (
    <nav class="onboarding-dots" aria-label={props.ariaLabel}>
      <For each={props.steps}>
        {(step, index) => (
          <button
            type="button"
            class="onboarding-dots__dot"
            classList={{ 'onboarding-dots__dot--active': props.currentStep === step }}
            aria-current={props.currentStep === step ? 'step' : undefined}
            aria-label={`Step ${index() + 1}`}
            onClick={() => props.onSelectStep(step)}
          />
        )}
      </For>
    </nav>
  )
}
