import type { IntegrationConnection } from '@/shared/api/integrations-client'

export type InboxSourceChannel =
  | 'messenger'
  | 'instagram'
  | 'whatsapp'
  | 'email'
  | 'slack'
  | 'teams'
  | 'discord'
  | 'linkedin'
  | 'x'

export interface ConnectedInboxSource {
  id: InboxSourceChannel
  accountLabels: string[]
  channel: InboxSourceChannel
  connectionIds: string[]
  href: string
  label: string
  providerKeys: string[]
}

/** A real mailbox lane, derived strictly from an active provider connection
 * that granted an email-read capability. A displayed shared mailbox is only a
 * provider-declared configuration; it never claims that Verevon has imported or
 * can read it until the provider connection says so. */
export interface ConnectedEmailAccount {
  id: string
  providerKey: 'google' | 'microsoft'
  label: string
  sharedMailboxes: string[]
  /** Source health is limited to the most recent provider inbox-sync attempt.
   * It is never a claim that a customer received, read, or replied to mail. */
  syncHealth: EmailAccountSyncHealth
  lastSyncAt?: string
}

export type EmailAccountSyncHealth = 'synced' | 'syncing' | 'needs_reconnect' | 'attention' | 'unknown'

type InboxConnection = Partial<IntegrationConnection> & {
  id: string
  userEmail?: string
  providerKey?: string
  providerId?: string
  status: string
  deletedAt?: string
}

type SourceDefinition = {
  channel: InboxSourceChannel
  label: string
  grants?: string[]
}

const sourceDefinitions: Record<string, SourceDefinition[]> = {
  meta: [
    { channel: 'messenger', label: 'Messenger', grants: ['social.messenger.manage', 'pages_messaging'] },
    { channel: 'instagram', label: 'Instagram', grants: ['social.inbox.read', 'instagram_manage_messages'] },
    { channel: 'whatsapp', label: 'WhatsApp', grants: ['social.whatsapp.manage', 'whatsapp_business_messaging'] },
  ],
  microsoft: [
    { channel: 'email', label: 'Outlook', grants: ['mail.read', 'mail.readbasic'] },
    { channel: 'teams', label: 'Microsoft Teams', grants: ['teams.messages.read', 'channelmessage.read.all', 'chat.read', 'chat.read.all'] },
  ],
  google: [
    { channel: 'email', label: 'Gmail', grants: ['gmail.read', 'https://www.googleapis.com/auth/gmail.readonly'] },
  ],
  slack: [
    { channel: 'slack', label: 'Slack', grants: ['channels.history', 'channels:history', 'groups:history', 'im:history', 'mpim:history'] },
  ],
  discord: [
    { channel: 'discord', label: 'Discord', grants: ['messages.read', 'bot'] },
  ],
  x: [
    { channel: 'x', label: 'Twitter / X', grants: ['social.inbox.read', 'dm.read'] },
  ],
  linkedin: [
    { channel: 'linkedin', label: 'LinkedIn', grants: ['social.inbox.read'] },
  ],
  messenger: [{ channel: 'messenger', label: 'Messenger', grants: ['social.inbox.read', 'pages_messaging'] }],
  instagram: [{ channel: 'instagram', label: 'Instagram', grants: ['social.inbox.read', 'instagram_manage_messages'] }],
  whatsapp: [{ channel: 'whatsapp', label: 'WhatsApp', grants: ['social.inbox.read', 'whatsapp_business_messaging'] }],
}

const sourceOrder: InboxSourceChannel[] = [
  'messenger',
  'instagram',
  'whatsapp',
  'email',
  'slack',
  'teams',
  'discord',
  'linkedin',
  'x',
]

const emailLabelOrder = ['Outlook', 'Gmail']
const activeStatuses = new Set(['active', 'connected', 'needs_refresh'])

export function deriveConnectedInboxSources(connections: InboxConnection[]): ConnectedInboxSource[] {
  const sources = new Map<InboxSourceChannel, {
    accountLabels: Set<string>
    connectionIds: Set<string>
    labels: Set<string>
    providerKeys: Set<string>
  }>()

  for (const connection of connections) {
    if (connection.deletedAt || !activeStatuses.has(connection.status.trim().toLowerCase())) continue
    const providerKey = normalizeProviderKey(firstNonEmpty(
      connection.providerKey,
      connection.providerId,
      connection.connectorType,
    ))
    const granted = new Set([...(connection.capabilities ?? []), ...(connection.scopes ?? [])]
      .map((value) => value.trim().toLowerCase()))

    for (const definition of sourceDefinitions[providerKey] ?? []) {
      if (!definition.grants?.some((grant) => granted.has(grant))) continue
      // A successful Meta OAuth callback proves consent, not that Meta returned
      // a Page, Instagram professional account, or WhatsApp Business account.
      // Only webhook-org persists these identifiers after it has discovered and
      // subscribed a provider asset. Do not advertise an empty social lane as
      // connected before that proof exists.
      if (isMetaSocialChannel(definition.channel) && requiresTypedMetaAssetProof(providerKey, definition.channel) && !hasProvisionedMetaWebhookAssets(connection.metadata, definition.channel)) continue
      const source = sources.get(definition.channel) ?? {
        accountLabels: new Set<string>(),
        connectionIds: new Set<string>(),
        labels: new Set<string>(),
        providerKeys: new Set<string>(),
      }
      source.accountLabels.add(connection.displayName ?? connection.providerName ?? providerKey)
      source.connectionIds.add(connection.id)
      source.labels.add(definition.label)
      source.providerKeys.add(providerKey)
      sources.set(definition.channel, source)
    }
  }

  return [...sources.entries()]
    .map(([channel, source]): ConnectedInboxSource => ({
      id: channel,
      accountLabels: [...source.accountLabels],
      channel,
      connectionIds: [...source.connectionIds],
      href: `/inbox?view=mine&channel=${channel}`,
      label: formatSourceLabel(channel, [...source.labels]),
      providerKeys: [...source.providerKeys],
    }))
    .sort((left, right) => sourceOrder.indexOf(left.channel) - sourceOrder.indexOf(right.channel))
}

export function hasProvisionedMetaWebhookAssets(
  metadata: Record<string, unknown> | undefined,
  channel?: 'instagram' | 'messenger' | 'whatsapp',
): boolean {
  const channelKey = channel === 'messenger'
    ? 'meta_page_ids'
    : channel === 'instagram'
      ? 'meta_instagram_account_ids'
      : channel === 'whatsapp'
        ? 'meta_whatsapp_business_account_ids'
        : null
  if (channelKey) {
    const channelAssets = metadata?.[channelKey]
    if (channelAssets !== undefined) return hasMetadataAssetValue(channelAssets)
    // The previous connection shape had one untyped, mixed asset list. It is
    // still enough to prove a Page-backed Messenger lane, but never enough to
    // claim an Instagram or WhatsApp asset exists.
    if (channel !== 'messenger') return false
  }
  const raw = metadata?.webhook_account_ids ?? metadata?.webhookAccountIds
  return hasMetadataAssetValue(raw)
}

/**
 * Identifies a consented Meta channel that is still waiting for provider asset
 * discovery. This lets the UI give the operator the real next step instead of
 * presenting a generic empty inbox.
 */
export function isMetaInboxChannelAwaitingAssetProvision(
  connections: InboxConnection[],
  channel: 'instagram' | 'messenger' | 'whatsapp',
): boolean {
  const definition = (sourceDefinitions.meta ?? []).find((source) => source.channel === channel)
  if (!definition) return false

  return connections.some((connection) => {
    if (connection.deletedAt || !activeStatuses.has(connection.status.trim().toLowerCase())) return false
    const providerKey = normalizeProviderKey(firstNonEmpty(connection.providerKey, connection.providerId, connection.connectorType))
    if (!requiresTypedMetaAssetProof(providerKey, channel) || hasProvisionedMetaWebhookAssets(connection.metadata, channel)) return false
    const granted = new Set([...(connection.capabilities ?? []), ...(connection.scopes ?? [])]
      .map((value) => value.trim().toLowerCase()))
    return definition.grants?.some((grant) => granted.has(grant)) ?? false
  })
}

/**
 * Discord is only an inbox source after the OAuth connection has the
 * message-reading capability. The worker still needs the app bot token and a
 * guild binding, but those are runtime delivery gates rather than a reason to
 * advertise an unprovisioned connection as ready.
 */
export function isDiscordInboxChannelAwaitingSetup(connections: InboxConnection[]): boolean {
  const activeDiscordConnections = connections.filter((connection) => {
    if (connection.deletedAt || !activeStatuses.has(connection.status.trim().toLowerCase())) return false
    const providerKey = normalizeProviderKey(firstNonEmpty(connection.providerKey, connection.providerId, connection.connectorType))
    if (providerKey !== 'discord') return false
    const granted = new Set([...(connection.capabilities ?? []), ...(connection.scopes ?? [])]
      .map((value) => value.trim().toLowerCase()))
    return ['messages.read', 'bot'].some((grant) => granted.has(grant))
  })
  if (activeDiscordConnections.length === 0) return true
  return activeDiscordConnections.some((connection) => ['failed', 'error'].includes((connection.lastSyncStatus ?? '').trim().toLowerCase()))
}

function hasMetadataAssetValue(raw: unknown): boolean {
  if (Array.isArray(raw)) return raw.some((value) => typeof value === 'string' && value.trim().length > 0)
  return typeof raw === 'string' && raw.split(',').some((value) => value.trim().length > 0)
}

export function deriveConnectedEmailAccounts(connections: InboxConnection[]): ConnectedEmailAccount[] {
  return connections.flatMap((connection): ConnectedEmailAccount[] => {
    if (connection.deletedAt || !activeStatuses.has(connection.status.trim().toLowerCase())) return []
    const providerKey = normalizeProviderKey(firstNonEmpty(connection.providerKey, connection.providerId, connection.connectorType))
    const granted = new Set([...(connection.capabilities ?? []), ...(connection.scopes ?? [])]
      .map((value) => value.trim().toLowerCase()))
    const isMicrosoftMailbox = providerKey === 'microsoft' && ['mail.read', 'mail.readbasic'].some((grant) => granted.has(grant))
    const isGoogleMailbox = providerKey === 'google' && ['gmail.read', 'https://www.googleapis.com/auth/gmail.readonly'].some((grant) => granted.has(grant))
    if (!isMicrosoftMailbox && !isGoogleMailbox) return []

    const label = firstNonEmpty(
      metadataString(connection.metadata, 'mailbox_address'),
      connection.providerEmail,
      connection.userEmail,
      connection.displayName,
      connection.providerName,
      providerKey,
    )
    const sharedMailboxes = isMicrosoftMailbox ? readSharedMailboxes(connection.metadata) : []
    return [{
      id: connection.id,
      providerKey: isMicrosoftMailbox ? 'microsoft' : 'google',
      label,
      sharedMailboxes,
      syncHealth: emailAccountSyncHealth(connection),
      lastSyncAt: connection.lastSyncAt,
    }]
  }).sort((left, right) => providerOrder(left.providerKey) - providerOrder(right.providerKey) || left.label.localeCompare(right.label))
}

function emailAccountSyncHealth(connection: InboxConnection): EmailAccountSyncHealth {
  const connectionStatus = connection.status.trim().toLowerCase()
  const syncStatus = connection.lastSyncStatus?.trim().toLowerCase() ?? ''
  if (connectionStatus === 'needs_refresh' || ['authorization_incomplete', 'needs_refresh', 'token_expired'].includes(syncStatus)) {
    return 'needs_reconnect'
  }
  if (['failed', 'error'].includes(syncStatus)) return 'attention'
  if (['queued', 'pending', 'running', 'syncing'].includes(syncStatus)) return 'syncing'
  if (['synced', 'completed', 'success'].includes(syncStatus)) return 'synced'
  return 'unknown'
}

function formatSourceLabel(channel: InboxSourceChannel, labels: string[]): string {
  if (channel !== 'email') return labels[0] ?? channel
  return labels.sort((left, right) => emailLabelOrder.indexOf(left) - emailLabelOrder.indexOf(right)).join(' + ')
}

function firstNonEmpty(...values: Array<string | undefined>): string {
  return values.find((value) => value?.trim())?.trim() ?? ''
}

function normalizeProviderKey(value: string): string {
  switch (value.trim().toLowerCase().replaceAll('_', '-')) {
    case 'google-workspace':
    case 'gmail':
      return 'google'
    case 'microsoft-365':
    case 'microsoft-graph':
    case 'outlook':
      return 'microsoft'
    case 'twitter':
      return 'x'
    case 'facebook':
    case 'facebook-pages':
      return 'messenger'
    case 'meta-unified':
      return 'meta'
    case 'whatsapp-business':
    case 'whatsapp-cloud':
      return 'whatsapp'
    default:
      return value.trim().toLowerCase()
  }
}

function isMetaSocialChannel(channel: InboxSourceChannel): channel is 'messenger' | 'instagram' | 'whatsapp' {
  return channel === 'messenger' || channel === 'instagram' || channel === 'whatsapp'
}

function requiresTypedMetaAssetProof(providerKey: string, channel: InboxSourceChannel): boolean {
  return (providerKey === 'meta' && isMetaSocialChannel(channel))
    || (providerKey === 'instagram' && channel === 'instagram')
}

function readSharedMailboxes(metadata: Record<string, unknown> | undefined): string[] {
  const raw = metadata?.shared_mailboxes ?? metadata?.sharedMailboxes
  const values = Array.isArray(raw)
    ? raw
    : typeof raw === 'string'
      ? raw.split(',')
      : []
  return [...new Set(values
    .filter((value): value is string => typeof value === 'string')
    .map((value) => value.trim().toLowerCase())
    .filter((value) => /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(value)))]
}

function metadataString(metadata: Record<string, unknown> | undefined, key: string): string {
  const value = metadata?.[key]
  return typeof value === 'string' ? value.trim() : ''
}

function providerOrder(providerKey: ConnectedEmailAccount['providerKey']): number {
  return providerKey === 'microsoft' ? 0 : 1
}
