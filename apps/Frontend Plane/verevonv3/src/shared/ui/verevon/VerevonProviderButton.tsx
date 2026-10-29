import type { JSX } from 'solid-js'
import { VerevonIconButton } from '@/shared/ui/verevon/VerevonIconButton'

type VerevonProviderButtonProps = {
  label: string
  icon: JSX.Element
  active?: boolean
  onClick?: () => void
}

export function VerevonProviderButton(props: VerevonProviderButtonProps) {
  return (
    <VerevonIconButton
      aria-label={props.label}
      class="verevon-provider-button"
      disabled={!props.active}
      onClick={() => props.active && props.onClick?.()}
      size="lg"
      shape="circle"
      tone="surface"
    >
      {props.icon}
    </VerevonIconButton>
  )
}
