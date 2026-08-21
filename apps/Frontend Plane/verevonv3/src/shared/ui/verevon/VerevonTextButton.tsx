import { omit } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { cn } from '@/shared/lib/cn'

type VerevonTextButtonProps = JSX.ButtonHTMLAttributes<HTMLButtonElement> & {
  emphasis?: 'default' | 'large'
}

export function VerevonTextButton(allProps: VerevonTextButtonProps) {
  const rest = omit(allProps, 'class', 'emphasis', 'type')

  return (
    <button
      {...rest}
      class={cn('verevon-text-button', allProps.emphasis === 'large' && 'verevon-text-button--large', allProps.class)}
      type={allProps.type ?? 'button'}
    />
  )
}
