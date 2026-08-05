import { splitProps, type JSX } from 'solid-js'
import { cn } from '@/shared/lib/cn'

type VerevonTextButtonProps = JSX.ButtonHTMLAttributes<HTMLButtonElement> & {
  emphasis?: 'default' | 'large'
}

export function VerevonTextButton(allProps: VerevonTextButtonProps) {
  const [local, props] = splitProps(allProps, ['class', 'emphasis', 'type'])

  return (
    <button
      {...props}
      class={cn('verevon-text-button', local.emphasis === 'large' && 'verevon-text-button--large', local.class)}
      type={local.type ?? 'button'}
    />
  )
}
