import type {
  PlanRecommendation,
  PlanRecommendationLocale,
  PlanRecommendationText,
} from '@/features/onboarding/lib/api'

export function recommendationLocale(locale: string): PlanRecommendationLocale {
  return locale === 'en' ? 'en' : 'nb'
}

export function planRecommendationDecisionContext(context: Record<string, unknown>): Record<string, unknown> {
  const decisionContext = { ...context }
  delete decisionContext.locale
  return decisionContext
}

export function planRecommendationContextHash(context: Record<string, unknown>): string {
  return JSON.stringify(planRecommendationDecisionContext(context))
}

export function recommendationText(recommendation: PlanRecommendation): PlanRecommendationText {
  return {
    reason: recommendation.reason,
    summary: recommendation.summary,
    proofPoints: recommendation.proofPoints,
    scopeSignals: recommendation.scopeSignals,
    opportunities: recommendation.opportunities,
  }
}

export function hasRecommendationLocale(
  recommendation: PlanRecommendation | undefined,
  locale: PlanRecommendationLocale,
): boolean {
  if (!recommendation) return false
  return recommendation.locale === locale || Boolean(recommendation.translations?.[locale])
}

export function localizeRecommendation(
  recommendation: PlanRecommendation | undefined,
  locale: PlanRecommendationLocale,
): PlanRecommendation | undefined {
  if (!recommendation) return undefined
  if (recommendation.locale === locale) return recommendation
  const translated = recommendation.translations?.[locale]
  if (!translated) return recommendation
  return {
    ...recommendation,
    ...translated,
    locale,
  }
}

export function withRecommendationTranslation(
  recommendation: PlanRecommendation,
  locale: PlanRecommendationLocale,
  translation: PlanRecommendationText,
): PlanRecommendation {
  return {
    ...recommendation,
    translations: {
      ...recommendation.translations,
      [locale]: translation,
    },
  }
}
