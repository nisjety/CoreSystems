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
  // The gateway (→ org-core Enhetsregisteret) returns `{ count, results }`, and
  // requestJson only unwraps a top-level `data` key — so we must read `.results`
  // ourselves, otherwise the caller gets an object and the result list renders
  // empty.
  const response = await requestJson<{ count?: number; results?: BrregEnhet[] }>(
    `/api/v1/onboarding/brreg/search?q=${encodeURIComponent(q)}&size=8`,
  )
  return Array.isArray(response.results) ? response.results : []
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
