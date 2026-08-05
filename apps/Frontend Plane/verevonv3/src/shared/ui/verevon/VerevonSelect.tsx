import { splitProps, type JSX } from 'solid-js'
import { cn } from '@/shared/lib/cn'

type VerevonSelectProps = JSX.SelectHTMLAttributes<HTMLSelectElement>

export function VerevonSelect(allProps: VerevonSelectProps) {
  const [local, props] = splitProps(allProps, ['class'])

  return (
    <select
      {...props}
      class={cn('verevon-field-compact', local.class)}
    />
  )
}
