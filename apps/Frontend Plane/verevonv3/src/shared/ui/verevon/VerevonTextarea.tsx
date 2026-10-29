import { splitProps, type JSX } from 'solid-js'
import { cn } from '@/shared/lib/cn'

type VerevonTextareaProps = JSX.TextareaHTMLAttributes<HTMLTextAreaElement>

export function VerevonTextarea(allProps: VerevonTextareaProps) {
  const [local, props] = splitProps(allProps, ['class'])

  return (
    <textarea
      {...props}
      class={cn('verevon-field-compact verevon-textarea-compact', local.class)}
    />
  )
}
