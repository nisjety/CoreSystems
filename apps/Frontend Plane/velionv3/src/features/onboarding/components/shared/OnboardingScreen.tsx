import type { JSX } from 'solid-js'
import { onboardingFooterLinks, type Step } from '@/features/onboarding/lib/model'
import { OnboardingTopbar } from '@/features/onboarding/components/shared/OnboardingTopbar'
import { VelionScreen } from '@/shared/ui/velion/VelionScreen'

type OnboardingScreenProps = {
  paywall?: boolean
  steps: readonly Step[]
  currentStep: Step
  currentStepIndex: number
  visibleStepNumber: number
  onBack?: () => void
  onSelectStep: (step: Step) => void
  backHref?: string
  screenStyle?: JSX.CSSProperties
  chromeStyle?: JSX.CSSProperties
  children: JSX.Element
}

export function OnboardingScreen(props: OnboardingScreenProps) {
  return (
    <VelionScreen
      rootClass={props.paywall ? 'onboarding-screen onboarding-screen--paywall' : 'onboarding-screen'}
      chromeClass={props.paywall ? 'onboarding-screen__chrome onboarding-screen__chrome--paywall' : 'onboarding-screen__chrome'}
      footerLinks={onboardingFooterLinks}
      rootStyle={props.screenStyle}
      chromeStyle={props.chromeStyle}
    >
      <>
        <OnboardingTopbar
          steps={props.steps}
          currentStep={props.currentStep}
          currentStepIndex={props.currentStepIndex}
          visibleStepNumber={props.visibleStepNumber}
          onBack={props.onBack}
          onSelectStep={props.onSelectStep}
          backHref={props.backHref}
        />
        {props.children}
      </>
    </VelionScreen>
  )
}
