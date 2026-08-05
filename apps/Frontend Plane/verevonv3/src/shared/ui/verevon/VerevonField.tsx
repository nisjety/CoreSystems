import { Show, type JSX } from 'solid-js'
import { cn } from '@/shared/lib/cn'

type VerevonFieldProps = {
  label: string
  class?: string
  labelClass?: string
  optionalLabel?: string
  children: JSX.Element
}

export function VerevonField(props: VerevonFieldProps) {
  return (
    <label class={props.class}>
      <span class={cn(props.labelClass)}>
        {props.label}
        <Show when={props.optionalLabel}>
          <em>{` (${props.optionalLabel})`}</em>
        </Show>
      </span>
      {props.children}
    </label>
  )
}
