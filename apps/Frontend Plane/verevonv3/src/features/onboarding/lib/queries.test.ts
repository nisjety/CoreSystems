import { describe, expect, it, vi } from 'vitest'
import type { OnboardingGatewayActions } from '@/features/onboarding/lib/actions'
import {
  graphPreviewQueryConfig,
  onboardingQueryKeys,
  planRecommendationQueryConfig,
} from '@/features/onboarding/lib/queries'

describe('onboarding query helpers', () => {
  it('keeps recommendation failures from retry-looping', () => {
    const actions = {
      recommendPlan: vi.fn(),
    } as unknown as OnboardingGatewayActions

    const config = planRecommendationQueryConfig(actions, () => ({ sourceCount: 1 }), () => true, () => 'ctx-1')

    expect(config.retry).toBe(false)
    expect(config.staleTime).toBe(Infinity)
    expect(config.queryKey).toEqual(onboardingQueryKeys.planRecommendation('ctx-1'))
  })

  it('polls graph preview only while enabled', () => {
    const actions = {
      fetchGraphPreview: vi.fn(),
    } as unknown as OnboardingGatewayActions

    expect(graphPreviewQueryConfig(actions, () => 'org-1', () => true).refetchInterval).toBe(2500)
    expect(graphPreviewQueryConfig(actions, () => 'org-1', () => false).refetchInterval).toBe(false)
  })
})
