import { splitProps, type JSX } from 'solid-js'
import { cn } from '@/shared/lib/cn'

type VelionSelectProps = JSX.SelectHTMLAttributes<HTMLSelectElement>

export function VelionSelect(allProps: VelionSelectProps) {
  const [local, props] = splitProps(allProps, ['class'])

  return (
    <select
      {...props}
      class={cn('velion-field-compact', local.class)}
    />
  )
}
