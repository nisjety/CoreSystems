import { A } from '@solidjs/router'
import { splitProps, type JSX } from 'solid-js'
import { buttonClasses, type ButtonShape, type ButtonSize, type ButtonVariant } from '@/shared/ui/button-classes'

type ButtonLinkProps = Omit<JSX.AnchorHTMLAttributes<HTMLAnchorElement>, 'href' | 'shape' | 'size'> & {
  href: string
  variant?: ButtonVariant
  size?: ButtonSize
  fullWidth?: boolean
  shape?: ButtonShape
}

export function ButtonLink(allProps: ButtonLinkProps) {
  const [local, props] = splitProps(allProps, ['class', 'href', 'variant', 'size', 'fullWidth', 'shape'])

  return (
    <A
      {...props}
      href={local.href}
      class={buttonClasses(local)}
    />
  )
}
