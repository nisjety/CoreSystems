import { requestJson } from '@/shared/api/http'
import {
  createEmptyPreviewResponse,
  type BrregEnhet,
  type PlanRecommendation,
  type PreviewResponse,
} from '@/features/onboarding/lib/api/contracts'

export async function fetchSessionBootstrap(): Promise<Record<string, unknown>> {
  return requestJson('/api/v1/session/bootstrap')
}

export async function fetchOnboardingStatus(): Promise<Record<string, unknown>> {
  return requestJson('/api/v1/onboarding/status')
}

export async function searchBrreg(q: string): Promise<BrregEnhet[]> {
  return requestJson(`/api/v1/onboarding/brreg/search?q=${encodeURIComponent(q)}&size=8`)
}

export async function fetchGraphPreview(orgId: string | undefined): Promise<PreviewResponse> {
  const currentOrgId = orgId?.trim()
  if (!currentOrgId) {
    return createEmptyPreviewResponse()
  }

  return requestJson(`/api/v1/onboarding/graph-preview?orgId=${encodeURIComponent(currentOrgId)}`)
}

export async function recommendPlan(context: Record<string, unknown>): Promise<PlanRecommendation> {
  const response = await requestJson<{ recommendation: PlanRecommendation }>('/api/v1/onboarding/recommend-plan', {
    method: 'POST',
    body: JSON.stringify({ context }),
  })
  return response.recommendation
}
