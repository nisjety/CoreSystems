import { splitProps, type JSX } from 'solid-js'
import { cn } from '@/shared/lib/cn'

type VerevonChoiceChipProps = JSX.ButtonHTMLAttributes<HTMLButtonElement> & {
  selected?: boolean
}

export function VerevonChoiceChip(allProps: VerevonChoiceChipProps) {
  const [local, props] = splitProps(allProps, ['class', 'selected', 'type'])

  return (
    <button
      {...props}
      aria-pressed={local.selected}
      class={cn('verevon-choice-chip', local.selected && 'verevon-choice-chip--selected', local.class)}
      type={local.type ?? 'button'}
    />
  )
}
