import type { JSX } from '@solidjs/web'
import { Bot, FileText } from '@/shared/icons'
import { cn } from '@/shared/lib/cn'
import type { BrandMarkId } from '@/features/agents/lib/verevon-workflow-builder-data'

export function WorkflowBrandMark(props: {
  brand: BrandMarkId
  size?: 'small' | 'medium' | 'large'
}) {
  const size = () => props.size ?? 'medium'
  const label = (): JSX.Element | string => {
    if (props.brand === 'openai') {
      return <Bot class={size() === 'large' ? 'size-6' : 'size-4'} strokeWidth={2} />
    }

    if (props.brand === 'sheets') {
      return <FileText class={size() === 'large' ? 'size-6' : 'size-4'} strokeWidth={2} />
    }

    return {
      facebook: 'f',
      instagram: 'ig',
      linkedin: 'in',
      gemini: '*',
      grok: 'G',
      perplexity: 'P',
      drive: 'D',
      slides: 'S',
      docs: 'D',
      slack: '#',
      notion: 'N',
    }[props.brand]
  }

  return (
    <span
      class={cn(
        'grid place-items-center rounded-[8px] font-semibold leading-none',
        size() === 'small'
          ? 'size-5 text-[11px]'
          : size() === 'large'
            ? 'size-9 text-[18px]'
            : 'size-8 text-[14px]',
        props.brand === 'instagram'
          ? 'bg-[radial-gradient(circle_at_30%_30%,#FFE66E_0%,#FF5C5C_34%,#C832D8_66%,#111111_100%)] text-white'
          : props.brand === 'facebook'
            ? 'bg-[#111111] text-white'
            : props.brand === 'linkedin'
              ? 'bg-[#111111] text-white'
              : props.brand === 'sheets'
                ? 'bg-[#2FBF71] text-white'
                : props.brand === 'gemini'
                  ? 'bg-white text-[#111111]'
                  : props.brand === 'grok'
                    ? 'bg-white text-[#111111]'
                    : props.brand === 'perplexity'
                      ? 'bg-white text-[#20898C]'
                      : props.brand === 'drive'
                        ? 'bg-white text-[#1EA362]'
                        : props.brand === 'slides'
                          ? 'bg-white text-[#F4B400]'
                          : props.brand === 'docs'
                            ? 'bg-white text-[#111111]'
                            : props.brand === 'slack'
                              ? 'bg-white text-[#111111]'
                              : 'bg-white text-[#111111]',
      )}
    >
      {label()}
    </span>
  )
}
