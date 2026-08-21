import { omit } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { buttonClasses, type ButtonShape, type ButtonSize, type ButtonVariant } from '@/shared/ui/button-classes'

type ButtonProps = JSX.ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: ButtonVariant
  size?: ButtonSize
  fullWidth?: boolean
  shape?: ButtonShape
}

export function Button(allProps: ButtonProps) {
  const rest = omit(allProps, 'class', 'type', 'variant', 'size', 'fullWidth', 'shape')

  return (
    <button
      {...rest}
      class={buttonClasses(allProps)}
      type={allProps.type ?? 'button'}
    />
  )
}
