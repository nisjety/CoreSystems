import { splitProps, type JSX } from 'solid-js'
import { cn } from '@/shared/lib/cn'

type VerevonInputProps = JSX.InputHTMLAttributes<HTMLInputElement>

export function VerevonInput(allProps: VerevonInputProps) {
  const [local, props] = splitProps(allProps, ['class'])

  return (
    <input
      {...props}
      class={cn('verevon-field-compact', local.class)}
    />
  )
}
