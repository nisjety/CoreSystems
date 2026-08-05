import { Switch as KobalteSwitch } from '@kobalte/core/switch'
import { splitProps } from 'solid-js'
import { cn } from '@/shared/lib/cn'

type VerevonSwitchProps = {
  checked?: boolean
  class?: string
  disabled?: boolean
  label: string
  onChange?: (checked: boolean) => void
}

export function VerevonSwitch(allProps: VerevonSwitchProps) {
  const [local] = splitProps(allProps, ['checked', 'class', 'disabled', 'label', 'onChange'])

  return (
    <KobalteSwitch
      checked={local.checked}
      class={cn('verevon-switch', local.class)}
      disabled={local.disabled}
      onChange={local.onChange}
    >
      <KobalteSwitch.Input aria-label={local.label} />
      <KobalteSwitch.Control class="verevon-switch__control">
        <KobalteSwitch.Thumb class="verevon-switch__thumb" />
      </KobalteSwitch.Control>
    </KobalteSwitch>
  )
}
