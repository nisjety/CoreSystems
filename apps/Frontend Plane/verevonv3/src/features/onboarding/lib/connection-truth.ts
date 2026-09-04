import type { IntegrationConnection, SyncLane, SyncLaneKey } from '@/shared/api/integrations-client'
import { type OnboardingState, onboardingConnectorOptions } from '@/features/onboarding/lib/model'

/**
 * Onboarding's `state.connectors[]` is a UI-only record of what the user
 * clicked. The truth about what is connected lives in integration-core
 * (`GET /api/v1/integrations/connections`, with per-pipeline `syncLanes`).
 * This module turns that truth into what the connect step shows — provider,
 * account, granted capabilities, per-lane sync health and the one concrete
 * next step — and reconciles the UI record with it.
 */

type Translate = (no: string, en: string) => string

export type ConnectionLaneStatus = 'pending' | 'running' | 'synced' | 'failed' | 'cancelled' | 'unknown'

export type ConnectionLaneView = {
  key: SyncLaneKey
  label: string
  status: ConnectionLaneStatus
  detail?: string
  failureCode?: string
  lastSyncAt?: string
}

export type ConnectionNextStepKind = 'pick_library' | 'reconnect' | 'attention' | 'syncing' | 'ready'

export type ConnectionNextStep = {
  kind: ConnectionNextStepKind
  message: string
}

export type ConnectedAccountView = {
  connectionId: string
  /** Matching onboarding connector card, when there is one. */
  connectorId?: string
  provider: string
  providerLabel: string
  account: string
  status: 'active' | 'needs_refresh' | 'inactive'
  /** Human labels for what the connection was granted ("Outlook", "Teams", …). */
  grants: string[]
  lanes: ConnectionLaneView[]
  nextStep: ConnectionNextStep
}

export const NO_SOURCES_REGISTERED = 'no_sources_registered'

const activeStatuses = new Set(['active', 'connected', 'needs_refresh'])

export function normalizeProviderKey(value: string | undefined): string {
  switch ((value ?? '').trim().toLowerCase().replaceAll('_', '-')) {
    case 'microsoft-365':
    case 'microsoft365':
    case 'microsoft-graph':
    case 'm365':
    case 'outlook':
    case 'teams':
    case 'sharepoint':
    case 'onedrive':
      return 'microsoft'
    case 'google-workspace':
    case 'gmail':
    case 'google-drive':
      return 'google'
    case 'twitter':
      return 'x'
    case 'meta-unified':
    case 'facebook':
      return 'meta'
    default:
      return (value ?? '').trim().toLowerCase()
  }
}

function connectionProvider(connection: IntegrationConnection): string {
  return normalizeProviderKey(connection.providerKey || connection.providerId || connection.connectorType)
}

function isActive(connection: IntegrationConnection): boolean {
  return !connection.deletedAt && activeStatuses.has(connection.status.trim().toLowerCase())
}

/** The newest active connection for a provider, or undefined. */
export function activeConnectionForProvider(connections: readonly IntegrationConnection[], provider: string): IntegrationConnection | undefined {
  const key = normalizeProviderKey(provider)
  return [...connections]
    .filter((connection) => isActive(connection) && connectionProvider(connection) === key)
    .sort((left, right) => (right.createdAt ?? '').localeCompare(left.createdAt ?? ''))[0]
}

/** True while any lane on any active connection is still pending or running,
 * i.e. the connect step should keep refreshing the truth. */
export function hasPendingSyncLanes(connections: readonly IntegrationConnection[]): boolean {
  return connections.some((connection) => isActive(connection)
    && Object.values(connection.syncLanes ?? {}).some((lane) => lane && ['pending', 'running'].includes(lane.status)))
}

type GrantRule = { capabilities: string[]; scopes?: string[]; label: string }

const grantRules: Record<string, GrantRule[]> = {
  microsoft: [
    { capabilities: ['mail.read'], scopes: ['mail.read'], label: 'Outlook' },
    { capabilities: ['teams.messages.read'], scopes: ['channelmessage.read.all', 'chat.read'], label: 'Teams' },
    { capabilities: ['sharepoint.read'], scopes: ['files.read.all', 'sites.read.all'], label: 'SharePoint' },
    { capabilities: ['sharepoint.read'], scopes: ['files.read.all'], label: 'OneDrive' },
    { capabilities: ['calendar.read'], scopes: ['calendars.read'], label: 'Kalender' },
  ],
  google: [
    { capabilities: ['gmail.read'], scopes: ['https://www.googleapis.com/auth/gmail.readonly'], label: 'Gmail' },
    { capabilities: ['drive.read', 'drive.metadata'], scopes: ['https://www.googleapis.com/auth/drive.readonly'], label: 'Drive' },
    { capabilities: ['calendar.read'], scopes: ['https://www.googleapis.com/auth/calendar.readonly'], label: 'Kalender' },
  ],
  slack: [
    { capabilities: ['channels.history'], scopes: ['channels:history'], label: 'Kanaler' },
    { capabilities: ['messages.read'], scopes: ['im:history'], label: 'Direktemeldinger' },
    { capabilities: ['files.read'], scopes: ['files:read'], label: 'Filer' },
  ],
}

function grantedSet(connection: IntegrationConnection): Set<string> {
  return new Set([...(connection.capabilities ?? []), ...(connection.scopes ?? [])].map((value) => value.trim().toLowerCase()))
}

function grantLabels(connection: IntegrationConnection, provider: string): string[] {
  const granted = grantedSet(connection)
  const rules = grantRules[provider]
  if (rules) {
    const labels: string[] = []
    for (const rule of rules) {
      const matched = rule.capabilities.some((capability) => granted.has(capability))
        || (rule.scopes ?? []).some((scope) => granted.has(scope))
      if (matched && !labels.includes(rule.label)) labels.push(rule.label)
    }
    // Teams metadata alone (no message read) is still a real grant worth naming.
    if (provider === 'microsoft' && !labels.includes('Teams') && granted.has('teams.read')) labels.push('Teams (metadata)')
    return labels
  }
  return (connection.capabilities ?? [])
    .filter((capability) => !/^(profile|account|workspace|store)\./.test(capability))
    .map(prettifyCapability)
    .filter((label, index, all) => all.indexOf(label) === index)
}

function prettifyCapability(capability: string): string {
  const parts = capability.split('.').filter((part) => !['read', 'manage', 'write', 'social'].includes(part))
  const word = parts.join(' ').replaceAll('_', ' ').trim() || capability
  return word.charAt(0).toUpperCase() + word.slice(1)
}

function laneLabel(key: SyncLaneKey, provider: string, tr: Translate): string {
  switch (key) {
    case 'mail':
      return provider === 'microsoft' ? 'Outlook' : provider === 'google' ? 'Gmail' : tr('E-post', 'Email')
    case 'collaboration':
      if (provider === 'microsoft') return 'Teams'
      if (provider === 'slack') return 'Slack'
      if (provider === 'discord') return 'Discord'
      if (provider === 'x') return tr('X-meldinger', 'X messages')
      return tr('Samtaler', 'Conversations')
    case 'documents':
      if (provider === 'microsoft') return 'SharePoint · OneDrive'
      if (provider === 'google') return 'Drive'
      return tr('Dokumenter', 'Documents')
  }
}

function laneStatus(lane: SyncLane): ConnectionLaneStatus {
  const status = lane.status.trim().toLowerCase()
  return (['pending', 'running', 'synced', 'failed', 'cancelled'] as const).find((known) => known === status) ?? 'unknown'
}

const laneOrder: SyncLaneKey[] = ['mail', 'collaboration', 'documents']

function laneViews(connection: IntegrationConnection, provider: string, tr: Translate): ConnectionLaneView[] {
  const lanes = connection.syncLanes ?? {}
  return laneOrder.flatMap((key): ConnectionLaneView[] => {
    const lane = lanes[key]
    if (!lane) return []
    return [{
      key,
      label: laneLabel(key, provider, tr),
      status: laneStatus(lane),
      detail: lane.lastError,
      failureCode: lane.failureCode,
      lastSyncAt: lane.lastSyncAt,
    }]
  })
}

function providerLabel(provider: string): string {
  const option = onboardingConnectorOptions.find((candidate) => candidate.provider === provider)
  if (option) return option.label
  return provider ? provider.charAt(0).toUpperCase() + provider.slice(1) : ''
}

function accountLabel(connection: IntegrationConnection, providerLabelText: string): string {
  const metadata = connection.metadata ?? {}
  const candidates = [
    connection.providerEmail,
    typeof metadata.mailbox_address === 'string' ? metadata.mailbox_address : undefined,
    connection.userEmail,
    connection.displayName,
    connection.providerName,
  ]
  return candidates.find((value) => value && value.trim())?.trim() ?? providerLabelText
}

function nextStepFor(
  status: ConnectedAccountView['status'],
  provider: string,
  lanes: ConnectionLaneView[],
  needsLibrary: boolean,
  tr: Translate,
): ConnectionNextStep {
  if (status === 'needs_refresh') {
    return { kind: 'reconnect', message: tr('Tilgangen må godkjennes på nytt før noe kan synkroniseres.', 'Access must be re-authorized before anything can sync.') }
  }
  if (needsLibrary) {
    return {
      kind: 'pick_library',
      message: tr(
        'Velg hvilket SharePoint- eller OneDrive-bibliotek Verevon skal lese. Første synkronisering starter når du har valgt.',
        'Pick which SharePoint or OneDrive library Verevon should read. The first sync starts once you have chosen.',
      ),
    }
  }
  const failed = lanes.find((lane) => lane.status === 'failed')
  if (failed) {
    const detail = failed.detail ? ` ${failed.detail}` : ''
    return { kind: 'attention', message: `${failed.label}: ${tr('siste synkronisering mislyktes.', 'the latest sync failed.')}${detail}` }
  }
  if (lanes.some((lane) => lane.status === 'running' || lane.status === 'pending')) {
    const inFlight = lanes.filter((lane) => lane.status === 'running' || lane.status === 'pending').map((lane) => lane.label).join(' · ')
    return { kind: 'syncing', message: tr(`Første synkronisering kjører: ${inFlight}. Du kan gå videre — dette fortsetter i bakgrunnen.`, `First sync is running: ${inFlight}. You can continue — this keeps going in the background.`) }
  }
  if (lanes.length > 0) {
    return {
      kind: 'ready',
      message: provider === 'microsoft' || provider === 'google'
        ? tr('Klar. Support, Innboks og Kunnskap bruker denne tilkoblingen.', 'Ready. Support, Inbox and Knowledge use this connection.')
        : tr('Klar. Tilkoblingen er i bruk.', 'Ready. The connection is in use.'),
    }
  }
  return { kind: 'ready', message: tr('Tilkoblet.', 'Connected.') }
}

export function summarizeConnectedAccounts(
  connections: readonly IntegrationConnection[],
  options: { librariesNeeded?: ReadonlySet<string>; tr: Translate },
): ConnectedAccountView[] {
  return connections
    .filter((connection) => !connection.deletedAt)
    .map((connection): ConnectedAccountView => {
      const provider = connectionProvider(connection)
      const label = providerLabel(provider)
      const status: ConnectedAccountView['status'] = connection.status.trim().toLowerCase() === 'needs_refresh'
        ? 'needs_refresh'
        : isActive(connection) ? 'active' : 'inactive'
      const lanes = laneViews(connection, provider, options.tr)
      const needsLibrary = provider === 'microsoft' && (
        (options.librariesNeeded?.has(connection.id) ?? false)
        || lanes.some((lane) => lane.key === 'documents' && lane.failureCode === NO_SOURCES_REGISTERED)
      )
      return {
        connectionId: connection.id,
        connectorId: onboardingConnectorOptions.find((candidate) => candidate.provider === provider)?.id,
        provider,
        providerLabel: label,
        account: accountLabel(connection, label),
        status,
        grants: grantLabels(connection, provider),
        lanes,
        nextStep: nextStepFor(status, provider, lanes, needsLibrary, options.tr),
      }
    })
    .filter((account) => account.status !== 'inactive')
    .sort((left, right) => left.providerLabel.localeCompare(right.providerLabel))
}

/**
 * Folds integration-core's truth into onboarding's UI record: every active
 * connection whose provider has a connector card becomes (or upgrades to) a
 * `connected` entry, so the graph and the plan recommendation see what the
 * server sees — also after a reload or a connection made from Settings.
 * Returns the same array when nothing changed.
 */
export function reconcileConnectorsWithConnections(
  connectors: OnboardingState['connectors'],
  connections: readonly IntegrationConnection[],
): OnboardingState['connectors'] {
  let changed = false
  const next = [...connectors]
  for (const option of onboardingConnectorOptions) {
    if (option.provider === 'shipping') continue
    const connection = activeConnectionForProvider(connections, option.provider)
    if (!connection) continue
    const index = next.findIndex((connector) => connector.id === option.id)
    const existing = next[index]
    if (!existing) {
      next.push({ id: option.id, label: option.label, status: 'connected', sources: option.sources })
      changed = true
    } else if (existing.status === 'pending') {
      next[index] = { ...existing, status: 'connected' }
      changed = true
    }
  }
  return changed ? next : connectors
}
