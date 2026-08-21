import { omit } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { cn } from '@/shared/lib/cn'

type VerevonTextareaProps = JSX.TextareaHTMLAttributes<HTMLTextAreaElement>

export function VerevonTextarea(allProps: VerevonTextareaProps) {
  const rest = omit(allProps, 'class')

  return (
    <textarea
      {...rest}
      class={cn('verevon-field-compact verevon-textarea-compact', allProps.class)}
    />
  )
}
