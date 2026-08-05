import { splitProps, type JSX } from 'solid-js'
import { cn } from '@/shared/lib/cn'

type VerevonSegmentedProps = JSX.HTMLAttributes<HTMLDivElement>
type VerevonSegmentedButtonProps = JSX.ButtonHTMLAttributes<HTMLButtonElement> & {
  selected?: boolean
}

export function VerevonSegmented(allProps: VerevonSegmentedProps) {
  const [local, props] = splitProps(allProps, ['class'])

  return (
    <div
      {...props}
      class={cn('verevon-segmented', local.class)}
    />
  )
}

export function VerevonSegmentedButton(allProps: VerevonSegmentedButtonProps) {
  const [local, props] = splitProps(allProps, ['class', 'selected', 'type'])

  return (
    <button
      {...props}
      aria-pressed={local.selected}
      class={cn('verevon-segmented-button', local.selected && 'verevon-segmented-button--selected', local.class)}
      type={local.type ?? 'button'}
    />
  )
}
