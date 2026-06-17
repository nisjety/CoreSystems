import { splitProps, type JSX } from 'solid-js'
import { VelionTextButton } from '@/shared/ui/velion/VelionTextButton'

type OnboardingLinkButtonProps = JSX.ButtonHTMLAttributes<HTMLButtonElement> & {
  emphasis?: 'link' | 'large'
}

export function OnboardingLinkButton(allProps: OnboardingLinkButtonProps) {
  const [local, props] = splitProps(allProps, ['class', 'emphasis', 'type'])

  return (
    <VelionTextButton
      {...props}
      class={local.class}
      emphasis={local.emphasis === 'large' ? 'large' : 'default'}
      type={local.type ?? 'button'}
    />
  )
}
