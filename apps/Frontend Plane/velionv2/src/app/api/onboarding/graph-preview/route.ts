import { NextResponse, type NextRequest } from "next/server";

import { authErrorResponse, requireSession } from "@/app/api/_lib/control-plane-auth";
import {
  buildServiceHeaders,
  getGraphIndexUrl,
  resolveActiveOrgId,
} from "@/app/api/onboarding/_lib/onboarding-proxy";

export const dynamic = "force-dynamic";

interface GraphEntity {
  entity_id: string;
  entity_type?: string;
  entity_text?: string;
}

interface GraphRelationship {
  entity_a_id: string;
  entity_b_id: string;
}

interface GraphIndexResponse {
  nodes?: GraphEntity[];
  edges?: GraphRelationship[];
  node_count?: number;
  edge_count?: number;
}

const EMPTY = { nodes: [], edges: [], counts: { nodes: 0, edges: 0, groups: 0 } };

/**
 * GET /api/onboarding/graph-preview
 * Proxies graph-index-rs `GET /v1/graphs/{org_id}` and maps the entity/edge
 * shape to the wizard's preview shape. Degrades to an empty graph on any
 * failure so the polling loop never breaks.
 */
export async function GET(request: NextRequest) {
  try {
    const session = await requireSession(request);
    const orgId = await resolveActiveOrgId(request, session);
    if (!orgId) return NextResponse.json(EMPTY);

    const url = `${getGraphIndexUrl()}/v1/graphs/${encodeURIComponent(orgId)}?limit_nodes=2000&limit_edges=8000`;
    const response = await fetch(url, {
      method: "GET",
      headers: buildServiceHeaders(request, session, orgId),
      cache: "no-store",
    });
    if (!response.ok) return NextResponse.json(EMPTY);

    const body = (await response.json().catch(() => null)) as GraphIndexResponse | null;
    if (!body) return NextResponse.json(EMPTY);

    const nodes = (body.nodes ?? [])
      .filter((entity) => entity.entity_id)
      .map((entity) => ({
        id: entity.entity_id,
        label: entity.entity_text || entity.entity_id,
        group: (entity.entity_type || "entity").toLowerCase(),
      }));
    const nodeIds = new Set(nodes.map((node) => node.id));
    const edges = (body.edges ?? [])
      .filter((edge) => nodeIds.has(edge.entity_a_id) && nodeIds.has(edge.entity_b_id))
      .map((edge) => ({ a: edge.entity_a_id, b: edge.entity_b_id }));
    const groups = new Set(nodes.filter((node) => node.group !== "org").map((node) => node.group));

    return NextResponse.json({
      nodes,
      edges,
      counts: {
        nodes: body.node_count ?? nodes.length,
        edges: body.edge_count ?? edges.length,
        groups: groups.size,
      },
    });
  } catch (error) {
    // Auth failures should surface as 401; everything else degrades to empty.
    if (error && typeof error === "object" && "status" in error && (error as { status: number }).status === 401) {
      return authErrorResponse(error);
    }
    return NextResponse.json(EMPTY);
  }
}
