import { requestJson } from '@/shared/api/http'
import { withDevActor } from '@/features/onboarding/lib/api/actor'
import type {
  ActionActor,
  BrregEnhet,
  ThemeMode,
} from '@/features/onboarding/lib/api/contracts'

export async function createOrganization(input: {
  actor: ActionActor
  name: string
  plan?: string
  orgNumber?: string
  brregData?: BrregEnhet | null
  metadata?: Record<string, unknown>
}): Promise<{ id: string; name: string; slug?: string; plan?: string }> {
  return requestJson('/api/v1/onboarding/actions/create-organization', {
    method: 'POST',
    body: JSON.stringify(withDevActor({
      name: input.name,
      plan: input.plan,
      orgNumber: input.orgNumber,
      brregData: input.brregData,
      metadata: input.metadata,
    }, input.actor)),
  })
}

export async function startWebsiteIngest(input: {
  actor: ActionActor
  orgId: string
  url: string
  brief?: string
}): Promise<{ id?: string }> {
  return requestJson('/api/v1/onboarding/actions/start-website-ingest', {
    method: 'POST',
    body: JSON.stringify(withDevActor({
      orgId: input.orgId,
      url: input.url,
      brief: input.brief,
    }, input.actor)),
  })
}

export async function setBrandTheme(input: {
  actor: ActionActor
  mode: ThemeMode
  primaryColor: string
}): Promise<{ persisted: boolean; mode: ThemeMode; primaryColor: string }> {
  return requestJson('/api/v1/onboarding/theme', {
    method: 'PUT',
    body: JSON.stringify(withDevActor({
      mode: input.mode,
      primaryColor: input.primaryColor,
    }, input.actor)),
  })
}

export async function completeOnboarding(input: {
  actor: ActionActor
  orgId?: string
  plan?: string
  source?: string
  metadata?: Record<string, unknown>
}): Promise<{ completed: boolean }> {
  return requestJson('/api/v1/onboarding/complete', {
    method: 'POST',
    body: JSON.stringify(withDevActor({
      orgId: input.orgId,
      plan: input.plan,
      source: input.source,
      metadata: input.metadata,
    }, input.actor)),
  })
}
