import { omit } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { VerevonTextButton } from '@/shared/ui/verevon/VerevonTextButton'

type OnboardingLinkButtonProps = JSX.ButtonHTMLAttributes<HTMLButtonElement> & {
  emphasis?: 'link' | 'large'
}

export function OnboardingLinkButton(allProps: OnboardingLinkButtonProps) {
  const props = omit(allProps, 'class', 'emphasis', 'type')

  return (
    <VerevonTextButton
      {...props}
      class={allProps.class}
      emphasis={allProps.emphasis === 'large' ? 'large' : 'default'}
      type={allProps.type ?? 'button'}
    />
  )
}
