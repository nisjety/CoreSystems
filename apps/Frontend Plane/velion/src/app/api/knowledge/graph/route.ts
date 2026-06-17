import { NextRequest, NextResponse } from 'next/server'

import { resolveChatActor, ChatStoreError } from '@/app/api/chat/_lib/session-store'
import { ensureGraphEnabled } from '@/lib/knowledge/feature-flags'
import {
  EMPTY_GRAPH_VIEWER_SNAPSHOT,
  graphViewerSnapshotSchema,
  type GraphViewerSnapshot,
} from '@/types/data-plane/graph_v1'

/**
 * Wave 11.C-a — GraphRAG snapshot proxy against Data Plane v2.
 *
 * Reads the aggregate snapshot from Data Plane v2 `graph-index-rs`:
 *
 *   `GET /v1/graphs/{org_id}?limit_nodes=&limit_edges=`
 *
 * Defaults assume the v2 compose deploy: `dpv2-graph-index:9203` inside
 * the docker network, override with `GRAPH_SERVICE_URL` for local.
 */

const GRAPH_SERVICE_URL =
  process.env.GRAPH_SERVICE_URL ||
  process.env.GRAPH_INDEX_URL ||
  process.env.DATA_GRAPH_API_URL ||
  'http://dpv2-graph-index:9203'

const INTERNAL_API_KEY =
  process.env.INTERNAL_API_KEY || process.env.INTERNAL_SERVICE_SECRET || ''

const NETWORK_ERROR =
  /fetch failed|ECONNREFUSED|ENOTFOUND|ETIMEDOUT|connect|network|aborted|UND_ERR_SOCKET/i

interface UpstreamEntity {
  entity_id: string
  org_id: string
  entity_type?: string
  entity_text?: string
  type?: string
  text?: string
  confidence?: number
  provenance?: string
  source_refs?: string[]
  created_at?: string
  metadata?: Record<string, unknown>
}

interface UpstreamRelationship {
  rel_id: string
  org_id: string
  entity_a_id: string
  entity_b_id: string
  relation_type: string
  confidence?: number
  provenance?: string
  source_refs?: string[]
  created_at?: string
  metadata?: Record<string, unknown>
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

function emptyWithWarning(message: string, status = 200): NextResponse {
  const body: GraphViewerSnapshot = { ...EMPTY_GRAPH_VIEWER_SNAPSHOT, warning: message }
  return NextResponse.json(body, {
    status,
    headers: { 'Cache-Control': 'no-store' },
  })
}

async function fetchGraph(path: string, init: RequestInit, orgId: string): Promise<Response> {
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
    signal: AbortSignal.timeout(15_000),
  })
}

function normalizeEntity(e: UpstreamEntity): GraphViewerSnapshot['entities'][number] {
  const type = e.type || e.entity_type || 'other'
  const text = e.text || e.entity_text || e.entity_id.slice(0, 8)

  return {
    entity_id: e.entity_id,
    org_id: e.org_id,
    type,
    text,
    confidence: e.confidence ?? 1,
    provenance: e.provenance ?? 'extracted',
    source_refs: e.source_refs ?? [],
    created_at: e.created_at,
    metadata: e.metadata,
  }
}

function normalizeRel(r: UpstreamRelationship): GraphViewerSnapshot['relationships'][number] {
  return {
    rel_id: r.rel_id,
    org_id: r.org_id,
    entity_a_id: r.entity_a_id,
    entity_b_id: r.entity_b_id,
    relation_type: r.relation_type,
    confidence: r.confidence ?? 1,
    provenance: r.provenance ?? 'extracted',
    source_refs: r.source_refs ?? [],
    created_at: r.created_at,
    metadata: r.metadata,
  }
}

export async function GET(request: NextRequest) {
  const gate = ensureGraphEnabled()
  if (gate !== true) {
    return NextResponse.json({ error: gate.error }, { status: gate.status })
  }

  try {
    const actor = await resolveChatActor()
    const sp = request.nextUrl.searchParams
    const limit = Math.min(Number(sp.get('limit') ?? '200'), 500)
    const entityType = sp.get('type') ?? ''

    const snapshotParams = new URLSearchParams({
      limit_nodes: String(limit),
      limit_edges: String(limit * 3),
    })

    let graphResp: Response
    try {
      graphResp = await fetchGraph(
        `/v1/graphs/${encodeURIComponent(actor.orgId)}?${snapshotParams.toString()}`,
        { method: 'GET' },
        actor.orgId,
      )
    } catch (error) {
      const message = error instanceof Error ? error.message : ''
      if (NETWORK_ERROR.test(message)) {
        return emptyWithWarning(
          'Data Plane graph service is not reachable. Bring up graph-index on port 9203 to see live data.',
        )
      }
      throw error
    }

    if (!graphResp.ok) {
      const detail = await graphResp.text().catch(() => '')
      return emptyWithWarning(
        `Data Plane graph service returned ${graphResp.status}${detail ? `: ${detail.slice(0, 200)}` : ''}`,
      )
    }

    const graphData = (await graphResp.json()) as UpstreamSnapshotResponse
    if (graphData.error) return emptyWithWarning(`graph-index: ${graphData.error}`)

    const entitiesRaw = (graphData.nodes ?? []).filter((entity) => {
      if (!entityType) return true
      const type = entity.type || entity.entity_type || ''
      return type === entityType
    })
    const relationshipsRaw = graphData.edges ?? []
    const visibleEntityIds = new Set(entitiesRaw.map((entity) => entity.entity_id))
    const relationships = relationshipsRaw
      .filter(
        (relationship) =>
          visibleEntityIds.has(relationship.entity_a_id) &&
          visibleEntityIds.has(relationship.entity_b_id),
      )
      .map(normalizeRel)

    if (entitiesRaw.length === 0) {
      return NextResponse.json(
        {
          entities: [],
          relationships: [],
          available_types: [],
        } satisfies GraphViewerSnapshot,
        { headers: { 'Cache-Control': 'no-store' } },
      )
    }

    const entities = entitiesRaw.map(normalizeEntity)
    const available_types = Array.from(new Set(entities.map((e) => e.type))).sort()

    const snapshot: GraphViewerSnapshot = { entities, relationships, available_types }
    const parsed = graphViewerSnapshotSchema.safeParse(snapshot)
    if (!parsed.success) {
      return emptyWithWarning('Graph payload failed schema validation — empty view shown to avoid stale data.')
    }
    return NextResponse.json(parsed.data, { headers: { 'Cache-Control': 'no-store' } })
  } catch (error) {
    if (error instanceof ChatStoreError) {
      return NextResponse.json({ error: error.message }, { status: error.statusCode })
    }
    return NextResponse.json(
      { error: error instanceof Error ? error.message : 'Graph load failed' },
      { status: 500 },
    )
  }
}
