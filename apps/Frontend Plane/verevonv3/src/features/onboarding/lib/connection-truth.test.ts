import { describe, expect, it } from 'vitest'
import type { IntegrationConnection } from '@/shared/api/integrations-client'
import {
  activeConnectionForProvider,
  hasPendingSyncLanes,
  reconcileConnectorsWithConnections,
  summarizeConnectedAccounts,
} from '@/features/onboarding/lib/connection-truth'

const tr = (no: string) => no

function first<T>(items: readonly T[]): T {
  const item = items[0]
  if (item === undefined) throw new Error('expected at least one item')
  return item
}

function microsoftConnection(overrides: Partial<IntegrationConnection> = {}): IntegrationConnection {
  return {
    id: 'conn_f019f69a',
    providerId: 'microsoft',
    providerKey: 'microsoft',
    connectorType: 'microsoft-graph',
    providerEmail: 'ima.dacosta@aquatiq.com',
    displayName: 'Ima Fernandes Da Costa',
    status: 'active',
    createdAt: '2026-09-04T01:00:00.000Z',
    capabilities: ['profile.read', 'sharepoint.read', 'teams.read', 'teams.messages.read', 'mail.read', 'mail.send'],
    scopes: ['Files.Read.All', 'Sites.Read.All', 'Mail.Read', 'ChannelMessage.Read.All'],
    ...overrides,
  }
}

describe('summarizeConnectedAccounts', () => {
  it('shows provider, account, granted capabilities and per-lane health from integration-core', () => {
    const account = first(summarizeConnectedAccounts([microsoftConnection({
      syncLanes: {
        mail: { status: 'synced', source: 'email-worker', lastSyncAt: '2026-09-04T02:00:00.000Z' },
        collaboration: { status: 'pending', source: 'email-worker' },
        documents: { status: 'running', source: 'finspo-core', jobId: 'sync_1' },
      },
    })], { tr }))

    expect(account.providerLabel).toBe('Microsoft 365')
    expect(account.connectorId).toBe('microsoft365')
    expect(account.account).toBe('ima.dacosta@aquatiq.com')
    expect(account.grants).toEqual(['Outlook', 'Teams', 'SharePoint', 'OneDrive'])
    expect(account.lanes.map((lane) => [lane.label, lane.status])).toEqual([
      ['Outlook', 'synced'],
      ['Teams', 'pending'],
      ['SharePoint · OneDrive', 'running'],
    ])
    expect(account.nextStep.kind).toBe('syncing')
    expect(account.nextStep.message).toContain('Teams · SharePoint · OneDrive')
  })

  // The core of the onboarding fix: a fresh Microsoft connection has no
  // SharePoint library registered, so the concrete next step is to pick one —
  // whether integration-core refused the sync (409) or a worker failed the
  // job with the same code.
  it('asks for a library when integration-core reports no_sources_registered', () => {
    const fromRefusal = summarizeConnectedAccounts([microsoftConnection({
      syncLanes: { mail: { status: 'synced' }, documents: { status: 'pending', source: 'finspo-core' } },
    })], { tr, librariesNeeded: new Set(['conn_f019f69a']) })
    expect(fromRefusal[0]?.nextStep.kind).toBe('pick_library')

    const fromFailedJob = summarizeConnectedAccounts([microsoftConnection({
      syncLanes: {
        mail: { status: 'synced' },
        documents: { status: 'failed', source: 'finspo-core', failureCode: 'no_sources_registered', lastError: 'no SharePoint or OneDrive library is registered' },
      },
    })], { tr })
    expect(fromFailedJob[0]?.nextStep.kind).toBe('pick_library')
  })

  it('reports the failing lane by name and keeps the other lanes intact', () => {
    const account = first(summarizeConnectedAccounts([microsoftConnection({
      syncLanes: {
        mail: { status: 'failed', source: 'email-worker', lastError: 'ingest bridge returned unexpected status 401' },
        documents: { status: 'synced', source: 'finspo-core', lastSyncAt: '2026-09-04T02:00:00.000Z' },
      },
    })], { tr }))
    expect(account.nextStep.kind).toBe('attention')
    expect(account.nextStep.message).toContain('Outlook')
    expect(account.nextStep.message).toContain('401')
    expect(account.lanes.find((lane) => lane.key === 'documents')?.status).toBe('synced')
  })

  it('is ready when every lane synced, and asks to reconnect on needs_refresh', () => {
    const ready = summarizeConnectedAccounts([microsoftConnection({
      syncLanes: { mail: { status: 'synced' }, documents: { status: 'synced' } },
    })], { tr })
    expect(ready[0]?.nextStep.kind).toBe('ready')
    expect(ready[0]?.nextStep.message).toContain('Support')

    const refresh = summarizeConnectedAccounts([microsoftConnection({
      status: 'needs_refresh',
      syncLanes: { mail: { status: 'failed', lastError: 'resolve access token' } },
    })], { tr })
    expect(refresh[0]?.status).toBe('needs_refresh')
    expect(refresh[0]?.nextStep.kind).toBe('reconnect')
  })

  it('drops deleted or revoked connections and names a documents-only grant honestly', () => {
    const accounts = summarizeConnectedAccounts([
      microsoftConnection({ id: 'gone', deletedAt: '2026-09-04T03:00:00.000Z' }),
      microsoftConnection({ id: 'revoked', status: 'revoked' }),
      microsoftConnection({
        id: 'docs-only',
        capabilities: ['profile.read', 'sharepoint.read', 'teams.read'],
        scopes: ['Files.Read.All', 'Sites.Read.All'],
        syncLanes: { documents: { status: 'pending', source: 'finspo-core' } },
      }),
    ], { tr })
    expect(accounts.map((account) => account.connectionId)).toEqual(['docs-only'])
    expect(accounts[0]?.grants).toEqual(['SharePoint', 'OneDrive', 'Teams (metadata)'])
  })

  it('labels Google and generic providers from their grants', () => {
    const [google, slack] = summarizeConnectedAccounts([
      {
        id: 'conn-slack', providerId: 'slack', providerKey: 'slack', status: 'active', createdAt: '2026-09-01T00:00:00.000Z',
        displayName: 'Aquatiq workspace', capabilities: ['workspace.read', 'channels.history', 'messages.read'],
      },
      {
        id: 'conn-google', providerId: 'google', providerKey: 'google', status: 'active', createdAt: '2026-09-02T00:00:00.000Z',
        providerEmail: 'ops@aquatiq.com', capabilities: ['profile.read', 'gmail.read', 'drive.read', 'calendar.read'],
      },
    ], { tr })
    expect(google?.providerLabel).toBe('Google Workspace')
    expect(google?.grants).toEqual(['Gmail', 'Drive', 'Kalender'])
    expect(slack?.account).toBe('Aquatiq workspace')
    expect(slack?.grants).toEqual(['Kanaler', 'Direktemeldinger'])
  })
})

describe('reconcileConnectorsWithConnections', () => {
  it('adds a connected card for every active provider connection and upgrades pending ones', () => {
    const next = reconcileConnectorsWithConnections(
      [{ id: 'slack', label: 'Slack', status: 'pending', sources: ['messages'] }],
      [
        microsoftConnection(),
        { id: 'conn-slack', providerId: 'slack', providerKey: 'slack', status: 'active', createdAt: '2026-09-01T00:00:00.000Z' },
      ],
    )
    expect(next).toEqual([
      { id: 'slack', label: 'Slack', status: 'connected', sources: ['messages'] },
      { id: 'microsoft365', label: 'Microsoft 365', status: 'connected', sources: ['teams', 'outlook', 'sharepoint', 'onedrive'] },
    ])
  })

  it('returns the same array when the truth adds nothing', () => {
    const connectors = [{ id: 'microsoft365', label: 'Microsoft 365', status: 'connected' as const, sources: ['teams'] }]
    expect(reconcileConnectorsWithConnections(connectors, [microsoftConnection()])).toBe(connectors)
    expect(reconcileConnectorsWithConnections(connectors, [])).toBe(connectors)
  })
})

describe('sync polling helpers', () => {
  it('keeps polling while any lane is pending or running', () => {
    expect(hasPendingSyncLanes([microsoftConnection({ syncLanes: { mail: { status: 'synced' }, documents: { status: 'running' } } })])).toBe(true)
    expect(hasPendingSyncLanes([microsoftConnection({ syncLanes: { mail: { status: 'synced' }, documents: { status: 'failed' } } })])).toBe(false)
    expect(hasPendingSyncLanes([microsoftConnection({ status: 'revoked', syncLanes: { mail: { status: 'pending' } } })])).toBe(false)
  })

  it('picks the newest active connection for a provider', () => {
    const older = microsoftConnection({ id: 'older', createdAt: '2026-09-01T00:00:00.000Z' })
    const newer = microsoftConnection({ id: 'newer', createdAt: '2026-09-03T00:00:00.000Z' })
    expect(activeConnectionForProvider([older, newer], 'microsoft')?.id).toBe('newer')
    expect(activeConnectionForProvider([older], 'google')).toBeUndefined()
  })
})
