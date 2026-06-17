import { ChatStoreError, resolveChatActor } from '../../chat/_lib/session-store'

import {
  INTEGRATION_PROVIDER_META,
  getIntegrationProviderLabel,
  getIntegrationProviderMeta,
} from '@/lib/integrations/catalog'
import type {
  IntegrationConnectionSummary,
  IntegrationProviderLinkSummary,
  IntegrationProviderSummary,
  KnowledgeDocumentSummary,
  KnowledgeIntegrationsResponse,
  KnowledgeSourceStatus,
  KnowledgeSourceSummary,
} from '@/lib/integrations/types'

const DOCUMENTS_SERVICE_URL =
  process.env.DOCUMENTS_SERVICE_URL ||
  process.env.DOCS_SERVICE_URL ||
  'http://documents-service:8001'

const INTEGRATION_SERVICE_URL =
  process.env.INTEGRATION_CORE_URL ||
  process.env.INTEGRATION_ENGINE_URL ||
  'http://integration-api:3026'

const INTERNAL_API_KEY = (process.env.INTERNAL_API_KEY || process.env.INTERNAL_SERVICE_SECRET) as string

if (!INTERNAL_API_KEY) {
  throw new Error(
    'INTERNAL_API_KEY or INTERNAL_SERVICE_SECRET environment variable is required for inter-service authentication'
  )
}

type IntegrationCoreConnection = {
  id: string
  org_id: string
  user_id: string
  provider: string
  organizationId?: string
  workspaceId?: string
  userId?: string
  providerKey?: string
  providerLabel?: string
  integration_key?: string
  external_connection_id?: string
  selected_sources?: string[]
  selectedSources?: string[]
  status?: string
  lastSyncStatus?: string | null
  lastSyncSummary?: Record<string, unknown> | null
  sync_status?: string | null
  sync_error?: string | null
  last_synced_at?: string | null
  lastSyncedAt?: string | null
  createdAt?: string | null
  updatedAt?: string | null
  deletedAt?: string | null
  created_at?: string | null
  updated_at?: string | null
  link_status?: string | null
}

type IntegrationCoreProviderLink = {
  id: string
  provider: string
  status?: string | null
  scopes_granted?: string[]
  last_sign_in_at?: string | null
  last_linked_at?: string | null
  created_at?: string | null
  updated_at?: string | null
}

type IntegrationCoreProvider = {
  key: string
  label: string
  description?: string
  sources?: string[]
  supported_sources?: string[]
  default_sources?: string[]
  auth_execution?: string | null
  sync_execution?: string | null
  configured?: boolean
}

type DocumentsServiceResponse = {
  documents?: Array<{
    document_id: string
    title: string
    source: string
    type: string
    status: string
    metadata?: Record<string, unknown>
    created_at: string
    updated_at: string
  }>
}

/**
 * Wave 11 Phase 1: every Data Plane call MUST carry `X-Org-ID`. The
 * documents-service middleware rejects requests without it ("X-Org-ID
 * header required"), and we honour that — the Data Plane is the
 * canonical owner of org-scoped knowledge per
 * `apps/master-ownership-matrix.md`.
 *
 * The actor object now accepts `orgId` as a required field. Existing
 * callers all derive from `resolveChatActor()` which already populates
 * `actor.orgId`, so the change is local.
 */
function buildInternalHeaders(actor: {
  userId: string
  userEmail: string
  userName: string
  orgId: string
}): HeadersInit {
  return {
    'Content-Type': 'application/json',
    'X-Internal-Api-Key': INTERNAL_API_KEY,
    'X-Org-ID': actor.orgId,
    'X-User-Id': actor.userId,
    'X-User-Email': actor.userEmail,
    'X-User-Name': actor.userName,
  }
}

async function fetchJson<T>(url: string, init?: RequestInit) {
  const response = await fetch(url, {
    ...init,
    cache: 'no-store',
  })

  if (!response.ok) {
    const rawBody = await response.text().catch(() => '')
    let payload: unknown = null

    if (rawBody) {
      try {
        payload = JSON.parse(rawBody)
      } catch {
        payload = rawBody
      }
    }

    const toMessage = (value: unknown): string | null => {
      if (typeof value === 'string') {
        const trimmed = value.trim()
        return trimmed ? trimmed : null
      }

      if (!value || typeof value !== 'object') {
        return null
      }

      const record = value as Record<string, unknown>
      const direct =
        toMessage(record.detail) ||
        toMessage(record.error) ||
        toMessage(record.message) ||
        toMessage(record.reason)

      if (direct) {
        return direct
      }

      try {
        return JSON.stringify(record)
      } catch {
        return null
      }
    }

    const detail = toMessage(payload)
    throw new Error(detail || `Upstream request failed: ${response.status}`)
  }

  return (await response.json()) as T
}

function getDocumentSourceUrl(
  source: string,
  metadata?: Record<string, unknown>,
) {
  const metadataUrl =
    typeof metadata?.url === 'string'
      ? metadata.url
      : typeof metadata?.source_url === 'string'
        ? metadata.source_url
        : typeof metadata?.page_url === 'string'
          ? metadata.page_url
          : null

  if (metadataUrl) {
    return metadataUrl
  }

  if (source.startsWith('website-crawl:')) {
    return source.slice('website-crawl:'.length)
  }

  return null
}

function normalizeDocument(
  document: NonNullable<DocumentsServiceResponse['documents']>[number],
): KnowledgeDocumentSummary {
  return {
    id: document.document_id,
    title: document.title,
    source: document.source,
    type: document.type,
    status: document.status,
    createdAt: document.created_at,
    updatedAt: document.updated_at,
    sourceUrl: getDocumentSourceUrl(document.source, document.metadata),
  }
}

function normalizeConnection(connection: IntegrationCoreConnection): IntegrationConnectionSummary {
  const provider =
    connection.providerKey || connection.provider || connection.integration_key || 'unknown'
  const summarySources = Array.isArray(connection.lastSyncSummary?.selectedSources)
    ? connection.lastSyncSummary.selectedSources.filter(
        (value): value is string => typeof value === 'string',
      )
    : []
  const selectedSources = Array.isArray(connection.selectedSources)
    ? connection.selectedSources
    : Array.isArray(connection.selected_sources)
      ? connection.selected_sources
      : summarySources

  return {
    id: connection.id,
    provider,
    label: connection.providerLabel || getIntegrationProviderLabel(provider),
    selectedSources,
    status: connection.status || 'connected',
    linkStatus: connection.link_status || null,
    syncStatus: connection.lastSyncStatus || connection.sync_status || null,
    syncError: connection.sync_error || null,
    lastSyncedAt: connection.lastSyncedAt || connection.last_synced_at || null,
    createdAt: connection.createdAt || connection.created_at || null,
    updatedAt: connection.updatedAt || connection.updated_at || null,
  }
}

function normalizeProviderLink(link: IntegrationCoreProviderLink): IntegrationProviderLinkSummary {
  return {
    id: link.id,
    provider: link.provider,
    status: link.status || 'signed_in_detected',
    scopesGranted: Array.isArray(link.scopes_granted) ? link.scopes_granted : [],
    lastSignInAt: link.last_sign_in_at || null,
    lastLinkedAt: link.last_linked_at || null,
    createdAt: link.created_at || null,
    updatedAt: link.updated_at || null,
  }
}

async function fetchProviderCatalog(): Promise<IntegrationCoreProvider[]> {
  try {
    const payload = await fetchJson<{
      data?: { providers?: IntegrationCoreProvider[] }
      providers?: IntegrationCoreProvider[]
    }>(
      `${INTEGRATION_SERVICE_URL}/api/v1/providers`,
      {
        headers: { 'Content-Type': 'application/json' },
      },
    )

    const providers = payload.data?.providers ?? payload.providers ?? []
    if (Array.isArray(providers) && providers.length > 0) {
      return providers
    }
  } catch {
    // Fall through to the local catalog when integration-core is unavailable.
  }

  return Object.values(INTEGRATION_PROVIDER_META).map((provider) => ({
    key: provider.key,
    label: provider.label,
    description: provider.description,
    supported_sources: provider.defaultSources,
    default_sources: provider.defaultSources,
    sources: provider.defaultSources,
    auth_execution: 'first_party',
    sync_execution: 'first_party',
    configured: true,
  }))
}

async function fetchConnectionsForActor(orgId: string) {
  try {
    const params = new URLSearchParams({ organizationId: orgId })
    const payload = await fetchJson<{
      data?: { connections?: IntegrationCoreConnection[] }
      connections?: IntegrationCoreConnection[]
    }>(
      `${INTEGRATION_SERVICE_URL}/api/v1/connections?${params.toString()}`,
      {
        headers: {
          'Content-Type': 'application/json',
          'X-Internal-Api-Key': INTERNAL_API_KEY,
          'X-Org-ID': orgId,
        },
      },
    )

    const connections = payload.data?.connections ?? payload.connections ?? []
    return Array.isArray(connections) ? connections : []
  } catch {
    return []
  }
}

async function fetchProviderLinksForActor(userId: string) {
  try {
    const payload = await fetchJson<{ provider_links?: IntegrationCoreProviderLink[] }>(
      `${INTEGRATION_SERVICE_URL}/api/v1/provider-links/${encodeURIComponent(userId)}`,
      {
        headers: { 'Content-Type': 'application/json' },
      },
    )

    return Array.isArray(payload.provider_links) ? payload.provider_links : []
  } catch {
    return []
  }
}

async function fetchDocumentsForActor(options?: {
  limit?: number
  q?: string
  status?: string
  type?: string
}) {
  const actor = await resolveChatActor()
  const params = new URLSearchParams({
    org_id: actor.orgId,
    limit: String(options?.limit ?? 200),
    offset: '0',
  })

  if (options?.q) {
    params.set('q', options.q)
  }
  if (options?.status) {
    params.set('status', options.status)
  }
  if (options?.type) {
    params.set('type', options.type)
  }

  // Wave 11 Phase 1: degrade gracefully when documents-service is offline
  // in dev. The page must still render — empty-state is the honest signal
  // for "no documents", and an unreachable backend is functionally the
  // same as an empty corpus from the UI's perspective.
  let payload: DocumentsServiceResponse = {}
  try {
    payload = await fetchJson<DocumentsServiceResponse>(
      `${DOCUMENTS_SERVICE_URL}/v1/documents?${params.toString()}`,
      {
        headers: buildInternalHeaders(actor),
      },
    )
  } catch (error) {
    // Auth and validation errors should still surface — those are bugs
    // in our request shape, not infrastructure problems. We only swallow
    // network-level failures (ECONNREFUSED, timeouts, DNS).
    const message = error instanceof Error ? error.message : ''
    const isNetworkError =
      /fetch failed|ECONNREFUSED|ENOTFOUND|ETIMEDOUT|connect|network|aborted/i.test(message)
    if (!isNetworkError) {
      throw error
    }
  }

  return {
    actor,
    documents: Array.isArray(payload.documents)
      ? payload.documents
        .map(normalizeDocument)
        .sort((left, right) => Date.parse(right.updatedAt) - Date.parse(left.updatedAt))
      : [],
  }
}

function statusFromDocumentStatuses(statuses: string[]): KnowledgeSourceStatus {
  const normalized = statuses.map((status) => status.toLowerCase())

  if (normalized.some((status) => status === 'failed' || status === 'error')) {
    return 'error'
  }

  if (normalized.some((status) => status === 'processing' || status === 'indexing')) {
    return 'indexing'
  }

  if (normalized.some((status) => status === 'pending' || status === 'queued')) {
    return 'pending'
  }

  return 'active'
}

function buildSourceName(sourceUrl: string) {
  try {
    const parsed = new URL(sourceUrl)
    return parsed.hostname.replace(/^www\./, '')
  } catch {
    return sourceUrl
  }
}

export async function getKnowledgeIntegrations(): Promise<KnowledgeIntegrationsResponse> {
  const actor = await resolveChatActor()
  const [connections, providerLinks, providerCatalog] = await Promise.all([
    fetchConnectionsForActor(actor.orgId),
    fetchProviderLinksForActor(actor.userId),
    fetchProviderCatalog(),
  ])

  const actorConnections = connections
    .filter((connection) => (connection.userId || connection.user_id) === actor.userId)
    .map(normalizeConnection)
  const actorProviderLinks = providerLinks.map(normalizeProviderLink)

  const latestByProvider = new Map<string, IntegrationConnectionSummary>()
  for (const connection of actorConnections) {
    latestByProvider.set(connection.provider, connection)
  }
  const linkByProvider = new Map<string, IntegrationProviderLinkSummary>()
  for (const link of actorProviderLinks) {
    linkByProvider.set(link.provider, link)
  }

  const providers: IntegrationProviderSummary[] = providerCatalog.map((provider) => {
    const meta = getIntegrationProviderMeta(provider.key)
    const connection = latestByProvider.get(provider.key) ?? null
    const providerLink = linkByProvider.get(provider.key) ?? null

    return {
      key: provider.key,
      label: meta?.label ?? provider.label,
      description: provider.description ?? meta?.description ?? `${provider.label} integration`,
      supportedSources: Array.isArray(provider.supported_sources)
        ? provider.supported_sources
        : Array.isArray(provider.sources)
          ? provider.sources
        : [],
      defaultSources: Array.isArray(provider.default_sources)
        ? provider.default_sources
        : meta?.defaultSources ??
        (Array.isArray(provider.supported_sources)
          ? provider.supported_sources
          : Array.isArray(provider.sources)
            ? provider.sources
            : []),
      categories: meta?.categories ?? [],
      configured: provider.configured ?? true,
      authExecution: provider.auth_execution ?? null,
      syncExecution: provider.sync_execution ?? null,
      connected: connection !== null,
      signInLinked: providerLink !== null,
      dataAccessReady:
        providerLink?.status === 'connected_for_data' ||
        providerLink?.status === 'connected_via_auth_core',
      providerLink,
      connection,
    }
  })

  return {
    orgId: actor.orgId,
    userId: actor.userId,
    totalConnected: actorConnections.length,
    connections: actorConnections,
    providerLinks: actorProviderLinks,
    providers,
  }
}

export async function getKnowledgeDocuments(options?: {
  limit?: number
  q?: string
  status?: string
  type?: string
}) {
  const { actor, documents } = await fetchDocumentsForActor(options)
  return {
    orgId: actor.orgId,
    total: documents.length,
    documents,
  }
}

export async function getKnowledgeSources() {
  const { actor, documents } = await fetchDocumentsForActor({ limit: 200 })
  const sourceMap = new Map<string, KnowledgeSourceSummary & { statuses: string[] }>()

  for (const document of documents) {
    if (!document.source.startsWith('website-crawl:')) {
      continue
    }

    const sourceUrl = document.sourceUrl || document.source.slice('website-crawl:'.length)
    if (!sourceUrl) {
      continue
    }

    const existing = sourceMap.get(sourceUrl)
    if (existing) {
      existing.pageCount += 1
      existing.documentCount += 1
      existing.statuses.push(document.status)
      if (!existing.lastIndexed || existing.lastIndexed < document.updatedAt) {
        existing.lastIndexed = document.updatedAt
      }
      continue
    }

    sourceMap.set(sourceUrl, {
      id: sourceUrl,
      name: buildSourceName(sourceUrl),
      url: sourceUrl,
      type: 'website',
      status: 'active',
      pageCount: 1,
      documentCount: 1,
      lastIndexed: document.updatedAt,
      statuses: [document.status],
    })
  }

  const sources = Array.from(sourceMap.values())
    .map(({ statuses, ...source }) => ({
      ...source,
      status: statusFromDocumentStatuses(statuses),
    }))
    .sort((left, right) => {
      const leftTime = left.lastIndexed ? Date.parse(left.lastIndexed) : 0
      const rightTime = right.lastIndexed ? Date.parse(right.lastIndexed) : 0
      return rightTime - leftTime
    })

  return {
    orgId: actor.orgId,
    total: sources.length,
    sources,
  }
}

/**
 * Wave 11 §2.2: real ingest helper. Routes:
 *   - kind=file → multipart POST /v1/documents with the binary
 *   - kind=text → JSON POST /v1/documents with `{type:'text', title, content}`
 *
 * Both shapes use the same documents-service endpoint; the backend
 * dispatches to its file-parser or text-handler based on `type`.
 */
type CreateDocInput =
  | { kind: 'file'; file: File }
  | { kind: 'text'; title: string; content: string }

export async function createKnowledgeDocument(input: CreateDocInput) {
  const actor = await resolveChatActor()

  if (input.kind === 'file') {
    const form = new FormData()
    form.append('file', input.file)
    form.append('org_id', actor.orgId)
    form.append('user_id', actor.userId)

    // Multipart: must NOT set Content-Type ourselves (browser/fetch sets the boundary).
    const headers: Record<string, string> = {
      'X-Internal-Api-Key': INTERNAL_API_KEY,
      'X-Org-ID': actor.orgId,
      'X-User-Id': actor.userId,
      'X-User-Email': actor.userEmail,
      'X-User-Name': actor.userName,
    }

    const response = await fetch(`${DOCUMENTS_SERVICE_URL}/v1/documents`, {
      method: 'POST',
      headers,
      body: form,
    })
    if (!response.ok) {
      const detail = await response.text().catch(() => '')
      throw new Error(detail || `Upstream upload failed: ${response.status}`)
    }
    return await response.json()
  }

  // Text ingest path.
  const payload = await fetchJson<unknown>(
    `${DOCUMENTS_SERVICE_URL}/v1/documents`,
    {
      method: 'POST',
      headers: buildInternalHeaders(actor),
      body: JSON.stringify({
        org_id: actor.orgId,
        user_id: actor.userId,
        type: 'text',
        title: input.title,
        content: input.content,
        source: `text:${actor.userId}:${Date.now()}`,
      }),
    },
  )
  return payload
}

export async function getKnowledgeDocumentDetail(documentId: string) {
  const actor = await resolveChatActor()
  const response = await fetch(
    `${DOCUMENTS_SERVICE_URL}/v1/documents/${encodeURIComponent(documentId)}?org_id=${encodeURIComponent(actor.orgId)}`,
    {
      headers: buildInternalHeaders(actor),
      cache: 'no-store',
    },
  )
  if (!response.ok) {
    if (response.status === 404) {
      throw new ChatStoreError('Document not found', 404)
    }
    const detail = await response.text().catch(() => '')
    throw new Error(detail || `Upstream fetch failed: ${response.status}`)
  }
  const raw = (await response.json()) as {
    document_id: string
    title: string
    source: string
    type: string
    status: string
    content?: string
    extracted_text?: string
    metadata?: Record<string, unknown>
    bound_agents?: Array<{ id: string; name: string }>
    created_at: string
    updated_at: string
  }
  return {
    id: raw.document_id,
    title: raw.title,
    source: raw.source,
    type: raw.type,
    status: raw.status,
    content: raw.content ?? raw.extracted_text ?? '',
    agents: Array.isArray(raw.bound_agents) ? raw.bound_agents : [],
    createdAt: raw.created_at,
    updatedAt: raw.updated_at,
  }
}

export async function updateKnowledgeDocument(
  documentId: string,
  patch: { title?: string; content?: string; status?: 'active' | 'deprecated' },
) {
  const actor = await resolveChatActor()
  const response = await fetch(
    `${DOCUMENTS_SERVICE_URL}/v1/documents/${encodeURIComponent(documentId)}`,
    {
      method: 'PATCH',
      headers: buildInternalHeaders(actor),
      body: JSON.stringify({ org_id: actor.orgId, ...patch }),
    },
  )
  if (!response.ok) {
    const detail = await response.text().catch(() => '')
    throw new Error(detail || `Update failed: ${response.status}`)
  }
  // Return the canonical detail shape after update.
  return await getKnowledgeDocumentDetail(documentId)
}

export async function deleteKnowledgeDocument(documentId: string): Promise<void> {
  const actor = await resolveChatActor()
  const response = await fetch(
    `${DOCUMENTS_SERVICE_URL}/v1/documents/${encodeURIComponent(documentId)}?org_id=${encodeURIComponent(actor.orgId)}`,
    {
      method: 'DELETE',
      headers: buildInternalHeaders(actor),
    },
  )
  if (!response.ok && response.status !== 404) {
    throw new Error(`Failed to delete document: ${response.status}`)
  }
}

export async function reindexKnowledgeDocument(documentId: string): Promise<void> {
  const actor = await resolveChatActor()
  const response = await fetch(
    `${DOCUMENTS_SERVICE_URL}/v1/documents/${encodeURIComponent(documentId)}/reindex`,
    {
      method: 'POST',
      headers: buildInternalHeaders(actor),
      body: JSON.stringify({ org_id: actor.orgId }),
    },
  )
  if (!response.ok) {
    throw new Error(`Failed to reindex document: ${response.status}`)
  }
}

/**
 * Wave 11 Phase 1: a tiny env helper for the App Shell layout so it
 * knows whether to render the Lindy "Connect" tiles for integrations
 * we haven't built backends for yet. The 8-tile picker shows them
 * grayed out with a "coming soon" pill rather than as broken buttons.
 */
export const KNOWLEDGE_TILE_FEATURE_FLAGS = {
  files: true,
  text: true,
  website: true,
  qa: true,
  googleDrive: Boolean(process.env.INTEGRATION_GDRIVE_ENABLED),
  oneDrive: Boolean(process.env.INTEGRATION_ONEDRIVE_ENABLED),
  notion: Boolean(process.env.INTEGRATION_NOTION_ENABLED),
  slack: Boolean(process.env.INTEGRATION_SLACK_ENABLED),
} as const

export { ChatStoreError }
