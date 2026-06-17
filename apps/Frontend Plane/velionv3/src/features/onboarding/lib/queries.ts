import { createQuery } from '@tanstack/solid-query'
import type { Accessor } from 'solid-js'
import type { OnboardingGatewayActions } from '@/features/onboarding/lib/actions'

export const onboardingQueryKeys = {
  graphPreview: (orgId?: string) => ['onboarding', 'graph-preview', orgId ?? 'none'] as const,
  planRecommendation: (context: Record<string, unknown>) => ['onboarding', 'recommend-plan', context] as const,
}

export function graphPreviewQueryConfig(
  actions: OnboardingGatewayActions,
  orgId: Accessor<string | undefined>,
  enabled: Accessor<boolean>,
) {
  const refetchInterval: 2500 | false = enabled() ? 2500 : false

  return {
    enabled: enabled(),
    queryFn: () => actions.fetchGraphPreview(orgId()),
    queryKey: onboardingQueryKeys.graphPreview(orgId()),
    refetchInterval,
  }
}

export function createGraphPreviewQuery(
  actions: OnboardingGatewayActions,
  orgId: Accessor<string | undefined>,
  enabled: Accessor<boolean>,
) {
  return createQuery(() => graphPreviewQueryConfig(actions, orgId, enabled))
}

export function planRecommendationQueryConfig(
  actions: OnboardingGatewayActions,
  context: Accessor<Record<string, unknown>>,
  enabled: Accessor<boolean>,
) {
  return {
    enabled: enabled(),
    queryFn: () => actions.recommendPlan(context()),
    queryKey: onboardingQueryKeys.planRecommendation(context()),
    retry: false,
    staleTime: Infinity,
  }
}

export function createPlanRecommendationQuery(
  actions: OnboardingGatewayActions,
  context: Accessor<Record<string, unknown>>,
  enabled: Accessor<boolean>,
) {
  return createQuery(() => planRecommendationQueryConfig(actions, context, enabled))
}
