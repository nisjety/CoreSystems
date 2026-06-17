import { splitProps, type JSX } from 'solid-js'
import { cn } from '@/shared/lib/cn'

type VelionChoiceChipProps = JSX.ButtonHTMLAttributes<HTMLButtonElement> & {
  selected?: boolean
}

export function VelionChoiceChip(allProps: VelionChoiceChipProps) {
  const [local, props] = splitProps(allProps, ['class', 'selected', 'type'])

  return (
    <button
      {...props}
      aria-pressed={local.selected}
      class={cn('velion-choice-chip', local.selected && 'velion-choice-chip--selected', local.class)}
      type={local.type ?? 'button'}
    />
  )
}
