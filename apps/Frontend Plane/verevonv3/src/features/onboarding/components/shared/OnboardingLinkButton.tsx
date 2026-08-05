import { splitProps, type JSX } from 'solid-js'
import { VerevonTextButton } from '@/shared/ui/verevon/VerevonTextButton'

type OnboardingLinkButtonProps = JSX.ButtonHTMLAttributes<HTMLButtonElement> & {
  emphasis?: 'link' | 'large'
}

export function OnboardingLinkButton(allProps: OnboardingLinkButtonProps) {
  const [local, props] = splitProps(allProps, ['class', 'emphasis', 'type'])

  return (
    <VerevonTextButton
      {...props}
      class={local.class}
      emphasis={local.emphasis === 'large' ? 'large' : 'default'}
      type={local.type ?? 'button'}
    />
  )
}
