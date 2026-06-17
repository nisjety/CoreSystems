export type PlanRecommendation = {
  planId: 'trial' | 'hobby' | 'standard' | 'pro' | 'enterprise'
  reason: string
  summary: string
  proofPoints: string[]
  scopeSignals: string[]
  opportunities: string[]
  generatedAt: string
  source: 'local' | 'model'
}
