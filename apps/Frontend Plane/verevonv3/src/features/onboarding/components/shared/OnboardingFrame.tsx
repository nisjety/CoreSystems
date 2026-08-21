import type { JSX } from '@solidjs/web'
import type { StepTransitionPhase } from '@/features/onboarding/lib/model'
import { VerevonSplitFrame } from '@/shared/ui/verevon/VerevonSplitFrame'

type OnboardingFrameProps = {
  leftPaneHeight?: number
  onLeftPaneRef?: (element: HTMLDivElement) => void
  showScanner: boolean
  stepTransitionPhase: StepTransitionPhase
  left: JSX.Element
  right: JSX.Element
}

export function OnboardingFrame(props: OnboardingFrameProps) {
  return (
    <VerevonSplitFrame
      rootClass="onboarding-frame"
      leftPaneClass="onboarding-pane onboarding-pane--left"
      rightPaneClass="onboarding-pane onboarding-pane--right"
      leftHeightClass="onboarding-pane__content-height"
      leftHeight={props.leftPaneHeight}
      leftContentClass="onboarding-step-shell"
      rightContentClass="onboarding-visual-shell"
      leftContentRef={props.onLeftPaneRef}
      leftLeavingClass="onboarding-step-shell--leaving"
      leftEnteringClass="onboarding-step-shell--entering"
      rightLeavingClass="onboarding-visual-shell--leaving"
      rightEnteringClass="onboarding-visual-shell--entering"
      phase={props.stepTransitionPhase}
      scannerClass="onboarding-scanner"
      showScanner={props.showScanner}
      left={props.left}
      right={props.right}
    />
  )
}
