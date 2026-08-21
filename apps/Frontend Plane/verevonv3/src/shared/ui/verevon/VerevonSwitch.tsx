import { Switch as KobalteSwitch } from '@kobalte/core/switch'
import { cn } from '@/shared/lib/cn'

type VerevonSwitchProps = {
  checked?: boolean
  class?: string
  disabled?: boolean
  label: string
  onChange?: (checked: boolean) => void
}

export function VerevonSwitch(allProps: VerevonSwitchProps) {
  return (
    <KobalteSwitch
      checked={allProps.checked}
      class={cn('verevon-switch', allProps.class)}
      disabled={allProps.disabled}
      onChange={allProps.onChange}
    >
      <KobalteSwitch.Input aria-label={allProps.label} />
      <KobalteSwitch.Control class="verevon-switch__control">
        <KobalteSwitch.Thumb class="verevon-switch__thumb" />
      </KobalteSwitch.Control>
    </KobalteSwitch>
  )
}
