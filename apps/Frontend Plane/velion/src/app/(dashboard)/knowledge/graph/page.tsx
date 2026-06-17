import { notFound } from 'next/navigation';
import { GraphPaneClient } from '@/components/knowledge/GraphPaneClient';
import {
  EMPTY_GRAPH_VIEWER_SNAPSHOT,
  graphViewerSnapshotSchema,
  type GraphViewerSnapshot,
} from '@/types/data-plane/graph_v1';
import { KNOWLEDGE_FEATURE_FLAGS } from '@/lib/knowledge/feature-flags';
import { resolveActiveOrgContext } from '@/lib/server/active-org';

export const dynamic = 'force-dynamic';

const GRAPH_SERVICE_URL = (
  process.env.GRAPH_SERVICE_URL ||
  process.env.GRAPH_INDEX_URL ||
  process.env.DATA_GRAPH_API_URL ||
  'http://dpv2-graph-index:9203'
).replace(/\/+$/, '');
const INTERNAL_API_KEY =
  process.env.INTERNAL_API_KEY || process.env.INTERNAL_SERVICE_SECRET || '';

interface UpstreamEntity {
  entity_id: string;
  org_id: string;
  entity_type?: string;
  entity_text?: string;
  type?: string;
  text?: string;
  confidence?: number;
  provenance?: string;
  source_refs?: string[];
  created_at?: string;
  metadata?: Record<string, unknown>;
}

interface UpstreamRelationship {
  rel_id: string;
  org_id: string;
  entity_a_id: string;
  entity_b_id: string;
  relation_type: string;
  confidence?: number;
  provenance?: string;
  source_refs?: string[];
  created_at?: string;
  metadata?: Record<string, unknown>;
}

interface UpstreamSnapshotResponse {
  org_id?: string;
  nodes?: UpstreamEntity[];
  edges?: UpstreamRelationship[];
  node_count?: number;
  edge_count?: number;
  truncated?: boolean;
  error?: string;
}

function normalizeEntity(e: UpstreamEntity): GraphViewerSnapshot['entities'][number] {
  const type = e.type || e.entity_type || 'other';
  const text = e.text || e.entity_text || e.entity_id.slice(0, 8);

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
  };
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
  };
}

/**
 * Wave 11.C-a — server-side snapshot loader. Talks directly to
 * Data Plane v2 graph-index-rs so the page is SSR'd and the canvas
 * can render on first paint. The browser-side client refreshes via
 * `/api/knowledge/graph` on filter change or manual refresh.
 */
async function loadSnapshot(): Promise<GraphViewerSnapshot> {
  try {
    const actor = await resolveActiveOrgContext();
    if (!actor?.orgId) {
      return {
        ...EMPTY_GRAPH_VIEWER_SNAPSHOT,
        warning: 'Sign in to view your Data Plane graph.',
      };
    }

    const params = new URLSearchParams({
      limit_nodes: '200',
      limit_edges: '600',
    });
    const headers: HeadersInit = {
      Accept: 'application/json',
      'X-Internal-Api-Key': INTERNAL_API_KEY,
      'X-Org-ID': actor.orgId,
    };

    const graphResp = await fetch(`${GRAPH_SERVICE_URL}/v1/graphs/${encodeURIComponent(actor.orgId)}?${params.toString()}`, {
      headers,
      cache: 'no-store',
      signal: AbortSignal.timeout(15_000),
    });
    if (!graphResp.ok) {
      return {
        ...EMPTY_GRAPH_VIEWER_SNAPSHOT,
        warning: `Data Plane graph service returned ${graphResp.status}`,
      };
    }

    const graphData = (await graphResp.json()) as UpstreamSnapshotResponse;
    if (graphData.error) {
      return {
        ...EMPTY_GRAPH_VIEWER_SNAPSHOT,
        warning: `graph-index: ${graphData.error}`,
      };
    }

    const entitiesRaw = graphData.nodes ?? [];
    if (entitiesRaw.length === 0) return EMPTY_GRAPH_VIEWER_SNAPSHOT;

    const relationships = graphData.edges ?? [];
    const entities = entitiesRaw.map(normalizeEntity);

    const snapshot: GraphViewerSnapshot = {
      entities,
      relationships: relationships.map(normalizeRel),
      available_types: Array.from(new Set(entities.map((e) => e.type))).sort(),
    };
    const parsed = graphViewerSnapshotSchema.safeParse(snapshot);
    return parsed.success ? parsed.data : EMPTY_GRAPH_VIEWER_SNAPSHOT;
  } catch (error) {
    const message = error instanceof Error ? error.message : 'load failed';
    return {
      ...EMPTY_GRAPH_VIEWER_SNAPSHOT,
      warning: `Graph snapshot load failed: ${message}. Bring up Data Plane graph-index on port 9203.`,
    };
  }
}

export default async function KnowledgeGraphPage() {
  if (!KNOWLEDGE_FEATURE_FLAGS.graph) {
    notFound();
  }
  const snapshot = await loadSnapshot();
  return <GraphPaneClient snapshot={snapshot} />;
}
