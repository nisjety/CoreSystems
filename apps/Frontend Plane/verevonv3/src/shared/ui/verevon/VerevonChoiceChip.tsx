import { omit } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { cn } from '@/shared/lib/cn'

type VerevonChoiceChipProps = JSX.ButtonHTMLAttributes<HTMLButtonElement> & {
  selected?: boolean
}

export function VerevonChoiceChip(allProps: VerevonChoiceChipProps) {
  const props = omit(allProps, 'class', 'selected', 'type')

  return (
    <button
      {...props}
      aria-pressed={allProps.selected ? 'true' : 'false'}
      class={cn('verevon-choice-chip', allProps.selected && 'verevon-choice-chip--selected', allProps.class)}
      type={allProps.type ?? 'button'}
    />
  )
}
