import type { JSX } from 'solid-js'
import { VelionIconButton } from '@/shared/ui/velion/VelionIconButton'

type VelionProviderButtonProps = {
  label: string
  icon: JSX.Element
  active?: boolean
  onClick?: () => void
}

export function VelionProviderButton(props: VelionProviderButtonProps) {
  return (
    <VelionIconButton
      aria-label={props.label}
      class="velion-provider-button"
      disabled={!props.active}
      onClick={() => props.active && props.onClick?.()}
      size="lg"
      shape="circle"
      tone="surface"
    >
      {props.icon}
    </VelionIconButton>
  )
}
