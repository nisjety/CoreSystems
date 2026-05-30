type IntegrationProviderKey =
  | 'github'
  | 'google'
  | 'google-drive'
  | 'hubspot'
  | 'microsoft'
  | 'notion'
  | 'shopify'
  | 'slack'
  | 'stripe'

export type IntegrationCategoryKey =
  | 'calendar-mail'
  | 'commerce'
  | 'communication'
  | 'crm'
  | 'developer'
  | 'documents'
  | 'finance'
  | 'storage'

interface IntegrationProviderMeta {
  key: IntegrationProviderKey
  label: string
  description: string
  defaultSources: string[]
  categories: IntegrationCategoryKey[]
}

export const INTEGRATION_PROVIDER_META: Record<IntegrationProviderKey, IntegrationProviderMeta> = {
  microsoft: {
    key: 'microsoft',
    label: 'Microsoft 365',
    description: 'Connect Outlook, Teams, SharePoint, and OneDrive into Aqencia\'s unified workspace layer.',
    defaultSources: ['sharepoint', 'onedrive', 'teams', 'outlook'],
    categories: ['communication', 'storage'],
  },
  google: {
    key: 'google',
    label: 'Google Workspace',
    description: 'Connect Gmail and Google Calendar for operational memory, planning, and retrieval.',
    defaultSources: ['gmail', 'google_calendar'],
    categories: ['calendar-mail', 'communication'],
  },
  'google-drive': {
    key: 'google-drive',
    label: 'Google Drive',
    description: 'Connect Drive files and folders for retrieval and knowledge sync.',
    defaultSources: ['google_drive', 'documents', 'folders'],
    categories: ['storage', 'documents'],
  },
  notion: {
    key: 'notion',
    label: 'Notion',
    description: 'Connect Notion pages, databases, and team knowledge.',
    defaultSources: ['pages', 'databases', 'content'],
    categories: ['documents'],
  },
  slack: {
    key: 'slack',
    label: 'Slack',
    description: 'Connect Slack channels and messages for conversation-aware knowledge capture.',
    defaultSources: ['messages'],
    categories: ['communication'],
  },
  github: {
    key: 'github',
    label: 'GitHub',
    description: 'Connect issues and pull requests for technical support context.',
    defaultSources: ['issues'],
    categories: ['developer'],
  },
  hubspot: {
    key: 'hubspot',
    label: 'HubSpot',
    description: 'Connect CRM contacts, companies, and deals.',
    defaultSources: ['contacts', 'deals'],
    categories: ['crm'],
  },
  shopify: {
    key: 'shopify',
    label: 'Shopify',
    description: 'Connect orders, customers, and products.',
    defaultSources: ['orders', 'customers', 'products'],
    categories: ['commerce'],
  },
  stripe: {
    key: 'stripe',
    label: 'Stripe',
    description: 'Connect customers, subscriptions, and invoices.',
    defaultSources: ['customers', 'subscriptions', 'invoices'],
    categories: ['finance'],
  },
}

const FALLBACK_SOURCE_LABELS: Record<string, string> = {
  content: 'Content',
  customers: 'Customers',
  databases: 'Databases',
  deals: 'Deals',
  documents: 'Documents',
  folders: 'Folders',
  gmail: 'Gmail',
  google_calendar: 'Google Calendar',
  google_drive: 'Google Drive',
  invoices: 'Invoices',
  issues: 'Issues',
  messages: 'Messages',
  onedrive: 'OneDrive',
  orders: 'Orders',
  outlook: 'Outlook',
  pages: 'Pages',
  products: 'Products',
  sharepoint: 'SharePoint',
  subscriptions: 'Subscriptions',
  teams: 'Teams',
}

export function getIntegrationProviderMeta(providerKey: string) {
  const normalized = providerKey.toLowerCase() as IntegrationProviderKey
  return INTEGRATION_PROVIDER_META[normalized] ?? null
}

export function getIntegrationProviderLabel(providerKey: string) {
  return getIntegrationProviderMeta(providerKey)?.label ?? providerKey
}

export function formatIntegrationSourceLabel(source: string) {
  return FALLBACK_SOURCE_LABELS[source] ?? source
}
