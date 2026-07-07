import { Show } from 'solid-js'
import type { Step } from '@/features/onboarding/lib/model'
import { useI18n } from '@/shared/i18n'
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
  const i18n = useI18n()

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
        ariaLabel={i18n.tr('Onboarding-steg', 'Onboarding steps')}
        stepLabel={i18n.tr('Steg', 'Step')}
        onSelectStep={props.onSelectStep}
      />

      <div class="onboarding-topbar__slot onboarding-topbar__slot--end">
        <div class="onboarding-meta-pills">
          <VelionLanguageButton
            code={i18n.localeCode()}
            hasMenu={false}
            ariaLabel={i18n.tr(`Bytt språk til ${i18n.nextLocaleName()}`, `Switch language to ${i18n.nextLocaleName()}`)}
            onClick={i18n.toggleLocale}
          />
          <VelionStepPill
            current={props.visibleStepNumber}
            total={props.steps.length - 1}
            label={i18n.tr('Steg', 'Step')}
            ofLabel={i18n.tr('av', 'of')}
          />
        </div>
      </div>
    </div>
  )
}
