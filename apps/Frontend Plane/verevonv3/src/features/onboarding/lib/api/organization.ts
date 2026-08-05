import { requestJson } from '@/shared/api/http'
import { withDevActor } from '@/features/onboarding/lib/api/actor'
import type {
  ActionActor,
  BrregEnhet,
  ThemeMode,
} from '@/features/onboarding/lib/api/contracts'
import {
  onboardingCompletedSchema,
  organizationCreatedSchema,
  parseOnboardingResponse,
  themePersistedSchema,
  websiteIngestStartedSchema,
} from './response-schemas'

export async function createOrganization(input: {
  actor: ActionActor
  name: string
  plan?: string
  orgNumber?: string
  brregData?: BrregEnhet | null
  metadata?: Record<string, unknown>
}): Promise<{ id: string; name: string; slug?: string; plan?: string }> {
  const endpoint = '/api/v1/onboarding/actions/create-organization'
  return parseOnboardingResponse(organizationCreatedSchema, await requestJson<unknown>(endpoint, {
    method: 'POST',
    body: JSON.stringify(withDevActor({
      name: input.name,
      plan: input.plan,
      orgNumber: input.orgNumber,
      brregData: input.brregData,
      metadata: input.metadata,
    }, input.actor)),
  }), endpoint)
}

export async function startWebsiteIngest(input: {
  actor: ActionActor
  orgId: string
  url: string
  brief?: string
}): Promise<{ id?: string }> {
  const endpoint = '/api/v1/onboarding/actions/start-website-ingest'
  return parseOnboardingResponse(websiteIngestStartedSchema, await requestJson<unknown>(endpoint, {
    method: 'POST',
    body: JSON.stringify(withDevActor({
      orgId: input.orgId,
      url: input.url,
      brief: input.brief,
    }, input.actor)),
  }), endpoint)
}

export async function setBrandTheme(input: {
  actor: ActionActor
  mode: ThemeMode
  primaryColor: string
}): Promise<{ persisted: boolean; mode: ThemeMode; primaryColor: string }> {
  const endpoint = '/api/v1/onboarding/theme'
  return parseOnboardingResponse(themePersistedSchema, await requestJson<unknown>(endpoint, {
    method: 'PUT',
    body: JSON.stringify(withDevActor({
      mode: input.mode,
      primaryColor: input.primaryColor,
    }, input.actor)),
  }), endpoint)
}

export async function completeOnboarding(input: {
  actor: ActionActor
  orgId?: string
  plan?: string
  source?: string
  metadata?: Record<string, unknown>
}): Promise<{ completed: boolean }> {
  const endpoint = '/api/v1/onboarding/complete'
  return parseOnboardingResponse(onboardingCompletedSchema, await requestJson<unknown>(endpoint, {
    method: 'POST',
    body: JSON.stringify(withDevActor({
      orgId: input.orgId,
      plan: input.plan,
      source: input.source,
      metadata: input.metadata,
    }, input.actor)),
  }), endpoint)
}
