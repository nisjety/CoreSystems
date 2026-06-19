import { ChevronDown, Globe } from 'lucide-solid'
import { cn } from '@/shared/lib/cn'

type VelionLanguageButtonProps = {
  code: string
  onClick?: () => void
  class?: string
  ariaLabel?: string
  ariaControls?: string
  ariaExpanded?: boolean
}

export function VelionLanguageButton(props: VelionLanguageButtonProps) {
  return (
    <button
      type="button"
      class={cn('onboarding-language-pill', props.class)}
      onClick={() => props.onClick?.()}
      aria-label={props.ariaLabel ?? 'Language'}
      aria-haspopup="menu"
      aria-expanded={props.ariaExpanded}
      aria-controls={props.ariaControls}
    >
      <Globe size={16} />
      <span>{props.code}</span>
      <ChevronDown size={14} />
    </button>
  )
}
