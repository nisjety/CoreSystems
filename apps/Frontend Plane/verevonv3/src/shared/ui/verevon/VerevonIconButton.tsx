import { splitProps, type JSX } from 'solid-js'
import { cn } from '@/shared/lib/cn'

type VerevonIconButtonProps = JSX.ButtonHTMLAttributes<HTMLButtonElement> & {
  tone?: 'surface' | 'ghost' | 'inverted' | 'primary'
  size?: 'xs' | 'sm' | 'md' | 'lg'
  shape?: 'circle' | 'rounded'
}

export function VerevonIconButton(allProps: VerevonIconButtonProps) {
  const [local, props] = splitProps(allProps, ['class', 'type', 'tone', 'size', 'shape'])
  const tone = () => local.tone ?? 'surface'
  const size = () => local.size ?? 'md'
  const shape = () => local.shape ?? 'circle'

  return (
    <button
      {...props}
      type={local.type ?? 'button'}
      class={cn(
        'verevon-icon-button',
        `verevon-icon-button--${tone()}`,
        `verevon-icon-button--${size()}`,
        `verevon-icon-button--${shape()}`,
        local.class,
      )}
    />
  )
}
