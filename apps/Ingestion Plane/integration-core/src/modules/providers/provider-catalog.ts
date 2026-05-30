import { AppConfig } from '../../common/config/app-config';

export type ProviderKey =
  | 'google'
  | 'google-drive'
  | 'microsoft'
  | 'notion'
  | 'slack'
  | 'shopify'
  | 'stripe'
  | 'hubspot'
  | 'github';

export interface ProviderDefinition {
  key: ProviderKey;
  label: string;
  description: string;
  nangoIntegrationId: string;
  /** Data sources this provider exposes in the agent sidebar context panel. */
  sources: string[];
}

export function buildProviderCatalog(config: AppConfig): Record<ProviderKey, ProviderDefinition> {
  return {
    microsoft: {
      key: 'microsoft',
      label: 'Microsoft 365',
      description: 'Connect Outlook, Teams, SharePoint, and OneDrive through Velion.',
      nangoIntegrationId: config.microsoftIntegrationKey,
      sources: ['outlook', 'sharepoint', 'teams', 'onedrive']
    },
    google: {
      key: 'google',
      label: 'Google Workspace',
      description: 'Connect Gmail and Google Calendar through Velion.',
      nangoIntegrationId: config.googleIntegrationKey,
      sources: ['gmail', 'google_calendar']
    },
    'google-drive': {
      key: 'google-drive',
      label: 'Google Drive',
      description: 'Connect Drive files and folders for retrieval and knowledge sync.',
      nangoIntegrationId: config.googleDriveIntegrationKey,
      sources: ['google_drive', 'documents', 'folders']
    },
    notion: {
      key: 'notion',
      label: 'Notion',
      description: 'Connect Notion pages, databases, and workspace knowledge.',
      nangoIntegrationId: config.notionIntegrationKey,
      sources: ['pages', 'databases', 'content']
    },
    slack: {
      key: 'slack',
      label: 'Slack',
      description: 'Connect Slack messages through Velion.',
      nangoIntegrationId: config.slackIntegrationKey,
      sources: ['messages']
    },
    shopify: {
      key: 'shopify',
      label: 'Shopify',
      description: 'Show customer order history and product data in the support sidebar.',
      nangoIntegrationId: config.shopifyIntegrationKey,
      sources: ['orders', 'customers', 'products']
    },
    stripe: {
      key: 'stripe',
      label: 'Stripe',
      description: 'Show customer subscriptions and billing status in the support sidebar.',
      nangoIntegrationId: config.stripeIntegrationKey,
      sources: ['customers', 'subscriptions', 'invoices']
    },
    hubspot: {
      key: 'hubspot',
      label: 'HubSpot',
      description: 'Show CRM contacts, deals, and notes in the support sidebar.',
      nangoIntegrationId: config.hubspotIntegrationKey,
      sources: ['contacts', 'deals']
    },
    github: {
      key: 'github',
      label: 'GitHub',
      description: 'Show linked issues and pull requests for technical support context.',
      nangoIntegrationId: config.githubIntegrationKey,
      sources: ['issues']
    }
  };
}

export function listProviders(config: AppConfig): ProviderDefinition[] {
  return Object.values(buildProviderCatalog(config));
}

export function getProviderDefinition(
  config: AppConfig,
  providerKey: string
): ProviderDefinition | null {
  const normalized = normalizeProviderKey(providerKey);
  return buildProviderCatalog(config)[normalized] ?? null;
}

function normalizeProviderKey(providerKey: string): ProviderKey {
  const normalized = providerKey.trim().toLowerCase();

  switch (normalized) {
    case 'gdrive':
    case 'google_drive':
    case 'drive':
      return 'google-drive';
    case 'm365':
    case 'microsoft365':
    case 'microsoft-365':
    case 'teams':
    case 'sharepoint':
    case 'onedrive':
    case 'outlook':
      return 'microsoft';
    default:
      return normalized as ProviderKey;
  }
}
