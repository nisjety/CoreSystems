export type PlanRecommendationLocale = 'nb' | 'en'

export type PlanRecommendationText = {
  reason: string
  summary: string
  proofPoints: string[]
  scopeSignals: string[]
  opportunities: string[]
}

export type PlanRecommendation = {
  planId: 'trial' | 'hobby' | 'standard' | 'pro' | 'enterprise'
  reason: string
  summary: string
  proofPoints: string[]
  scopeSignals: string[]
  opportunities: string[]
  generatedAt: string
  source: 'local' | 'model'
  connectedSourceCount?: number
  contextHash?: string
  locale?: PlanRecommendationLocale
  sourceCount?: number
  translations?: Partial<Record<PlanRecommendationLocale, PlanRecommendationText>>
}
