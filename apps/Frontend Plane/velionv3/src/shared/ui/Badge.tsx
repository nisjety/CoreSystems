import type { JSX } from 'solid-js'
import { cn } from '@/shared/lib/cn'

export function Badge(props: { tone?: 'neutral' | 'accent' | 'risk'; children: JSX.Element }) {
  return <span class={cn('badge', `badge--${props.tone ?? 'neutral'}`)}>{props.children}</span>
}
