import { For } from 'solid-js'

type VerevonStepDotsProps<TStep extends string> = {
  steps: readonly TStep[]
  currentStep: TStep
  ariaLabel: string
  stepLabel?: string
  onSelectStep: (step: TStep) => void
}

export function VerevonStepDots<TStep extends string>(props: VerevonStepDotsProps<TStep>) {
  return (
    <nav class="onboarding-dots" aria-label={props.ariaLabel}>
      <For each={props.steps}>
        {(step, index) => (
          <button
            type="button"
            class={['onboarding-dots__dot', { 'onboarding-dots__dot--active': props.currentStep === step }]}
            aria-current={props.currentStep === step ? 'step' : undefined}
            aria-label={`${props.stepLabel ?? 'Step'} ${index() + 1}`}
            onClick={() => props.onSelectStep(step)}
          />
        )}
      </For>
    </nav>
  )
}
