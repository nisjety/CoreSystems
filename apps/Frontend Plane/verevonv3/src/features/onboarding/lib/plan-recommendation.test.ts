import { describe, expect, it } from 'vitest'
import type { PlanRecommendation } from '@/features/onboarding/lib/api'
import {
  localizeRecommendation,
  planRecommendationContextHash,
  withRecommendationTranslation,
} from '@/features/onboarding/lib/plan-recommendation'

describe('plan recommendation localization', () => {
  it('does not include locale in the plan decision hash', () => {
    const base = {
      locale: 'nb',
      organization: { name: 'AQUATIQ AS', employeeCount: 93 },
      connectedSourceCount: 9,
      sourceCount: 10,
    }

    expect(planRecommendationContextHash(base)).toBe(
      planRecommendationContextHash({
        ...base,
        locale: 'en',
      }),
    )
  })

  it('uses cached translated text without changing the chosen plan', () => {
    const recommendation: PlanRecommendation = {
      planId: 'standard',
      reason: 'Behov for flere kilder og automasjon peker mot Advanced.',
      summary: '10 kilder og 93 ansatte gir best start med Advanced.',
      proofPoints: ['9 tilkoblede kilder valgt.'],
      scopeSignals: ['coresystem.com'],
      opportunities: ['Automatiser første sortering'],
      generatedAt: '2026-07-05T00:00:00Z',
      source: 'model',
      locale: 'nb',
    }

    const withEnglish = withRecommendationTranslation(recommendation, 'en', {
      reason: 'Multiple sources and automation needs point to Advanced.',
      summary: '10 sources and 93 employees make Advanced the best starting point.',
      proofPoints: ['9 connected sources selected.'],
      scopeSignals: ['coresystem.com'],
      opportunities: ['Automate first triage'],
    })

    const localized = localizeRecommendation(withEnglish, 'en')

    expect(localized?.planId).toBe('standard')
    expect(localized?.summary).toBe('10 sources and 93 employees make Advanced the best starting point.')
    expect(recommendation.summary).toBe('10 kilder og 93 ansatte gir best start med Advanced.')
  })
})
