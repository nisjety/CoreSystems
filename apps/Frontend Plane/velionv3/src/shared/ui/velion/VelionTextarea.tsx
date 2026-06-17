import { splitProps, type JSX } from 'solid-js'
import { cn } from '@/shared/lib/cn'

type VelionTextareaProps = JSX.TextareaHTMLAttributes<HTMLTextAreaElement>

export function VelionTextarea(allProps: VelionTextareaProps) {
  const [local, props] = splitProps(allProps, ['class'])

  return (
    <textarea
      {...props}
      class={cn('velion-field-compact velion-textarea-compact', local.class)}
    />
  )
}
