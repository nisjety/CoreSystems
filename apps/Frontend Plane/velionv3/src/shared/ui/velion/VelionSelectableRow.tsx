import { splitProps, type JSX } from 'solid-js'
import { cn } from '@/shared/lib/cn'

type VelionSelectableRowProps = JSX.ButtonHTMLAttributes<HTMLButtonElement> & {
  compact?: boolean
  description?: JSX.Element
  meta?: JSX.Element
  selected?: boolean
  title: JSX.Element
}

export function VelionSelectableRow(allProps: VelionSelectableRowProps) {
  const [local, props] = splitProps(allProps, [
    'class',
    'compact',
    'description',
    'meta',
    'selected',
    'title',
    'type',
  ])

  return (
    <button
      {...props}
      aria-pressed={local.selected}
      class={cn('velion-selectable-row', local.compact && 'velion-selectable-row--compact', local.class)}
      type={local.type ?? 'button'}
    >
      <span>
        <strong>{local.title}</strong>
        {local.description ? <small>{local.description}</small> : null}
      </span>
      {local.meta}
    </button>
  )
}
