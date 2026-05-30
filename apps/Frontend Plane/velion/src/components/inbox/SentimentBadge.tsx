'use client'

type Sentiment = 'positive' | 'neutral' | 'negative' | 'frustrated'

interface SentimentBadgeProps {
  sentiment: Sentiment | string
}

const SENTIMENT_CONFIG: Record<string, { dotClass: string; label: string; textClass: string }> = {
  positive: {
    dotClass: 'bg-green-500',
    label: 'Positive',
    textClass: 'text-green-700',
  },
  neutral: {
    dotClass: 'bg-gray-400',
    label: 'Neutral',
    textClass: 'text-gray-600',
  },
  negative: {
    dotClass: 'bg-orange-400',
    label: 'Frustrated',
    textClass: 'text-orange-700',
  },
  frustrated: {
    dotClass: 'bg-red-500',
    label: 'Very Frustrated',
    textClass: 'text-red-700',
  },
}

export function SentimentBadge({ sentiment }: SentimentBadgeProps) {
  const config = SENTIMENT_CONFIG[sentiment] ?? SENTIMENT_CONFIG['neutral']

  return (
    <span className="inline-flex items-center gap-1">
      <span className={`inline-block size-1.5 rounded-full ${config.dotClass}`} />
      <span className={`text-[11px] font-medium ${config.textClass}`}>{config.label}</span>
    </span>
  )
}
