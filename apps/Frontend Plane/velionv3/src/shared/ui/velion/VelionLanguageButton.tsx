import { ChevronDown, Globe } from 'lucide-solid'
import { cn } from '@/shared/lib/cn'

type VelionLanguageButtonProps = {
  code: string
  onClick?: () => void
  class?: string
  ariaLabel?: string
}

export function VelionLanguageButton(props: VelionLanguageButtonProps) {
  return (
    <button
      type="button"
      class={cn('onboarding-language-pill', props.class)}
      onClick={() => props.onClick?.()}
      aria-label={props.ariaLabel ?? 'Language'}
    >
      <Globe size={16} />
      <span>{props.code}</span>
      <ChevronDown size={14} />
    </button>
  )
}
