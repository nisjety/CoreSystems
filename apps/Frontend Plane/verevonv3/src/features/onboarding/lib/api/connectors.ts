import { requestJson } from '@/shared/api/http'
import { withDevActor } from '@/features/onboarding/lib/api/actor'
import { connectBundlesForSources } from '@/shared/integrations/connect-bundles'
import type { ActionActor } from '@/features/onboarding/lib/api/contracts'
import {
  cleanupSourceSchema,
  connectSessionSchema,
  discoverSourceSchema,
  integrationSyncSchema,
  parseOnboardingResponse,
  sharePointWarmupSchema,
} from './response-schemas'

export async function startConnectSession(input: {
  actor: ActionActor
  orgId: string
  provider: string
  selectedSources: string[]
}): Promise<{ connectUrl: string; authMode?: string; sessionToken?: string }> {
  const endpoint = '/api/v1/onboarding/actions/start-connect-session'
  return parseOnboardingResponse(connectSessionSchema, await requestJson<unknown>(endpoint, {
    method: 'POST',
    body: JSON.stringify(withDevActor({
      orgId: input.orgId,
      provider: input.provider,
      selectedSources: input.selectedSources,
      bundles: connectBundlesForSources(input.provider, input.selectedSources),
    }, input.actor)),
  }), endpoint)
}

export async function discoverSource(input: {
  actor: ActionActor
  orgId: string
  connectorId: string
  label: string
  provider: string
  sources: string[]
}): Promise<{ id?: string; discovered?: boolean }> {
  const endpoint = '/api/v1/onboarding/actions/discover-source'
  return parseOnboardingResponse(discoverSourceSchema, await requestJson<unknown>(endpoint, {
    method: 'POST',
    body: JSON.stringify(withDevActor({
      orgId: input.orgId,
      connectorId: input.connectorId,
      label: input.label,
      provider: input.provider,
      sources: input.sources,
    }, input.actor)),
  }), endpoint)
}

export async function cleanupSource(input: {
  actor: ActionActor
  orgId: string
  sourceId: string
}): Promise<{ cleaned?: boolean }> {
  const endpoint = '/api/v1/onboarding/actions/cleanup-source'
  return parseOnboardingResponse(cleanupSourceSchema, await requestJson<unknown>(endpoint, {
    method: 'POST',
    body: JSON.stringify(withDevActor({
      orgId: input.orgId,
      sourceId: input.sourceId,
    }, input.actor)),
  }), endpoint)
}

export async function warmSharePointDiscovery(input: {
  actor: ActionActor
  orgId: string
}): Promise<{ warmed?: boolean }> {
  const endpoint = '/api/v1/onboarding/actions/warm-sharepoint-discovery'
  return parseOnboardingResponse(sharePointWarmupSchema, await requestJson<unknown>(endpoint, {
    method: 'POST',
    body: JSON.stringify(withDevActor({ orgId: input.orgId }, input.actor)),
  }), endpoint)
}

export async function startIntegrationSync(input: {
  actor: ActionActor
  orgId: string
  connectorId: string
  provider: string
  sources: string[]
}): Promise<{ id?: string; started?: boolean }> {
  const endpoint = '/api/v1/onboarding/actions/start-integration-sync'
  return parseOnboardingResponse(integrationSyncSchema, await requestJson<unknown>(endpoint, {
    method: 'POST',
    body: JSON.stringify(withDevActor({
      orgId: input.orgId,
      connectorId: input.connectorId,
      provider: input.provider,
      sources: input.sources,
    }, input.actor)),
  }), endpoint)
}
