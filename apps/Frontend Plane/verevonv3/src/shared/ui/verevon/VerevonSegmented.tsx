import { omit } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { cn } from '@/shared/lib/cn'

type VerevonSegmentedProps = JSX.HTMLAttributes<HTMLDivElement>
type VerevonSegmentedButtonProps = JSX.ButtonHTMLAttributes<HTMLButtonElement> & {
  selected?: boolean
}

export function VerevonSegmented(allProps: VerevonSegmentedProps) {
  const props = omit(allProps, 'class')

  return (
    <div
      {...props}
      class={cn('verevon-segmented', allProps.class)}
    />
  )
}

export function VerevonSegmentedButton(allProps: VerevonSegmentedButtonProps) {
  const props = omit(allProps, 'class', 'selected', 'type')

  return (
    <button
      {...props}
      aria-pressed={allProps.selected ? 'true' : 'false'}
      class={cn('verevon-segmented-button', allProps.selected && 'verevon-segmented-button--selected', allProps.class)}
      type={allProps.type ?? 'button'}
    />
  )
}
