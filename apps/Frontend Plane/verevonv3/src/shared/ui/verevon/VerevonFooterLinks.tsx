import { For } from 'solid-js'
import { cn } from '@/shared/lib/cn'

type VerevonFooterLinksProps = {
  links: readonly string[]
  class?: string
  buttonClass?: string
}

export function VerevonFooterLinks(props: VerevonFooterLinksProps) {
  return (
    <div class={cn('onboarding-footer-links', props.class)}>
      <For each={props.links}>
        {(item) => (
          <button type="button" class={props.buttonClass}>
            {item}
          </button>
        )}
      </For>
    </div>
  )
}
