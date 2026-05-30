/**
 * Data Plane v2 — D4/D5 Knowledge Surface (v2.3.0).
 *
 * Generated against `apps/Data Plane v2/openapi/d4d5.yaml`. When the OpenAPI
 * code-generation pipeline lands (openapi-typescript), replace this file
 * with the generated output — the shapes here are the authoritative TS
 * mirror of the YAML schemas.
 *
 * Naming convention: PascalCase for types, snake_case for fields (matches
 * Data Plane v2 JSON-on-the-wire shape exactly; no rewriting).
 */

// ─── Common ────────────────────────────────────────────────────────────
export interface DPError {
  error: string;
}

export type ZdrMode = 'disabled' | 'reject' | 'ephemeral';
export type ZdrClassification = '' | 'internal' | 'public' | 'sensitive' | 'restricted';
export type ContextFormat = 'json' | 'markdown' | 'text';

// ─── Retrieval ─────────────────────────────────────────────────────────
export interface RetrievalFilters {
  document_types?: string[];
  departments?: string[];
  languages?: string[];
  document_ids?: string[]; // ≤ 1000
  sources?: string[];
  region?: string | null;
  workspaces?: string[];
  collections?: string[];
  acl_tags?: string[];
}

/** Per-query blend weights (D4+D5 spec §7). Unset weights inherit config defaults. */
export interface ModeMixWeights {
  w_dense?: number;
  w_bm25?: number;
  w_graph?: number;
  w_wiki?: number;
  rerank?: boolean;
}

export interface RetrievalRequest {
  org_id: string;
  query: string;
  top_k?: number;
  top_n?: number;
  filters?: RetrievalFilters;
  user_id?: string | null;
  query_expansion?: string | null;
  reranker_model?: string | null;
  zdr_mode?: ZdrMode;
  context_budget_tokens?: number;
  context_format?: ContextFormat;
  mode_mix?: ModeMixWeights;
}

export interface ScoredCandidate {
  knowledge_id: string;
  document_id: string;
  text: string;
  dense_score?: number;
  sparse_score?: number;
  rerank_score?: number;
  final_score: number;
  chunk_index?: number;
}

export interface SourceRef {
  document_id: string;
  title: string;
  source?: string;
  type?: string;
}

export interface ContextFact {
  knowledge_id?: string;
  document_id?: string;
  text?: string;
  score?: number;
  source_title?: string;
  source_type?: string;
  estimated_tokens?: number;
}

export interface ContextPack {
  facts: ContextFact[];
  total_tokens: number;
  budget_tokens: number;
  format: string;
}

export interface RetrievalResponse {
  candidates: ScoredCandidate[];
  sources: SourceRef[];
  query: string;
  org_id: string;
  trace_id: string;
  index_version?: string;
  zdr_mode: string;
  low_confidence?: boolean;
  context_pack?: ContextPack;
  /** Agent retrieval planner hints (v2.3 wave 2). */
  suggested_next_tools?: string[];
}

// ─── Documents ─────────────────────────────────────────────────────────
export type DocumentStatus = 'pending' | 'indexed' | 'failed';

export interface DPDocument {
  document_id: string;
  org_id: string;
  source: string;
  type: string;
  title: string;
  content: string;
  status: DocumentStatus;
  metadata?: Record<string, unknown>;
  zdr_classification: ZdrClassification;
  idempotency_key?: string | null;
  created_at: string;
  updated_at: string;
  deleted_at?: string | null;
}

export interface CreateDocumentRequest {
  source: string;
  type: string;
  title: string;
  /** ≤ 5 MiB. */
  content: string;
  metadata?: Record<string, unknown>;
  zdr_classification?: ZdrClassification;
  idempotency_key?: string;
}

export interface BulkIngestRequest {
  /** ≤ 500 items. */
  documents: CreateDocumentRequest[];
}

export interface BulkIngestResponse {
  accepted: number;
  rejected: number;
  reused: number;
  document_ids: string[];
  rejection_reasons?: string[];
}

// ─── Wiki ──────────────────────────────────────────────────────────────
export type WikiPageStatus = 'draft' | 'published' | 'deprecated';

export interface WikiPage {
  page_id: string;
  org_id: string;
  workspace_id?: string | null;
  title: string;
  path: string;
  current_version_id?: string | null;
  page_status: WikiPageStatus;
  backlinks?: unknown[];
  created_at: string;
  updated_at: string;
}

export interface WikiPageVersion {
  version_id: string;
  page_id: string;
  content?: string | null;
  source_refs?: unknown[];
  proposed_by_agent?: string | null;
  proposed_by_user?: string | null;
  approved_by?: string | null;
  edit_reason?: string | null;
}

export type WikiDiffHunkOp = 'add' | 'remove' | 'equal';

export interface WikiDiffHunk {
  op: WikiDiffHunkOp;
  lines: string[];
}

export interface WikiDiff {
  page_id: string;
  from_version_id: string;
  to_version_id: string;
  hunks: WikiDiffHunk[];
  added_lines: number;
  removed_lines: number;
  unchanged_lines: number;
}

export type MaintenanceKind =
  | 'orphan_wiki'
  | 'stale_wiki'
  | 'weak_citation'
  | 'contradiction'
  | 'stale'
  | 'orphan';

export interface MaintenanceSweepItem {
  page_id?: string;
  kind: MaintenanceKind;
  actor?: string;
  details?: Record<string, unknown>;
}

export interface MaintenanceSweepRequest {
  /** ≤ 1000 items. */
  items: MaintenanceSweepItem[];
}

// ─── Graph (D4+D5 spec §3.1) ───────────────────────────────────────────
export type GraphEdgeProvenance = 'extracted' | 'inferred' | 'ambiguous';

export interface GraphNode {
  entity_id: string;
  entity_type: string;
  entity_text: string;
  confidence?: number;
  community_id?: string | null;
}

export interface GraphEdge {
  rel_id: string;
  entity_a_id: string;
  entity_b_id: string;
  relation_type: string;
  confidence?: number;
  provenance?: GraphEdgeProvenance;
}

/** Spec §3.1 — `GET /v1/graphs/{org_id}` aggregate snapshot. */
export interface GraphSnapshot {
  org_id: string;
  nodes: GraphNode[];
  edges: GraphEdge[];
  node_count: number;
  edge_count: number;
}

// ─── Service base URLs (container DNS on aquatiq-local) ────────────────
export const DataPlaneURLs = {
  retrieval: 'http://retrieval-engine:8004',
  documents: 'http://documents-api:8010',
  wiki: 'http://wiki-store:8011',
  orchestrator: 'http://data-orchestrator:8012',
  quality: 'http://data-quality:8013',
  graph: 'http://graph-index:9203',
} as const;
