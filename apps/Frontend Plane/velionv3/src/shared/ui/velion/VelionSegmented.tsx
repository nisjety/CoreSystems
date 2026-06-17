import { splitProps, type JSX } from 'solid-js'
import { cn } from '@/shared/lib/cn'

type VelionSegmentedProps = JSX.HTMLAttributes<HTMLDivElement>
type VelionSegmentedButtonProps = JSX.ButtonHTMLAttributes<HTMLButtonElement> & {
  selected?: boolean
}

export function VelionSegmented(allProps: VelionSegmentedProps) {
  const [local, props] = splitProps(allProps, ['class'])

  return (
    <div
      {...props}
      class={cn('velion-segmented', local.class)}
    />
  )
}

export function VelionSegmentedButton(allProps: VelionSegmentedButtonProps) {
  const [local, props] = splitProps(allProps, ['class', 'selected', 'type'])

  return (
    <button
      {...props}
      aria-pressed={local.selected}
      class={cn('velion-segmented-button', local.selected && 'velion-segmented-button--selected', local.class)}
      type={local.type ?? 'button'}
    />
  )
}
