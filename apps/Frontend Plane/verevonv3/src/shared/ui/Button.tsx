import { splitProps, type JSX } from 'solid-js'
import { buttonClasses, type ButtonShape, type ButtonSize, type ButtonVariant } from '@/shared/ui/button-classes'

type ButtonProps = JSX.ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: ButtonVariant
  size?: ButtonSize
  fullWidth?: boolean
  shape?: ButtonShape
}

export function Button(allProps: ButtonProps) {
  const [local, props] = splitProps(allProps, ['class', 'type', 'variant', 'size', 'fullWidth', 'shape'])

  return (
    <button
      {...props}
      class={buttonClasses(local)}
      type={local.type ?? 'button'}
    />
  )
}
