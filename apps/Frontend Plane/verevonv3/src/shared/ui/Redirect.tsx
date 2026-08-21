import { useNavigate, type NavigateOptions } from '@solidjs/router'
import { createEffect } from 'solid-js'

/**
 * Solid Router 2 removes the declarative <Navigate> component with no
 * component-for-component replacement — the migration guide's own example
 * calls useNavigate() imperatively instead. This wraps that in a component so
 * call sites that rendered <Navigate href={...} /> conditionally in JSX keep
 * the same shape.
 */
export function Redirect(props: { href: string; options?: Partial<NavigateOptions> }) {
  const navigate = useNavigate()

  createEffect(
    () => props.href,
    (href) => navigate(href, { replace: true, ...props.options }),
  )

  return null
}
