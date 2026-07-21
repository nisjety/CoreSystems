import type { IntegrationConnection } from '@/shared/api/integrations-client'

export type InboxSourceChannel =
  | 'messenger'
  | 'instagram'
  | 'whatsapp'
  | 'email'
  | 'slack'
  | 'teams'
  | 'discord'
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

type InboxConnection = Partial<IntegrationConnection> & {
  id: string
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
