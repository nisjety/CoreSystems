import { ChevronDown, Globe } from '@/shared/icons'
import { cn } from '@/shared/lib/cn'

type VerevonLanguageButtonProps = {
  code: string
  onClick?: () => void
  class?: string
  ariaLabel?: string
  ariaControls?: string
  ariaExpanded?: boolean
  hasMenu?: boolean
}

export function VerevonLanguageButton(props: VerevonLanguageButtonProps) {
  return (
    <button
      type="button"
      class={cn('onboarding-language-pill', props.class)}
      onClick={() => props.onClick?.()}
      aria-label={props.ariaLabel ?? 'Language'}
      aria-haspopup={props.hasMenu === false ? undefined : 'menu'}
      aria-expanded={props.hasMenu === false ? undefined : (props.ariaExpanded ? 'true' : 'false')}
      aria-controls={props.hasMenu === false ? undefined : props.ariaControls}
    >
      <Globe size={16} />
      <span>{props.code}</span>
      <ChevronDown size={14} />
    </button>
  )
}
