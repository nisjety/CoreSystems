import { splitProps, type JSX } from 'solid-js'
import { cn } from '@/shared/lib/cn'

type VelionInputProps = JSX.InputHTMLAttributes<HTMLInputElement>

export function VelionInput(allProps: VelionInputProps) {
  const [local, props] = splitProps(allProps, ['class'])

  return (
    <input
      {...props}
      class={cn('velion-field-compact', local.class)}
    />
  )
}
