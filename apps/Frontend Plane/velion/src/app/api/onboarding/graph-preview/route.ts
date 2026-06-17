/**
 * Phase 1 onboarding · live graph preview.
 *
 * The ConnectStep used to fabricate nodes client-side in a deterministic
 * loop. This route returns the *real* knowledge graph for the signed-in
 * org by reading the aggregate snapshot endpoint on `graph-index-rs`
 * (port 9203 inside the dpv2 network):
 *
 *   `GET /v1/graphs/{org_id}?limit_nodes=&limit_edges=`
 *
 * The shape is intentionally narrower than the dashboard's graph viewer
 * — onboarding only needs `{id, label, group}` per node and `{a, b}`
 * per edge. The ConnectStep polls this endpoint while the user is
 * picking sources so the canvas reveals new nodes as Data Plane
 * indexes them.
 *
 * Fallback policy: no synthetic sample graph. If graph-index is not
 * ready yet, we render only real integration connection metadata from
 * integration-core. If neither Data Plane nor integration-core has data,
 * the UI receives an empty graph and a warning.
 *
 * Connector state is read from integration-core when available. It is
 * not the source of graph facts; Data Plane owns graph facts, while
 * integration-core owns external account connection status.
 */

import { NextRequest, NextResponse } from 'next/server'

import { resolveActiveOrgContext } from '@/lib/server/active-org'

export const runtime = 'nodejs'
export const dynamic = 'force-dynamic'

const GRAPH_SERVICE_URL = (
  process.env.GRAPH_SERVICE_URL ||
  process.env.GRAPH_INDEX_URL ||
  process.env.DATA_GRAPH_API_URL ||
  'http://dpv2-graph-index:9203'
).replace(/\/+$/, '')

const INTEGRATION_CORE_URL = (
  process.env.INTEGRATION_CORE_URL ||
  'http://integration-api:3026'
).replace(/\/+$/, '')

const INTERNAL_API_KEY =
  process.env.INTERNAL_API_KEY || process.env.INTERNAL_SERVICE_SECRET || ''

const INTEGRATION_INTERNAL_API_KEY =
  process.env.AUTH_CORE_INTERNAL_API_KEY ||
  process.env.INTEGRATION_CORE_INTERNAL_API_KEY ||
  INTERNAL_API_KEY

const REQUEST_TIMEOUT_MS = 4_000
const DEFAULT_NODE_LIMIT = 60
const MAX_NODE_LIMIT = 200

const NETWORK_ERROR_PATTERN =
  /fetch failed|ECONNREFUSED|ENOTFOUND|ETIMEDOUT|connect|network|aborted|UND_ERR_SOCKET/i

interface PreviewNode {
  id: string
  label: string
  /**
   * Logical grouping the SVG layer uses for colour. Derived from the
   * upstream entity type (lowercased), with `org` reserved for the
   * anchor node.
   */
  group: string
}

interface PreviewEdge {
  a: string
  b: string
  type?: string
}

interface PreviewResponse {
  nodes: PreviewNode[]
  edges: PreviewEdge[]
  counts: {
    nodes: number
    edges: number
    /** How many of the user's connector groups contributed to the graph. */
    groups: number
  }
  /** Set when the graph is partial or still waiting on Data Plane. */
  warning?: string
}

interface UpstreamEntity {
  entity_id: string
  org_id: string
  entity_type?: string
  entity_text?: string
  type?: string
  text?: string
}

interface UpstreamRelationship {
  rel_id: string
  entity_a_id: string
  entity_b_id: string
  relation_type: string
}

interface UpstreamSnapshotResponse {
  org_id?: string
  nodes?: UpstreamEntity[]
  edges?: UpstreamRelationship[]
  node_count?: number
  edge_count?: number
  truncated?: boolean
  error?: string
}

interface IntegrationCoreConnection {
  id: string
  providerKey?: string
  providerLabel?: string
  provider?: string
  provider_key?: string
  provider_label?: string
  status?: string
  lastSyncStatus?: string | null
  last_sync_status?: string | null
  lastSyncSummary?: Record<string, unknown> | null
  last_sync_summary?: Record<string, unknown> | null
  nangoConnectionId?: string
  nango_connection_id?: string
}

interface IntegrationIntroItem {
  id: string
  kind: string
  label: string
  source: string
}

interface IntegrationConnectionsEnvelope {
  success?: boolean
  data?: {
    connections?: IntegrationCoreConnection[]
  }
  connections?: IntegrationCoreConnection[]
}

function emptyGraph(warning?: string): PreviewResponse {
  return {
    nodes: [],
    edges: [],
    counts: { nodes: 0, edges: 0, groups: 0 },
    ...(warning ? { warning } : {}),
  }
}

function connectionLabel(connection: IntegrationCoreConnection): string {
  return (
    connection.providerLabel ||
    connection.provider_label ||
    connectionProvider(connection)
  )
}

function connectionSelectedSources(connection: IntegrationCoreConnection): string[] {
  const summary = connection.lastSyncSummary ?? connection.last_sync_summary ?? null
  const selectedSources = summary?.selectedSources
  if (!Array.isArray(selectedSources)) return []
  return selectedSources
    .filter((value): value is string => typeof value === 'string')
    .map((value) => value.trim())
    .filter(Boolean)
}

function connectionIntroItems(connection: IntegrationCoreConnection): IntegrationIntroItem[] {
  const summary = connection.lastSyncSummary ?? connection.last_sync_summary ?? null
  const introItems = summary?.introItems
  if (!Array.isArray(introItems)) return []

  return introItems.flatMap((item) => {
    if (!item || typeof item !== 'object') return []
    const record = item as Record<string, unknown>
    const id = stringValue(record.id)
    const label = stringValue(record.label)
    if (!id || !label) return []
    return [{
      id,
      kind: stringValue(record.kind) || 'item',
      label,
      source: stringValue(record.source) || 'integration',
    }]
  })
}

function graphFromConnections(
  orgId: string,
  connections: IntegrationCoreConnection[],
): PreviewResponse {
  const active = connections.filter((connection) => connection.status !== 'deleted')
  if (active.length === 0) return emptyGraph()

  const nodes: PreviewNode[] = [
    { id: 'org', label: orgId.slice(0, 8) || 'Org', group: 'org' },
  ]
  const edges: PreviewEdge[] = []
  const groups = new Set<string>()

  active.forEach((connection, index) => {
    const provider = connectionProvider(connection)
    const connectionId =
      connection.id ||
      connection.nangoConnectionId ||
      connection.nango_connection_id ||
      `${provider}-${index}`
    const providerNodeId = `integration:${connectionId}`
    groups.add(provider)
    nodes.push({
      id: providerNodeId,
      label: connectionLabel(connection),
      group: 'integration',
    })
    edges.push({ a: 'org', b: providerNodeId, type: 'connected_to' })

    connectionSelectedSources(connection).forEach((source) => {
      const sourceNodeId = `${providerNodeId}:source:${source}`
      nodes.push({
        id: sourceNodeId,
        label: source.replace(/[_-]/g, ' '),
        group: 'source',
      })
      edges.push({ a: providerNodeId, b: sourceNodeId, type: 'selected_source' })
    })

    connectionIntroItems(connection).forEach((item) => {
      const itemNodeId = [
        providerNodeId,
        'intro',
        safeNodeIdPart(item.source),
        safeNodeIdPart(item.kind),
        safeNodeIdPart(item.id),
      ].join(':')
      nodes.push({
        id: itemNodeId,
        label: item.label,
        group: normaliseGroup(item.kind),
      })
      edges.push({ a: providerNodeId, b: itemNodeId, type: 'verified_intro' })
    })
  })

  return normalisePreviewResponse({
    nodes,
    edges,
    counts: { nodes: nodes.length, edges: edges.length, groups: groups.size },
  })
}

function fetchGraph(
  path: string,
  init: RequestInit,
  orgId: string,
): Promise<Response> {
  return fetch(`${GRAPH_SERVICE_URL}${path}`, {
    ...init,
    headers: {
      Accept: 'application/json',
      'Content-Type': 'application/json',
      'X-Internal-Api-Key': INTERNAL_API_KEY,
      'X-Org-ID': orgId,
      ...(init.headers ?? {}),
    },
    cache: 'no-store',
    signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
  })
}

function integrationHeaders(orgId: string): HeadersInit {
  return {
    Accept: 'application/json',
    'Content-Type': 'application/json',
    'X-Internal-Api-Key': INTEGRATION_INTERNAL_API_KEY,
    'X-Org-ID': orgId,
  }
}

function normaliseGroup(entityType: string): string {
  return entityType.toLowerCase().replace(/[^a-z0-9_-]/g, '_').slice(0, 32) ||
    'other'
}

function safeNodeIdPart(input: string): string {
  return input.replace(/[^A-Za-z0-9_-]/g, '_').slice(0, 64) || 'item'
}

function stringValue(value: unknown): string | undefined {
  return typeof value === 'string' && value.trim() ? value.trim() : undefined
}

function entityType(entity: UpstreamEntity): string {
  return entity.entity_type || entity.type || 'other'
}

function entityText(entity: UpstreamEntity): string {
  return entity.entity_text || entity.text || entity.entity_id.slice(0, 8)
}

function entityToNode(entity: UpstreamEntity): PreviewNode {
  return {
    id: entity.entity_id,
    label: entityText(entity),
    group: normaliseGroup(entityType(entity)),
  }
}

function relationshipToEdge(rel: UpstreamRelationship): PreviewEdge {
  return {
    a: rel.entity_a_id,
    b: rel.entity_b_id,
    type: rel.relation_type,
  }
}

function normalisePreviewResponse(response: PreviewResponse): PreviewResponse {
  const nodesById = new Map<string, PreviewNode>()
  for (const node of response.nodes) {
    if (!node.id || nodesById.has(node.id)) continue
    nodesById.set(node.id, node)
  }

  const edgesByKey = new Map<string, PreviewEdge>()
  for (const edge of response.edges) {
    if (!nodesById.has(edge.a) || !nodesById.has(edge.b)) continue
    const key = `${edge.a}\u0000${edge.b}\u0000${edge.type ?? ''}`
    if (edgesByKey.has(key)) continue
    edgesByKey.set(key, edge)
  }

  const nodes = Array.from(nodesById.values())
  const edges = Array.from(edgesByKey.values())
  const groupSet = new Set(
    nodes
      .filter((node) => node.group !== 'org')
      .map((node) => node.group),
  )

  return {
    nodes,
    edges,
    counts: {
      nodes: nodes.length,
      edges: edges.length,
      groups: groupSet.size,
    },
    ...(response.warning ? { warning: response.warning } : {}),
  }
}

async function loadGraphSnapshot(
  orgId: string,
  limit: number,
): Promise<{ entities: UpstreamEntity[]; relationships: UpstreamRelationship[] } | { error: string }> {
  try {
    const params = new URLSearchParams({
      limit_nodes: String(limit),
      limit_edges: String(limit * 3),
    })
    const response = await fetchGraph(
      `/v1/graphs/${encodeURIComponent(orgId)}?${params.toString()}`,
      { method: 'GET' },
      orgId,
    )
    if (!response.ok) {
      return { error: `Data Plane graph returned ${response.status}` }
    }
    const body = (await response.json()) as UpstreamSnapshotResponse
    if (body.error) return { error: body.error }
    return {
      entities: body.nodes ?? [],
      relationships: body.edges ?? [],
    }
  } catch (error) {
    const message = error instanceof Error ? error.message : 'network error'
    if (NETWORK_ERROR_PATTERN.test(message)) {
      return {
        error:
          'Data Plane graph preview is offline. Connected sources still appear once integration-core confirms them.',
      }
    }
    return { error: message }
  }
}

function connectionProvider(connection: IntegrationCoreConnection): string {
  return (
    connection.providerKey ||
    connection.provider_key ||
    connection.provider ||
    'unknown'
  )
}

async function loadIntegrationConnections(
  orgId: string,
): Promise<IntegrationCoreConnection[]> {
  if (!INTEGRATION_INTERNAL_API_KEY) return []
  try {
    const params = new URLSearchParams({ organizationId: orgId })
    const response = await fetch(
      `${INTEGRATION_CORE_URL}/api/v1/connections?${params.toString()}`,
      {
        method: 'GET',
        headers: integrationHeaders(orgId),
        cache: 'no-store',
        signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
      },
    )
    if (!response.ok) return []
    const body = (await response.json()) as IntegrationConnectionsEnvelope
    return body.data?.connections ?? body.connections ?? []
  } catch {
    return []
  }
}

export async function GET(request: NextRequest): Promise<NextResponse> {
  const actor = await resolveActiveOrgContext().catch(() => null)
  if (!actor?.orgId) {
    return NextResponse.json(
      emptyGraph('Sign in to preview connected knowledge.'),
      { status: 200, headers: { 'Cache-Control': 'no-store' } },
    )
  }

  const sp = request.nextUrl.searchParams
  const limit = Math.max(
    1,
    Math.min(Number(sp.get('limit') ?? DEFAULT_NODE_LIMIT), MAX_NODE_LIMIT),
  )

  const [graphResult, integrationConnections] = await Promise.all([
    loadGraphSnapshot(actor.orgId, limit),
    loadIntegrationConnections(actor.orgId),
  ])
  const connectionGraph = graphFromConnections(actor.orgId, integrationConnections)

  if ('error' in graphResult) {
    const payload =
      connectionGraph.nodes.length > 0
        ? { ...connectionGraph, warning: graphResult.error }
        : emptyGraph(graphResult.error)

    return NextResponse.json(
      payload,
      { status: 200, headers: { 'Cache-Control': 'no-store' } },
    )
  }

  if (graphResult.entities.length === 0) {
    const payload =
      connectionGraph.nodes.length > 0
        ? {
            ...connectionGraph,
            warning:
              'Data Plane graph extraction is pending — showing live connected sources until indexed entities arrive.',
          }
        : emptyGraph(
            'Ingen kunnskap er indeksert ennå. Koble til en kilde for å bygge grafen.',
          )

    return NextResponse.json(
      payload,
      { status: 200, headers: { 'Cache-Control': 'no-store' } },
    )
  }

  const nodes: PreviewNode[] = [
    { id: 'org', label: actor.orgId.slice(0, 8) || 'Org', group: 'org' },
    ...graphResult.entities.map(entityToNode),
  ]

  // Anchor every top-level entity to the org node so the canvas is
  // connected — the SVG layout falls apart on disconnected components.
  const orgAnchorEdges: PreviewEdge[] = graphResult.entities.map((e) => ({
    a: 'org',
    b: e.entity_id,
  }))
  const edges: PreviewEdge[] = [
    ...orgAnchorEdges,
    ...graphResult.relationships.map(relationshipToEdge),
  ]
  const existingNodeIds = new Set(nodes.map((node) => node.id))
  for (const node of connectionGraph.nodes) {
    if (node.id === 'org' || existingNodeIds.has(node.id)) continue
    nodes.push(node)
    existingNodeIds.add(node.id)
  }
  edges.push(...connectionGraph.edges)

  const payload: PreviewResponse = normalisePreviewResponse({
    nodes,
    edges,
    counts: {
      nodes: nodes.length,
      edges: edges.length,
      groups: 0,
    },
  })
  return NextResponse.json(payload, {
    status: 200,
    headers: { 'Cache-Control': 'no-store' },
  })
}
