import { cn } from '@/shared/lib/cn'

const sentimentConfig: Record<string, { dotClass: string; label: string; textClass: string }> = {
  positive: {
    dotClass: 'verevon-inbox-sentiment__dot--positive',
    label: 'Positive',
    textClass: 'verevon-inbox-sentiment__text--positive',
  },
  neutral: {
    dotClass: 'verevon-inbox-sentiment__dot--neutral',
    label: 'Neutral',
    textClass: 'verevon-inbox-sentiment__text--neutral',
  },
  negative: {
    dotClass: 'verevon-inbox-sentiment__dot--negative',
    label: 'Frustrated',
    textClass: 'verevon-inbox-sentiment__text--negative',
  },
  frustrated: {
    dotClass: 'verevon-inbox-sentiment__dot--frustrated',
    label: 'Very Frustrated',
    textClass: 'verevon-inbox-sentiment__text--frustrated',
  },
}

export function SentimentBadge(props: { sentiment: string }) {
  const config = () => sentimentConfig[props.sentiment] ?? sentimentConfig.neutral!

  return (
    <span class="verevon-inbox-sentiment">
      <span class={cn('verevon-inbox-sentiment__dot', config().dotClass)} />
      <span class={cn('verevon-inbox-sentiment__text', config().textClass)}>{config().label}</span>
    </span>
  )
}
