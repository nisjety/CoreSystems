import { A } from '@solidjs/router'
import { ArrowLeft } from 'lucide-solid'
import { Show } from 'solid-js'
import { cn } from '@/shared/lib/cn'

type VerevonBackButtonProps = {
  href?: string
  onClick?: () => void
  label?: string
  class?: string
}

export function VerevonBackButton(props: VerevonBackButtonProps) {
  const label = () => props.label ?? 'Tilbake'

  return (
    <Show
      when={props.onClick}
      fallback={
        <A href={props.href ?? '/'} class={cn('onboarding-back', props.class)}>
          <ArrowLeft size={16} />
          {label()}
        </A>
      }
    >
      <button type="button" class={cn('onboarding-back', props.class)} onClick={() => props.onClick?.()}>
        <ArrowLeft size={16} />
        {label()}
      </button>
    </Show>
  )
}
