import { requestJson } from '@/shared/api/http'
import {
  createEmptyPreviewResponse,
  type BrregEnhet,
  type PlanRecommendation,
  type PlanRecommendationLocale,
  type PlanRecommendationText,
  type PreviewResponse,
} from '@/features/onboarding/lib/api/contracts'
import {
  brregSearchResponseSchema,
  graphPreviewSchema,
  onboardingLifecycleSchema,
  parseOnboardingResponse,
  planRecommendationResponseSchema,
  planRecommendationTranslationResponseSchema,
  unknownRecordSchema,
} from './response-schemas'

export async function fetchSessionBootstrap(): Promise<Record<string, unknown>> {
  const endpoint = '/api/v1/session/bootstrap'
  return parseOnboardingResponse(unknownRecordSchema, await requestJson<unknown>(endpoint), endpoint)
}

export async function fetchOnboardingStatus(): Promise<Record<string, unknown>> {
  const endpoint = '/api/v1/onboarding/status'
  return parseOnboardingResponse(unknownRecordSchema, await requestJson<unknown>(endpoint), endpoint)
}

export async function fetchOnboardingLifecycle(): Promise<{ state: 'PROFILE_READY' | 'COMPLETED'; orgId: string }> {
  const endpoint = '/api/v1/onboarding/lifecycle'
  return parseOnboardingResponse(onboardingLifecycleSchema, await requestJson<unknown>(endpoint), endpoint)
}

export async function searchBrreg(q: string): Promise<BrregEnhet[]> {
  // The gateway (→ org-core Enhetsregisteret) returns `{ count, results }`, and
  // requestJson only unwraps a top-level `data` key — so we must read `.results`
  // ourselves, otherwise the caller gets an object and the result list renders
  // empty.
  const endpoint = `/api/v1/onboarding/brreg/search?q=${encodeURIComponent(q)}&size=8`
  const response = parseOnboardingResponse(brregSearchResponseSchema, await requestJson<unknown>(
    endpoint,
  ), endpoint)
  return response.results
}

export async function fetchGraphPreview(orgId: string | undefined): Promise<PreviewResponse> {
  const currentOrgId = orgId?.trim()
  if (!currentOrgId) {
    return createEmptyPreviewResponse()
  }

  const endpoint = `/api/v1/onboarding/graph-preview?orgId=${encodeURIComponent(currentOrgId)}`
  return parseOnboardingResponse(graphPreviewSchema, await requestJson<unknown>(endpoint), endpoint)
}

export async function recommendPlan(context: Record<string, unknown>): Promise<PlanRecommendation> {
  const endpoint = '/api/v1/onboarding/recommend-plan'
  const response = parseOnboardingResponse(planRecommendationResponseSchema, await requestJson<unknown>(endpoint, {
    method: 'POST',
    body: JSON.stringify({ context }),
  }), endpoint)
  return response.recommendation
}

export async function translatePlanRecommendation(input: {
  recommendation: PlanRecommendationText
  sourceLanguage?: PlanRecommendationLocale
  targetLanguage: PlanRecommendationLocale
}): Promise<PlanRecommendationText> {
  const endpoint = '/api/v1/onboarding/translate-recommendation'
  const response = parseOnboardingResponse(planRecommendationTranslationResponseSchema, await requestJson<unknown>(
    endpoint,
    {
      method: 'POST',
      body: JSON.stringify(input),
    },
  ), endpoint)
  return response.translation
}
