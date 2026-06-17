import { splitProps, type JSX } from 'solid-js'
import { cn } from '@/shared/lib/cn'

type VelionTextButtonProps = JSX.ButtonHTMLAttributes<HTMLButtonElement> & {
  emphasis?: 'default' | 'large'
}

export function VelionTextButton(allProps: VelionTextButtonProps) {
  const [local, props] = splitProps(allProps, ['class', 'emphasis', 'type'])

  return (
    <button
      {...props}
      class={cn('velion-text-button', local.emphasis === 'large' && 'velion-text-button--large', local.class)}
      type={local.type ?? 'button'}
    />
  )
}
