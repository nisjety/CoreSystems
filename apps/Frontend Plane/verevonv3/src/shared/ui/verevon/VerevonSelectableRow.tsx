import { omit } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { cn } from '@/shared/lib/cn'

type VerevonSelectableRowProps = JSX.ButtonHTMLAttributes<HTMLButtonElement> & {
  compact?: boolean
  description?: JSX.Element
  meta?: JSX.Element
  selected?: boolean
  title: JSX.Element
}

export function VerevonSelectableRow(allProps: VerevonSelectableRowProps) {
  const props = omit(allProps,
    'class',
    'compact',
    'description',
    'meta',
    'selected',
    'title',
    'type',
  )

  return (
    <button
      {...props}
      aria-pressed={allProps.selected ? 'true' : 'false'}
      class={cn('verevon-selectable-row', allProps.compact && 'verevon-selectable-row--compact', allProps.class)}
      type={allProps.type ?? 'button'}
    >
      <span>
        <strong>{allProps.title}</strong>
        {allProps.description ? <small>{allProps.description}</small> : null}
      </span>
      {allProps.meta}
    </button>
  )
}
