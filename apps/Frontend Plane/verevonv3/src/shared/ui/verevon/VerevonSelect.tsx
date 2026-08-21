import { omit } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { cn } from '@/shared/lib/cn'

type VerevonSelectProps = JSX.SelectHTMLAttributes<HTMLSelectElement>

export function VerevonSelect(allProps: VerevonSelectProps) {
  const props = omit(allProps, 'class')

  return (
    <select
      {...props}
      class={cn('verevon-field-compact', allProps.class)}
    />
  )
}
