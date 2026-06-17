import type { JSX } from 'solid-js'
import { cn } from '@/shared/lib/cn'
import { VelionField } from '@/shared/ui/velion/VelionField'

type OnboardingFieldProps = {
  label: string
  class?: string
  optionalLabel?: string
  children: JSX.Element
}

export function OnboardingField(props: OnboardingFieldProps) {
  return (
    <VelionField
      class={cn('onboarding-field', props.class)}
      label={props.label}
      optionalLabel={props.optionalLabel}
    >
      {props.children}
    </VelionField>
  )
}
