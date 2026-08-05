/**
 * Wave 11.C-a — TypeScript mirror of `apps/Data Plane/proto/graph_v1.proto`.
 *
 * Hand-rolled from the proto so the verevon UI binds to a versioned
 * contract while we wait for proto-gen tooling. When `pnpm proto-gen`
 * lands, replace this file with the generated output — the public
 * shapes are identical.
 *
 * Naming convention: proto messages keep their snake_case field names
 * to match what the HTTP/gRPC layer emits. Zod parsers (below) are the
 * runtime safety net.
 */
import { z } from 'zod';

// ─── core ─────────────────────────────────────────────────────────────────

export const graphEntitySchema = z.object({
  entity_id: z.string(),
  org_id: z.string(),
  /** person | organization | location | product | concept | … */
  type: z.string(),
  text: z.string(),
  confidence: z.number().min(0).max(1),
  /** extracted | inferred | ambiguous */
  provenance: z.string(),
  source_refs: z.array(z.string()).default([]),
  created_at: z.string().optional(),
  metadata: z.record(z.string(), z.unknown()).optional(),
});
export type GraphEntity = z.infer<typeof graphEntitySchema>;

export const graphRelationshipSchema = z.object({
  rel_id: z.string(),
  org_id: z.string(),
  entity_a_id: z.string(),
  entity_b_id: z.string(),
  /** is_part_of | works_for | located_in | mentions | … */
  relation_type: z.string(),
  confidence: z.number().min(0).max(1),
  provenance: z.string(),
  source_refs: z.array(z.string()).default([]),
  created_at: z.string().optional(),
  metadata: z.record(z.string(), z.unknown()).optional(),
});
export type GraphRelationship = z.infer<typeof graphRelationshipSchema>;

export const graphClaimSchema = z.object({
  claim_id: z.string(),
  org_id: z.string(),
  text: z.string(),
  entity_ids: z.array(z.string()).default([]),
  confidence: z.number().min(0).max(1),
  provenance: z.string(),
  source_refs: z.array(z.string()).default([]),
  contradicted_by_claim_ids: z.array(z.string()).default([]),
  /** active | disputed | resolved */
  status: z.string(),
  created_at: z.string().optional(),
  metadata: z.record(z.string(), z.unknown()).optional(),
});
export type GraphClaim = z.infer<typeof graphClaimSchema>;

// ─── query envelopes ──────────────────────────────────────────────────────

export const listEntitiesByTypeResponseSchema = z.object({
  entities: z.array(graphEntitySchema).default([]),
  total: z.number().int().min(0).default(0),
});
export type ListEntitiesByTypeResponse = z.infer<typeof listEntitiesByTypeResponseSchema>;

export const getRelationshipsResponseSchema = z.object({
  relationships: z.array(graphRelationshipSchema).default([]),
});
export type GetRelationshipsResponse = z.infer<typeof getRelationshipsResponseSchema>;

export const getClaimsResponseSchema = z.object({
  claims: z.array(graphClaimSchema).default([]),
});
export type GetClaimsResponse = z.infer<typeof getClaimsResponseSchema>;

export const expandedGraphSchema = z.object({
  entities: z.array(graphEntitySchema).default([]),
  relationships: z.array(graphRelationshipSchema).default([]),
  claims: z.array(graphClaimSchema).default([]),
});
export type ExpandedGraph = z.infer<typeof expandedGraphSchema>;

export const graphExpansionResponseSchema = z.object({
  graph: expandedGraphSchema,
  hops_traversed: z.number().int().min(0).default(0),
  new_entities_found: z.number().int().min(0).default(0),
});
export type GraphExpansionResponse = z.infer<typeof graphExpansionResponseSchema>;

// ─── verevon UI-facing aggregate ───────────────────────────────────────────

/**
 * What the GraphRAG viewer hydrates from `/api/knowledge/graph`. The
 * route composes ListEntitiesByType + ExpandGraph into a single
 * payload the force-directed canvas can render.
 *
 * Named `GraphViewerSnapshot` to avoid colliding with `d4d5.GraphSnapshot`
 * (which is the upstream `/v1/graphs/{org_id}` shape using
 * `nodes`/`edges`). The UI shape uses `entities`/`relationships` to keep
 * the proto vocabulary; the route normalizes both upstream variants.
 */
export const graphViewerSnapshotSchema = z.object({
  entities: z.array(graphEntitySchema).default([]),
  relationships: z.array(graphRelationshipSchema).default([]),
  /** Available entity types (for filter chips). */
  available_types: z.array(z.string()).default([]),
  /** Optional banner shown when D4 services aren't fully online. */
  warning: z.string().optional(),
});
export type GraphViewerSnapshot = z.infer<typeof graphViewerSnapshotSchema>;

/**
 * Empty snapshot used when the feature flag is off OR the upstream is
 * unreachable. The viewer renders an empty-state instead of erroring.
 */
export const EMPTY_GRAPH_VIEWER_SNAPSHOT: GraphViewerSnapshot = {
  entities: [],
  relationships: [],
  available_types: [],
};
