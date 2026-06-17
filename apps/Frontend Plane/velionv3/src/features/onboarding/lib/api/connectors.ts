import { requestJson } from '@/shared/api/http'
import { withDevActor } from '@/features/onboarding/lib/api/actor'
import type { ActionActor } from '@/features/onboarding/lib/api/contracts'

export async function startConnectSession(input: {
  actor: ActionActor
  orgId: string
  provider: string
  selectedSources: string[]
}): Promise<{ connectUrl: string; authMode?: string; sessionToken?: string }> {
  return requestJson('/api/v1/onboarding/actions/start-connect-session', {
    method: 'POST',
    body: JSON.stringify(withDevActor({
      orgId: input.orgId,
      provider: input.provider,
      selectedSources: input.selectedSources,
      bundles: ['onboarding'],
    }, input.actor)),
  })
}

export async function discoverSource(input: {
  actor: ActionActor
  orgId: string
  connectorId: string
  label: string
  provider: string
  sources: string[]
}): Promise<{ id?: string; discovered?: boolean }> {
  return requestJson('/api/v1/onboarding/actions/discover-source', {
    method: 'POST',
    body: JSON.stringify(withDevActor({
      orgId: input.orgId,
      connectorId: input.connectorId,
      label: input.label,
      provider: input.provider,
      sources: input.sources,
    }, input.actor)),
  })
}

export async function cleanupSource(input: {
  actor: ActionActor
  orgId: string
  sourceId: string
}): Promise<{ cleaned?: boolean }> {
  return requestJson('/api/v1/onboarding/actions/cleanup-source', {
    method: 'POST',
    body: JSON.stringify(withDevActor({
      orgId: input.orgId,
      sourceId: input.sourceId,
    }, input.actor)),
  })
}

export async function warmSharePointDiscovery(input: {
  actor: ActionActor
  orgId: string
}): Promise<{ warmed?: boolean }> {
  return requestJson('/api/v1/onboarding/actions/warm-sharepoint-discovery', {
    method: 'POST',
    body: JSON.stringify(withDevActor({ orgId: input.orgId }, input.actor)),
  })
}

export async function startIntegrationSync(input: {
  actor: ActionActor
  orgId: string
  connectorId: string
  provider: string
  sources: string[]
}): Promise<{ id?: string; started?: boolean }> {
  return requestJson('/api/v1/onboarding/actions/start-integration-sync', {
    method: 'POST',
    body: JSON.stringify(withDevActor({
      orgId: input.orgId,
      connectorId: input.connectorId,
      provider: input.provider,
      sources: input.sources,
    }, input.actor)),
  })
}
