import { Show } from 'solid-js'
import type { Step } from '@/features/onboarding/lib/model'
import { VelionBackButton } from '@/shared/ui/velion/VelionBackButton'
import { VelionLanguageButton } from '@/shared/ui/velion/VelionLanguageButton'
import { VelionStepDots } from '@/shared/ui/velion/VelionStepDots'
import { VelionStepPill } from '@/shared/ui/velion/VelionStepPill'

type OnboardingTopbarProps = {
  steps: readonly Step[]
  currentStep: Step
  currentStepIndex: number
  visibleStepNumber: number
  onBack?: () => void
  onSelectStep: (step: Step) => void
  backHref?: string
}

export function OnboardingTopbar(props: OnboardingTopbarProps) {
  return (
    <div class="onboarding-topbar">
      <div class="onboarding-topbar__slot">
        <Show
          when={props.onBack}
          fallback={
            <VelionBackButton href={props.backHref ?? '/'} />
          }
        >
          <VelionBackButton onClick={props.onBack} />
        </Show>
      </div>

      <VelionStepDots
        steps={props.steps.slice(1)}
        currentStep={props.currentStep}
        ariaLabel="Onboarding steps"
        onSelectStep={props.onSelectStep}
      />

      <div class="onboarding-topbar__slot onboarding-topbar__slot--end">
        <div class="onboarding-meta-pills">
          <VelionLanguageButton code="NB" />
          <VelionStepPill current={props.visibleStepNumber} total={props.steps.length - 1} />
        </div>
      </div>
    </div>
  )
}
