import { omit } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { buttonClasses, type ButtonShape, type ButtonSize, type ButtonVariant } from '@/shared/ui/button-classes'

type ButtonLinkProps = Omit<JSX.AnchorHTMLAttributes<HTMLAnchorElement>, 'href' | 'shape' | 'size'> & {
  href: string
  variant?: ButtonVariant
  size?: ButtonSize
  fullWidth?: boolean
  shape?: ButtonShape
}

export function ButtonLink(allProps: ButtonLinkProps) {
  const rest = omit(allProps, 'class', 'href', 'variant', 'size', 'fullWidth', 'shape')

  return (
    <a
      {...rest}
      href={allProps.href}
      link
      class={buttonClasses(allProps)}
    />
  )
}
