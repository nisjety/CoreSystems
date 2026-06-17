import { cn } from '@/shared/lib/cn'

const sentimentConfig: Record<string, { dotClass: string; label: string; textClass: string }> = {
  positive: {
    dotClass: 'velion-inbox-sentiment__dot--positive',
    label: 'Positive',
    textClass: 'velion-inbox-sentiment__text--positive',
  },
  neutral: {
    dotClass: 'velion-inbox-sentiment__dot--neutral',
    label: 'Neutral',
    textClass: 'velion-inbox-sentiment__text--neutral',
  },
  negative: {
    dotClass: 'velion-inbox-sentiment__dot--negative',
    label: 'Frustrated',
    textClass: 'velion-inbox-sentiment__text--negative',
  },
  frustrated: {
    dotClass: 'velion-inbox-sentiment__dot--frustrated',
    label: 'Very Frustrated',
    textClass: 'velion-inbox-sentiment__text--frustrated',
  },
}

export function SentimentBadge(props: { sentiment: string }) {
  const config = () => sentimentConfig[props.sentiment] ?? sentimentConfig.neutral!

  return (
    <span class="velion-inbox-sentiment">
      <span class={cn('velion-inbox-sentiment__dot', config().dotClass)} />
      <span class={cn('velion-inbox-sentiment__text', config().textClass)}>{config().label}</span>
    </span>
  )
}
