import { cn } from '@/shared/lib/cn'

export type ButtonVariant = 'primary' | 'secondary' | 'ghost' | 'soft' | 'dark' | 'outline'
export type ButtonSize = 'xs' | 'sm' | 'md' | 'lg'
export type ButtonShape = 'rounded' | 'pill'

export type ButtonClassOptions = {
  class?: string
  fullWidth?: boolean
  shape?: ButtonShape
  size?: ButtonSize
  variant?: ButtonVariant
}

export function buttonClasses(options: ButtonClassOptions = {}) {
  const variant = options.variant ?? 'secondary'
  const size = options.size ?? 'md'
  const shape = options.shape ?? 'rounded'

  return cn(
    'button',
    `button--${variant}`,
    `button--${size}`,
    shape === 'pill' && 'button--pill',
    options.fullWidth && 'button--full',
    options.class,
  )
}
