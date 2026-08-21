import type { JSX } from '@solidjs/web'
import { cn } from '@/shared/lib/cn'
import { VerevonField } from '@/shared/ui/verevon/VerevonField'

type OnboardingFieldProps = {
  label: string
  class?: string
  optionalLabel?: string
  children: JSX.Element
}

export function OnboardingField(props: OnboardingFieldProps) {
  return (
    <VerevonField
      class={cn('onboarding-field', props.class)}
      label={props.label}
      optionalLabel={props.optionalLabel}
    >
      {props.children}
    </VerevonField>
  )
}
