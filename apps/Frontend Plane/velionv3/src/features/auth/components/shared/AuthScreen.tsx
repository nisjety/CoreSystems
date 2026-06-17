import type { JSX } from 'solid-js'
import { onboardingFooterLinks } from '@/features/onboarding/lib/model'
import { VelionScreen } from '@/shared/ui/velion/VelionScreen'
import { VelionSplitFrame } from '@/shared/ui/velion/VelionSplitFrame'

type AuthScreenProps = {
  cardScale: number
  pageVisible: boolean
  left: JSX.Element
  right: JSX.Element
}

export function AuthScreen(props: AuthScreenProps) {
  return (
    <VelionScreen
      rootClass="auth-screen"
      chromeClass="auth-screen__chrome"
      footerLinks={onboardingFooterLinks}
      footerClass="auth-footer-links"
      visible={props.pageVisible}
      visibleClass="auth-screen--visible"
    >
      <VelionSplitFrame
        rootClass="auth-card"
        rootStyle={{
          transform: `scale(${props.cardScale})`,
          'transform-origin': 'center center',
        }}
        leftPaneClass="auth-card__panel auth-card__panel--left"
        rightPaneClass="auth-card__panel auth-card__panel--right"
        left={props.left}
        right={props.right}
      />
    </VelionScreen>
  )
}
