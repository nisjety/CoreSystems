import { getSessionContext } from './auth-client'
import { requestJson } from './http'

export type LiveKnowledgeCollection = {
  count: number
  id: string
  label: string
}

export type KnowledgeMetricTone = 'good' | 'warn'

export type LiveKnowledgeMetric = {
  delta: string
  label: string
  tone: KnowledgeMetricTone
  value: string
}

export type LiveKnowledgeFolder = {
  connections: string[]
  id: string
  primaryLabel: string
  primaryValue: string
  providerKey: string
  secondaryLabel: string
  secondaryValue: string
  subtitle: string
  title: string
  tone: 'warm' | 'green' | 'blue' | 'gray'
}

export type LiveKnowledgeIntegration = {
  detail?: string
  documents: string
  freshness: string
  id: string
  name: string
  providerKey: string
  status: 'Connected' | 'Syncing' | 'Review'
}

export type LiveKnowledgeChunk = {
  id: string
  score: string
  text: string
  title: string
}

export type LiveKnowledgeSourceType = 'PDF' | 'Notion' | 'Docs' | 'URL'
export type LiveKnowledgeSourceStatus = 'Indexed' | 'Pending review' | 'Re-indexing'

export type LiveKnowledgeSource = {
  category: string
  chunks: number
  chunksPreview: LiveKnowledgeChunk[]
  coverage: string
  description: string
  hitRate: string
  id: string
  owner: string
  provider: string
  providerKey: string
  related: string[]
  similarity: string
  size: string
  status: LiveKnowledgeSourceStatus
  tags: string[]
  title: string
  type: LiveKnowledgeSourceType
  updated: string
  /**
   * Per-user ownership (the PR-2 documents columns retrieval + documents-api
   * filter on — the SAME authority, never a separate display flag). Optional:
   * when absent, no privacy badge renders (honest empty, not a fabricated state).
   */
  visibility?: 'private' | 'org' | 'shared'
  owner_id?: string
}

export type LiveKnowledgeFile = {
  addedBy: string
  id: string
  name: string
  providerKey: string
  source: string
  type: LiveKnowledgeSourceType
  updated: string
  /**
   * Origin URL for the document when the gateway workspace payload carries
   * one (build_files in apps/gateway .../knowledge/workspace.rs does not emit
   * it today). Optional so UI affordances that open the source render only
   * when a real URL exists — honest empty otherwise.
   */
  url?: string
}

export type LiveKnowledgeGraphNode = {
  group: string
  id: string
  label: string
  radius: number
  sourceIds: string[]
  sourceRefs: string[]
  tone: 'core' | 'support' | 'policy' | 'product' | 'risk'
  x: number
  y: number
}

export type LiveKnowledgeGraphLink = {
  from: string
  label: string
  sourceRefs: string[]
  strength: number
  to: string
}

export type LiveKnowledgeGraph = {
  available: boolean
  edgeCount: number
  groups: string[]
  links: LiveKnowledgeGraphLink[]
  nodeCount: number
  nodes: LiveKnowledgeGraphNode[]
  truncated: boolean
}

export type LiveKnowledgeFinspo = {
  available: boolean
  duplicateGroups: number
  inactiveCount: number
  largestCount: number
  reclaimableBytes: number
  recommendationCount: number
  sourceCount: number
}

export type LiveKnowledgeSyncMetrics = {
  connected: number
  failed: number
  syncing: number
}

export type LiveKnowledgeWebSource = {
  id: string
  kind: string
  name: string
  providerKey: string
  status: string
  updated: string
  url: string
}

export type LiveKnowledgeDiagnosticTone = 'bad' | 'good' | 'neutral' | 'warn'

export type LiveKnowledgeDiagnosticItem = {
  detail: string
  id: string
  label: string
  meta?: string
  status: string
  tone: LiveKnowledgeDiagnosticTone
}

export type LiveKnowledgeDiagnostics = {
  available: boolean
  capabilities: LiveKnowledgeDiagnosticItem[]
  quickwitIndexes: string[]
  services: LiveKnowledgeDiagnosticItem[]
  sparseBackend: string | null
  storage: LiveKnowledgeDiagnosticItem[]
  vectorCollections: string[]
}

export type LiveKnowledgePayload = {
  collections: LiveKnowledgeCollection[]
  dataPlane: {
    available: boolean
    documentCount: number
    documentsTruncated?: boolean
    indexedCount: number
    loadedDocumentCount?: number
  }
  diagnostics?: LiveKnowledgeDiagnostics | null
  files: LiveKnowledgeFile[]
  finspo: LiveKnowledgeFinspo
  folders: LiveKnowledgeFolder[]
  generatedAt: string
  graph: LiveKnowledgeGraph
  integrations: LiveKnowledgeIntegration[]
  metricCards: LiveKnowledgeMetric[]
  metrics: LiveKnowledgeSyncMetrics
  orgId: string | null
  sources: LiveKnowledgeSource[]
  webSources: LiveKnowledgeWebSource[]
}

export type KnowledgeSidebarFolderNode = {
  children?: KnowledgeSidebarFolderNode[]
  count: number
  id: string
  label: string
  providerKey: string
  sourceIds?: string[]
}

export type KnowledgeSidebarTag = {
  count: number
  label: string
}

export type KnowledgeSidebarLiveData = {
  folders: KnowledgeSidebarFolderNode[]
  sources: LiveKnowledgeSource[]
  tags: KnowledgeSidebarTag[]
}

/**
 * Load the aggregated knowledge workspace. The gateway scopes the payload by the
 * `x-verevon-org-id` header (the active org), matching every other v3 client. When
 * the caller already has the org id (e.g. KnowledgePage) it passes it through;
 * otherwise we resolve it from the session context so callers like the sidebar
 * panel stay one-liners.
 */
export async function loadKnowledgeSources(
  signal?: AbortSignal,
  orgId?: string,
): Promise<LiveKnowledgePayload> {
  let resolvedOrgId = orgId
  if (!resolvedOrgId) {
    const ctx = await getSessionContext().catch(() => null)
    resolvedOrgId = ctx?.orgs?.[0]?.id ?? ''
  }
  const headers = resolvedOrgId ? { 'x-verevon-org-id': resolvedOrgId } : undefined
  return requestJson<LiveKnowledgePayload>('/api/v1/knowledge/sources', { signal, headers })
}

export function buildKnowledgeSidebarLiveData(payload: LiveKnowledgePayload): KnowledgeSidebarLiveData {
  const collections = payload.collections.filter((collection) => collection.id !== 'all')
  const collectionNodes = collections.map((collection) => buildCollectionNode(collection, payload))
  const allCollection = payload.collections.find((collection) => collection.id === 'all')

  return {
    folders: [
      buildGeneralKnowledgeNode(payload, allCollection, collectionNodes),
      buildRagOperationsNode(payload),
    ],
    sources: payload.sources,
    tags: buildKnowledgeTags(payload.sources),
  }
}

export function filterKnowledgeFolders(
  folders: readonly KnowledgeSidebarFolderNode[],
  normalizedSearch: string,
): readonly KnowledgeSidebarFolderNode[] {
  if (!normalizedSearch) return folders

  return folders.reduce<KnowledgeSidebarFolderNode[]>((matches, folder) => {
    const children = folder.children ? [...filterKnowledgeFolders(folder.children, normalizedSearch)] : []
    const folderMatches = folder.label.toLowerCase().includes(normalizedSearch)

    if (!folderMatches && children.length === 0) return matches

    return [
      ...matches,
      {
        ...folder,
        children: children.length ? children : folder.children ? [...folder.children] : undefined,
      },
    ]
  }, [])
}

export function filterKnowledgeSources(
  sources: readonly LiveKnowledgeSource[],
  normalizedSearch: string,
): LiveKnowledgeSource[] {
  return sources.filter((source) => (
    !normalizedSearch ||
    source.title.toLowerCase().includes(normalizedSearch) ||
    source.type.toLowerCase().includes(normalizedSearch) ||
    source.category.toLowerCase().includes(normalizedSearch) ||
    source.tags.some((tag) => tag.toLowerCase().includes(normalizedSearch))
  ))
}

export function getVisibleKnowledgeTags(
  sources: readonly LiveKnowledgeSource[],
  normalizedSearch: string,
): KnowledgeSidebarTag[] {
  return buildKnowledgeTags(sources).filter((tag) => (
    !normalizedSearch || tag.label.toLowerCase().includes(normalizedSearch)
  ))
}

export function getKnowledgeFolderSourceIds(
  folders: readonly KnowledgeSidebarFolderNode[],
  folderId: string,
): string[] | null {
  for (const folder of folders) {
    if (folder.id === folderId) return folder.sourceIds ?? null

    const childMatch = getKnowledgeFolderSourceIds(folder.children ?? [], folderId)
    if (childMatch !== null) return childMatch
  }

  return null
}

function buildGeneralKnowledgeNode(
  payload: LiveKnowledgePayload,
  allCollection: LiveKnowledgeCollection | undefined,
  collectionNodes: KnowledgeSidebarFolderNode[],
): KnowledgeSidebarFolderNode {
  const onboardingSources = payload.sources.filter(matchesOnboardingSource)
  const documentSources = payload.sources.filter((source) => !matchesOnboardingSource(source))
  const integrationChildren = collectionNodes.length > 0 ? collectionNodes : buildFallbackNodes(payload.sources)

  return {
    id: allCollection?.id ?? 'all',
    label: allCollection?.label ?? 'General Knowledge',
    count: allCollection?.count ?? payload.sources.length + payload.webSources.length,
    providerKey: 'all',
    sourceIds: payload.sources.map((source) => source.id),
    children: [
      {
        id: 'general:onboarding',
        label: 'Onboarding',
        count: onboardingSources.length,
        providerKey: 'onboarding',
        sourceIds: onboardingSources.map((source) => source.id),
        children: onboardingSources.slice(0, 6).map((source) => buildSourceLeafNode(source, 'onboarding', 'general:onboarding')),
      },
      {
        id: 'general:integrations',
        label: 'Integrations',
        count: integrationChildren.reduce((sum, child) => sum + child.count, 0),
        providerKey: 'integrations',
        sourceIds: uniqueStringIds(integrationChildren.flatMap((child) => child.sourceIds ?? [])),
        children: integrationChildren,
      },
      {
        id: 'general:documents',
        label: 'Documents',
        count: documentSources.length,
        providerKey: 'documents',
        sourceIds: documentSources.map((source) => source.id),
        children: documentSources.slice(0, 6).map((source) => buildSourceLeafNode(source, 'documents', 'general:documents')),
      },
    ],
  }
}

function buildCollectionNode(
  collection: LiveKnowledgeCollection,
  payload: LiveKnowledgePayload,
): KnowledgeSidebarFolderNode {
  const providerKey = providerKeyFromCollectionId(collection.id)
  const sourceIds = payload.sources
    .filter((source) => source.providerKey === providerKey || (providerKey === 'web' && source.type === 'URL'))
    .map((source) => source.id)

  if (providerKey === 'web') {
    return {
      id: collection.id,
      label: collection.label,
      count: collection.count,
      providerKey,
      sourceIds,
      children: payload.webSources.map((source) => ({
        id: `web:${source.id}`,
        label: source.name,
        count: 1,
        providerKey: 'web',
        sourceIds,
      })),
    }
  }

  return {
    id: collection.id,
    label: collection.label,
    count: collection.count,
    providerKey,
    sourceIds,
    children: payload.sources
      .filter((source) => source.providerKey === providerKey)
      .slice(0, 6)
      .map((source) => buildSourceLeafNode(source, providerKey, collection.id)),
  }
}

function buildFallbackNodes(sources: readonly LiveKnowledgeSource[]): KnowledgeSidebarFolderNode[] {
  const byProvider = new Map<string, LiveKnowledgeSource[]>()

  for (const source of sources) {
    const current = byProvider.get(source.providerKey) ?? []
    byProvider.set(source.providerKey, [...current, source])
  }

  return Array.from(byProvider.entries()).map(([providerKey, providerSources]) => ({
    id: `provider:${providerKey}`,
    label: providerSources[0]?.provider || providerKey,
    count: providerSources.length,
    providerKey,
    sourceIds: providerSources.map((source) => source.id),
    children: providerSources.slice(0, 6).map((source) => buildSourceLeafNode(source, providerKey, `provider:${providerKey}`)),
  }))
}

function buildKnowledgeTags(sources: readonly LiveKnowledgeSource[]): KnowledgeSidebarTag[] {
  const tagCounts = new Map<string, number>()

  for (const source of sources) {
    for (const tag of source.tags) {
      tagCounts.set(tag, (tagCounts.get(tag) ?? 0) + 1)
    }
  }

  return Array.from(tagCounts, ([label, count]) => ({ label, count }))
    .sort((left, right) => right.count - left.count || left.label.localeCompare(right.label))
}

function buildRagOperationsNode(payload: LiveKnowledgePayload): KnowledgeSidebarFolderNode {
  const chunkQualitySources = payload.sources.filter((source) => source.chunks > 0)
  const retrievalEvalSources = payload.sources.filter((source) => (
    source.hitRate.length > 0 ||
    source.coverage.length > 0 ||
    source.similarity.length > 0
  ))

  return {
    id: 'rag',
    label: 'RAG Operations',
    count: Math.max(chunkQualitySources.length, retrievalEvalSources.length),
    providerKey: 'rag',
    sourceIds: uniqueStringIds([
      ...chunkQualitySources.map((source) => source.id),
      ...retrievalEvalSources.map((source) => source.id),
    ]),
    children: [
      {
        id: 'rag:chunk-quality',
        label: 'Chunk quality',
        count: chunkQualitySources.length,
        providerKey: 'rag',
        sourceIds: chunkQualitySources.map((source) => source.id),
      },
      {
        id: 'rag:retrieval-evals',
        label: 'Retrieval evals',
        count: retrievalEvalSources.length,
        providerKey: 'rag',
        sourceIds: retrievalEvalSources.map((source) => source.id),
      },
    ],
  }
}

function buildSourceLeafNode(
  source: LiveKnowledgeSource,
  providerKey: string,
  parentId: string,
): KnowledgeSidebarFolderNode {
  return {
    id: `${parentId}:${source.id}`,
    label: source.title,
    count: source.chunks,
    providerKey,
    sourceIds: [source.id],
  }
}

function matchesOnboardingSource(source: LiveKnowledgeSource) {
  const searchable = [
    source.title,
    source.description,
    source.category,
    ...source.tags,
    ...source.related,
  ].join(' ').toLowerCase()

  return /(onboard|setup|activation|implement|launch|handoff|get started|getting started)/.test(searchable)
}

function uniqueStringIds(ids: readonly string[]) {
  return Array.from(new Set(ids.filter(Boolean)))
}

function providerKeyFromCollectionId(collectionId: string) {
  if (collectionId.startsWith('provider:')) return collectionId.slice('provider:'.length)
  return collectionId
}
