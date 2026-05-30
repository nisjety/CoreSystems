import { NextRequest, NextResponse } from 'next/server'

import { ensureGraphEnabled } from '@/lib/knowledge/feature-flags'
import { resolveActiveOrgContext } from '@/lib/server/active-org'
import {
  graphEntitySchema,
  graphRelationshipSchema,
  type GraphEntity,
  type GraphRelationship,
  type GraphClaim,
} from '@/types/data-plane'

const GRAPH_SERVICE_URL = (
  process.env.GRAPH_SERVICE_URL ||
  process.env.GRAPH_INDEX_URL ||
  process.env.DATA_GRAPH_API_URL ||
  'http://dpv2-graph-index:9203'
).replace(/\/+$/, '')

const INTERNAL_API_KEY =
  process.env.INTERNAL_API_KEY || process.env.INTERNAL_SERVICE_SECRET || ''

interface RouteContext {
  params: Promise<{ entityId: string }>
}

interface EntityDetailResponse {
  entity: GraphEntity | null
  relationships: GraphRelationship[]
  claims: GraphClaim[]
  warning?: string
}

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

function normalizeEntityPayload(value: unknown): unknown {
  if (!value || typeof value !== 'object') return value
  const record = value as Record<string, unknown>
  return {
    ...record,
    type: record.type ?? record.entity_type,
    text: record.text ?? record.entity_text,
    confidence: record.confidence ?? 1,
    provenance: record.provenance ?? 'extracted',
    source_refs: Array.isArray(record.source_refs) ? record.source_refs : [],
  }
}

function normalizeRelationshipPayload(value: unknown): unknown {
  if (!value || typeof value !== 'object') return value
  const record = value as Record<string, unknown>
  return {
    ...record,
    confidence: record.confidence ?? 1,
    provenance: record.provenance ?? 'extracted',
    source_refs: Array.isArray(record.source_refs) ? record.source_refs : [],
  }
}

/**
 * Wave 11.C-a — entity detail drawer payload.
 *
 * Reads from the v2 graph-index aggregate snapshot:
 *   - GET /v1/graphs/{org_id}?limit_nodes=&limit_edges=
 *
 * The deployed graph-index service exposes graph nodes/edges through
 * that aggregate contract. Claim detail is not exposed there yet, so
 * the drawer returns an empty claims array until Data Plane publishes
 * a dedicated claim-detail endpoint.
 */
export async function GET(_request: NextRequest, context: RouteContext) {
  const gate = ensureGraphEnabled()
  if (gate !== true) {
    return NextResponse.json({ error: gate.error }, { status: gate.status })
  }

  try {
    const { entityId } = await context.params
    const actor = await resolveActiveOrgContext()
    if (!actor?.orgId) {
      return NextResponse.json({ error: 'No active organization' }, { status: 401 })
    }

    const headers: HeadersInit = {
      Accept: 'application/json',
      'X-Internal-Api-Key': INTERNAL_API_KEY,
      'X-Org-ID': actor.orgId,
    }
    const init: RequestInit = {
      method: 'GET',
      headers,
      cache: 'no-store',
      signal: AbortSignal.timeout(15_000),
    }

    const params = new URLSearchParams({
      limit_nodes: '500',
      limit_edges: '1500',
    })
    const response = await fetch(
      `${GRAPH_SERVICE_URL}/v1/graphs/${encodeURIComponent(actor.orgId)}?${params.toString()}`,
      init,
    )

    let entity: GraphEntity | null = null
    let relationships: GraphRelationship[] = []
    const claims: GraphClaim[] = []
    const warnings: string[] = []

    if (!response.ok) {
      warnings.push(`Data Plane graph service returned ${response.status}`)
    } else {
      const raw = (await response.json()) as UpstreamSnapshotResponse
      if (raw.error) warnings.push(`graph-index: ${raw.error}`)

      const entityRaw = (raw.nodes ?? []).find((node) => node.entity_id === entityId)
      const parsedEntity = graphEntitySchema.safeParse(
        normalizeEntityPayload(entityRaw),
      )
      if (parsedEntity.success) entity = parsedEntity.data

      relationships = (raw.edges ?? [])
        .filter((r) => r.entity_a_id === entityId || r.entity_b_id === entityId)
        .map((r) => graphRelationshipSchema.safeParse(normalizeRelationshipPayload(r)))
        .filter((p): p is { success: true; data: GraphRelationship } => p.success)
        .map((p) => p.data)

      if (raw.truncated && !entity) {
        warnings.push('Graph snapshot was truncated before this entity was returned')
      }
    }

    const payload: EntityDetailResponse = {
      entity,
      relationships,
      claims,
      ...(warnings.length ? { warning: warnings.join('; ') } : {}),
    }
    return NextResponse.json(payload, { headers: { 'Cache-Control': 'no-store' } })
  } catch (error) {
    return NextResponse.json(
      { error: error instanceof Error ? error.message : 'Entity detail load failed' },
      { status: 500 },
    )
  }
}
