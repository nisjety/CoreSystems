import { createQuery } from '@tanstack/solid-query'
import type { Accessor } from 'solid-js'
import type { OnboardingGatewayActions } from '@/features/onboarding/lib/actions'

export const onboardingQueryKeys = {
  graphPreview: (orgId?: string) => ['onboarding', 'graph-preview', orgId ?? 'none'] as const,
  planRecommendation: (contextHash: string) => ['onboarding', 'recommend-plan', contextHash] as const,
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
  contextHash: Accessor<string> = () => JSON.stringify(context()),
) {
  return {
    enabled: enabled(),
    queryFn: () => actions.recommendPlan(context()),
    queryKey: onboardingQueryKeys.planRecommendation(contextHash()),
    retry: false,
    staleTime: Infinity,
  }
}

export function createPlanRecommendationQuery(
  actions: OnboardingGatewayActions,
  context: Accessor<Record<string, unknown>>,
  enabled: Accessor<boolean>,
  contextHash?: Accessor<string>,
) {
  return createQuery(() => planRecommendationQueryConfig(actions, context, enabled, contextHash))
}
