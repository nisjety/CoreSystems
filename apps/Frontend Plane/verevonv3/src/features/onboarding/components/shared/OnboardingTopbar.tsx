import { Show } from 'solid-js'
import type { Step } from '@/features/onboarding/lib/model'
import { useI18n } from '@/shared/i18n'
import { VerevonBackButton } from '@/shared/ui/verevon/VerevonBackButton'
import { VerevonLanguageButton } from '@/shared/ui/verevon/VerevonLanguageButton'
import { VerevonStepDots } from '@/shared/ui/verevon/VerevonStepDots'
import { VerevonStepPill } from '@/shared/ui/verevon/VerevonStepPill'

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
            <VerevonBackButton href={props.backHref ?? '/'} />
          }
        >
          <VerevonBackButton onClick={props.onBack} />
        </Show>
      </div>

      <VerevonStepDots
        steps={props.steps.slice(1)}
        currentStep={props.currentStep}
        ariaLabel={i18n.tr('Onboarding-steg', 'Onboarding steps')}
        stepLabel={i18n.tr('Steg', 'Step')}
        onSelectStep={props.onSelectStep}
      />

      <div class="onboarding-topbar__slot onboarding-topbar__slot--end">
        <div class="onboarding-meta-pills">
          <VerevonLanguageButton
            code={i18n.localeCode()}
            hasMenu={false}
            ariaLabel={i18n.tr(`Bytt språk til ${i18n.nextLocaleName()}`, `Switch language to ${i18n.nextLocaleName()}`)}
            onClick={i18n.toggleLocale}
          />
          <VerevonStepPill
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
