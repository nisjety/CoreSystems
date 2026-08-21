import { omit } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { cn } from '@/shared/lib/cn'

type VerevonInputProps = JSX.InputHTMLAttributes<HTMLInputElement>

export function VerevonInput(allProps: VerevonInputProps) {
  const props = omit(allProps, 'class')

  return (
    <input
      {...props}
      class={cn('verevon-field-compact', allProps.class)}
    />
  )
}
