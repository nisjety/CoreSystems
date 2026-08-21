import { omit } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { cn } from '@/shared/lib/cn'

type VerevonIconButtonProps = JSX.ButtonHTMLAttributes<HTMLButtonElement> & {
  tone?: 'surface' | 'ghost' | 'inverted' | 'primary'
  size?: 'xs' | 'sm' | 'md' | 'lg'
  shape?: 'circle' | 'rounded'
}

export function VerevonIconButton(allProps: VerevonIconButtonProps) {
  const props = omit(allProps, 'class', 'type', 'tone', 'size', 'shape')
  const tone = () => allProps.tone ?? 'surface'
  const size = () => allProps.size ?? 'md'
  const shape = () => allProps.shape ?? 'circle'

  return (
    <button
      {...props}
      type={allProps.type ?? 'button'}
      class={cn(
        'verevon-icon-button',
        `verevon-icon-button--${tone()}`,
        `verevon-icon-button--${size()}`,
        `verevon-icon-button--${shape()}`,
        allProps.class,
      )}
    />
  )
}
