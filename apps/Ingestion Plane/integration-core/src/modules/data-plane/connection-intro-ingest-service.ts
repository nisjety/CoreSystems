import { AppConfig } from '../../common/config/app-config';
import { NangoRuntimeClient } from '../../common/runtime/nango-runtime-client';
import { ConnectionMapping } from '../connections/connection-mapping-repository';

interface IngestConnectionIntroInput {
  connection: ConnectionMapping;
  intro?: ConnectionIntroSnapshot;
  selectedSources: string[];
  webhookEventId: string;
}

interface BuildConnectionIntroInput {
  nangoConnectionId: string;
  nangoIntegrationId: string;
  providerKey: string;
  selectedSources: string[];
}

export interface ConnectionIntroItem {
  id: string;
  kind: string;
  label: string;
  source: string;
  url?: string;
}

export interface ConnectionIntroSnapshot {
  fetchedAt?: string;
  items: ConnectionIntroItem[];
  error?: string;
}

const INTRO_FETCH_TIMEOUT_MS = 4_000;
const INTRO_ITEM_LIMIT = 12;
const DATA_PLANE_INGEST_TIMEOUT_MS = 10_000;

export class ConnectionIntroIngestService {
  private readonly documentsUrl: string;
  private readonly internalApiKey: string;

  constructor(
    config: AppConfig,
    private readonly runtimeClient?: NangoRuntimeClient
  ) {
    this.documentsUrl = config.dataPlaneDocumentsUrl.replace(/\/+$/, '');
    this.internalApiKey =
      config.dataPlaneInternalApiKey ||
      config.authCoreInternalApiKey ||
      config.connectorRuntimeWebhookSecret;
  }

  async buildIntroSnapshot(input: BuildConnectionIntroInput): Promise<ConnectionIntroSnapshot> {
    if (!this.runtimeClient) return { items: [] };

    try {
      const accessToken = await this.runtimeClient.getConnectionToken(
        input.nangoIntegrationId,
        input.nangoConnectionId
      );
      const items = await fetchProviderIntroItems({
        accessToken,
        providerKey: input.providerKey,
        selectedSources: input.selectedSources
      });

      return {
        fetchedAt: new Date().toISOString(),
        items
      };
    } catch (error) {
      return {
        error: introErrorMessage(error),
        fetchedAt: new Date().toISOString(),
        items: []
      };
    }
  }

  async ingestConnectionIntro(input: IngestConnectionIntroInput): Promise<void> {
    if (!this.internalApiKey) return;

    const { connection, intro, selectedSources, webhookEventId } = input;
    const introLines =
      intro?.items.length
        ? [
            '',
            'Verified workspace preview:',
            ...intro.items.map((item) => `- ${item.kind}: ${item.label}`)
          ]
        : intro?.error
          ? ['', `Workspace preview was unavailable: ${intro.error}`]
          : [];

    const content = [
      `${connection.providerLabel} is connected to Velion.`,
      `Provider key: ${connection.providerKey}.`,
      `Selected sources: ${selectedSources.length > 0 ? selectedSources.join(', ') : 'all available sources'}.`,
      `Connection status: ${connection.status}.`,
      `Connected user: ${connection.userEmail || connection.userId}.`,
      `Workspace: ${connection.workspaceId}.`,
      ...introLines
    ].join('\n');

    const response = await fetch(`${this.documentsUrl}/v1/documents`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'X-Internal-Api-Key': this.internalApiKey,
        'X-Org-ID': connection.organizationId,
        'X-User-Id': connection.userId,
        ...(connection.userEmail ? { 'X-User-Email': connection.userEmail } : {}),
      },
      body: JSON.stringify({
        source: `integration:${connection.providerKey}:${connection.nangoConnectionId}`,
        type: 'integration_connection',
        title: `${connection.providerLabel} connection`,
        content,
        created_by: connection.userId,
        idempotency_key: `integration:${connection.nangoConnectionId}`,
        zdr_classification: 'internal',
        metadata: {
          connection_id: connection.id,
          intro_error: intro?.error,
          intro_items: intro?.items ?? [],
          intro_fetched_at: intro?.fetchedAt,
          nango_connection_id: connection.nangoConnectionId,
          nango_integration_id: connection.nangoIntegrationId,
          provider_key: connection.providerKey,
          selected_sources: selectedSources,
          webhook_event_id: webhookEventId,
        },
      }),
      signal: AbortSignal.timeout(DATA_PLANE_INGEST_TIMEOUT_MS),
    });

    if (!response.ok) {
      const detail = await response.text().catch(() => '');
      throw new Error(detail || `Data Plane documents returned ${response.status}`);
    }
  }
}

async function fetchProviderIntroItems(input: {
  accessToken: string;
  providerKey: string;
  selectedSources: string[];
}): Promise<ConnectionIntroItem[]> {
  switch (input.providerKey) {
    case 'slack':
      return fetchSlackIntro(input.accessToken);
    case 'notion':
      return fetchNotionIntro(input.accessToken);
    case 'google-drive':
      return fetchGoogleDriveIntro(input.accessToken);
    case 'github':
      return fetchGithubIntro(input.accessToken);
    case 'microsoft':
      return fetchMicrosoftIntro(input.accessToken, input.selectedSources);
    case 'stripe':
      return fetchStripeIntro(input.accessToken);
    case 'hubspot':
      return fetchHubSpotIntro(input.accessToken);
    default:
      return [];
  }
}

async function fetchSlackIntro(accessToken: string): Promise<ConnectionIntroItem[]> {
  const body = await fetchJson<{ channels?: Array<{ id?: string; name?: string }> }>(
    'https://slack.com/api/conversations.list?limit=10&types=public_channel,private_channel',
    { headers: bearerHeaders(accessToken) }
  );

  return uniqueIntroItems(
    (body.channels ?? []).flatMap((channel) => {
      if (!channel.id || !channel.name) return [];
      return [{
        id: channel.id,
        kind: 'channel',
        label: `#${channel.name}`,
        source: 'slack'
      }];
    })
  );
}

async function fetchNotionIntro(accessToken: string): Promise<ConnectionIntroItem[]> {
  const body = await fetchJson<{ results?: unknown[] }>(
    'https://api.notion.com/v1/search',
    {
      body: JSON.stringify({
        page_size: 10,
        sort: { direction: 'descending', timestamp: 'last_edited_time' }
      }),
      headers: {
        ...bearerHeaders(accessToken),
        'Content-Type': 'application/json',
        'Notion-Version': '2022-06-28'
      },
      method: 'POST'
    }
  );

  return uniqueIntroItems(
    (body.results ?? []).flatMap((result) => {
      const record = result as Record<string, unknown>;
      const id = stringValue(record.id);
      if (!id) return [];
      const label = notionTitle(record) || id.slice(0, 8);
      return [{
        id,
        kind: stringValue(record.object) || 'notion',
        label,
        source: 'notion',
        url: stringValue(record.url)
      }];
    })
  );
}

async function fetchGoogleDriveIntro(accessToken: string): Promise<ConnectionIntroItem[]> {
  const params = new URLSearchParams({
    fields: 'files(id,name,mimeType,webViewLink)',
    pageSize: '10',
    q: 'trashed=false'
  });
  const body = await fetchJson<{
    files?: Array<{ id?: string; mimeType?: string; name?: string; webViewLink?: string }>;
  }>(
    `https://www.googleapis.com/drive/v3/files?${params.toString()}`,
    { headers: bearerHeaders(accessToken) }
  );

  return uniqueIntroItems(
    (body.files ?? []).flatMap((file) => {
      if (!file.id || !file.name) return [];
      return [{
        id: file.id,
        kind: file.mimeType?.includes('folder') ? 'folder' : 'file',
        label: file.name,
        source: 'google-drive',
        url: file.webViewLink
      }];
    })
  );
}

async function fetchGithubIntro(accessToken: string): Promise<ConnectionIntroItem[]> {
  const body = await fetchJson<Array<{ full_name?: string; html_url?: string; id?: number; name?: string }>>(
    'https://api.github.com/user/repos?per_page=10&sort=updated',
    {
      headers: {
        ...bearerHeaders(accessToken),
        Accept: 'application/vnd.github+json',
        'X-GitHub-Api-Version': '2022-11-28'
      }
    }
  );

  return uniqueIntroItems(
    body.flatMap((repo) => {
      if (!repo.id || !(repo.full_name || repo.name)) return [];
      return [{
        id: String(repo.id),
        kind: 'repository',
        label: repo.full_name || repo.name || String(repo.id),
        source: 'github',
        url: repo.html_url
      }];
    })
  );
}

async function fetchMicrosoftIntro(
  accessToken: string,
  selectedSources: string[]
): Promise<ConnectionIntroItem[]> {
  const requests: Array<Promise<ConnectionIntroItem[]>> = [
    fetchMicrosoftProfileIntro(accessToken)
  ];
  if (selectedSources.length === 0 || selectedSources.includes('teams')) {
    requests.push(fetchMicrosoftTeamsIntro(accessToken));
  }
  if (selectedSources.length === 0 || selectedSources.includes('outlook')) {
    requests.push(fetchMicrosoftOutlookIntro(accessToken));
  }
  if (
    selectedSources.length === 0 ||
    selectedSources.includes('sharepoint') ||
    selectedSources.includes('onedrive')
  ) {
    requests.push(fetchMicrosoftDriveIntro(accessToken));
  }

  const results = await Promise.allSettled(requests);
  return uniqueIntroItems(
    results.flatMap((result) => result.status === 'fulfilled' ? result.value : [])
  );
}

async function fetchMicrosoftProfileIntro(accessToken: string): Promise<ConnectionIntroItem[]> {
  const body = await fetchJson<{ displayName?: string; id?: string; mail?: string; userPrincipalName?: string }>(
    'https://graph.microsoft.com/v1.0/me',
    { headers: bearerHeaders(accessToken) }
  );
  if (!body.id) return [];
  return [{
    id: body.id,
    kind: 'user',
    label: body.displayName || body.mail || body.userPrincipalName || body.id,
    source: 'microsoft'
  }];
}

async function fetchMicrosoftTeamsIntro(accessToken: string): Promise<ConnectionIntroItem[]> {
  const body = await fetchJson<{ value?: Array<{ displayName?: string; id?: string; webUrl?: string }> }>(
    'https://graph.microsoft.com/v1.0/me/joinedTeams?$top=10',
    { headers: bearerHeaders(accessToken) }
  );
  return uniqueIntroItems(
    (body.value ?? []).flatMap((team) => {
      if (!team.id || !team.displayName) return [];
      return [{
        id: team.id,
        kind: 'team',
        label: team.displayName,
        source: 'microsoft',
        url: team.webUrl
      }];
    })
  );
}

async function fetchMicrosoftOutlookIntro(accessToken: string): Promise<ConnectionIntroItem[]> {
  const params = new URLSearchParams({
    '$select': 'id,subject,webLink,from,receivedDateTime',
    '$top': '10'
  });
  const body = await fetchJson<{
    value?: Array<{
      from?: { emailAddress?: { address?: string; name?: string } };
      id?: string;
      receivedDateTime?: string;
      subject?: string;
      webLink?: string;
    }>;
  }>(
    `https://graph.microsoft.com/v1.0/me/messages?${params.toString()}`,
    { headers: bearerHeaders(accessToken) }
  );
  return uniqueIntroItems(
    (body.value ?? []).flatMap((message) => {
      if (!message.id) return [];
      const sender = message.from?.emailAddress?.name || message.from?.emailAddress?.address;
      const label = [message.subject, sender].filter(Boolean).join(' - ') || message.id;
      return [{
        id: message.id,
        kind: 'email',
        label,
        source: 'microsoft',
        url: message.webLink
      }];
    })
  );
}

async function fetchMicrosoftDriveIntro(accessToken: string): Promise<ConnectionIntroItem[]> {
  const body = await fetchJson<{ value?: Array<{ file?: unknown; folder?: unknown; id?: string; name?: string; webUrl?: string }> }>(
    'https://graph.microsoft.com/v1.0/me/drive/root/children?$top=10',
    { headers: bearerHeaders(accessToken) }
  );
  return uniqueIntroItems(
    (body.value ?? []).flatMap((item) => {
      if (!item.id || !item.name) return [];
      return [{
        id: item.id,
        kind: item.folder ? 'folder' : 'file',
        label: item.name,
        source: 'microsoft',
        url: item.webUrl
      }];
    })
  );
}

async function fetchStripeIntro(accessToken: string): Promise<ConnectionIntroItem[]> {
  const body = await fetchJson<{ data?: Array<{ email?: string; id?: string; name?: string }> }>(
    'https://api.stripe.com/v1/customers?limit=10',
    { headers: bearerHeaders(accessToken) }
  );
  return uniqueIntroItems(
    (body.data ?? []).flatMap((customer) => {
      if (!customer.id) return [];
      return [{
        id: customer.id,
        kind: 'customer',
        label: customer.name || customer.email || customer.id,
        source: 'stripe'
      }];
    })
  );
}

async function fetchHubSpotIntro(accessToken: string): Promise<ConnectionIntroItem[]> {
  const body = await fetchJson<{
    results?: Array<{ id?: string; properties?: { email?: string; firstname?: string; lastname?: string } }>;
  }>(
    'https://api.hubapi.com/crm/v3/objects/contacts?limit=10&properties=email,firstname,lastname',
    { headers: bearerHeaders(accessToken) }
  );
  return uniqueIntroItems(
    (body.results ?? []).flatMap((contact) => {
      if (!contact.id) return [];
      const name = [contact.properties?.firstname, contact.properties?.lastname]
        .filter(Boolean)
        .join(' ')
        .trim();
      return [{
        id: contact.id,
        kind: 'contact',
        label: name || contact.properties?.email || contact.id,
        source: 'hubspot'
      }];
    })
  );
}

async function fetchJson<T>(url: string, init: RequestInit): Promise<T> {
  const response = await fetch(url, {
    ...init,
    signal: AbortSignal.timeout(INTRO_FETCH_TIMEOUT_MS)
  });
  if (!response.ok) {
    throw new Error(`Provider intro returned ${response.status}`);
  }
  return response.json() as Promise<T>;
}

function bearerHeaders(accessToken: string): Record<string, string> {
  return {
    Accept: 'application/json',
    Authorization: `Bearer ${accessToken}`
  };
}

function uniqueIntroItems(items: ConnectionIntroItem[]): ConnectionIntroItem[] {
  const seen = new Set<string>();
  const unique: ConnectionIntroItem[] = [];
  for (const item of items) {
    const key = `${item.source}:${item.kind}:${item.id}`;
    if (seen.has(key)) continue;
    seen.add(key);
    unique.push(item);
    if (unique.length >= INTRO_ITEM_LIMIT) break;
  }
  return unique;
}

function notionTitle(record: Record<string, unknown>): string | null {
  const title = textArrayPlainText(record.title);
  if (title) return title;

  const properties = record.properties;
  if (!properties || typeof properties !== 'object') return null;
  for (const value of Object.values(properties as Record<string, unknown>)) {
    if (!value || typeof value !== 'object') continue;
    const prop = value as Record<string, unknown>;
    const propTitle = textArrayPlainText(prop.title);
    if (propTitle) return propTitle;
  }
  return null;
}

function textArrayPlainText(value: unknown): string | null {
  if (!Array.isArray(value)) return null;
  const text = value
    .map((entry) => {
      if (!entry || typeof entry !== 'object') return '';
      return stringValue((entry as Record<string, unknown>).plain_text);
    })
    .join('')
    .trim();
  return text || null;
}

function stringValue(value: unknown): string | undefined {
  return typeof value === 'string' && value.trim() ? value.trim() : undefined;
}

function introErrorMessage(error: unknown): string {
  if (error instanceof Error && error.message.trim()) {
    return error.message;
  }
  return 'Provider intro fetch failed';
}
