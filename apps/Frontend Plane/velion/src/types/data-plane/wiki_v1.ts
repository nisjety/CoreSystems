/**
 * Wave 11.C-a — TypeScript mirror of `apps/Data Plane/proto/wiki_v1.proto`.
 *
 * Hand-rolled from the proto; replace with proto-gen output when
 * tooling lands. Field names stay snake_case to match the over-the-wire
 * shape.
 */
import { z } from 'zod';

// ─── core ─────────────────────────────────────────────────────────────────

export const wikiPageSchema = z.object({
  page_id: z.string(),
  org_id: z.string(),
  workspace_id: z.string(),
  title: z.string(),
  /** "/engineering/onboarding" | "/policies/data-classification" */
  path: z.string(),
  current_version_id: z.string(),
  /** draft | published | deprecated */
  status: z.string(),
  /** page_ids that reference this page. */
  backlinks: z.array(z.string()).default([]),
  created_at: z.string().optional(),
  updated_at: z.string().optional(),
  metadata: z.record(z.string(), z.unknown()).optional(),
});
export type WikiPage = z.infer<typeof wikiPageSchema>;

export const wikiPageVersionSchema = z.object({
  version_id: z.string(),
  page_id: z.string(),
  /** Logseq-format block-outline markdown. */
  content: z.string(),
  source_refs: z.array(z.string()).default([]),
  proposed_by_agent: z.string().optional().nullable(),
  proposed_by_user: z.string().optional().nullable(),
  approved_by: z.string().optional().nullable(),
  /** initial_synthesis | manual_edit | refresh | correction */
  edit_reason: z.string(),
  /** draft | approved | published */
  status: z.string(),
  created_at: z.string().optional(),
  published_at: z.string().optional().nullable(),
  metadata: z.record(z.string(), z.unknown()).optional(),
});
export type WikiPageVersion = z.infer<typeof wikiPageVersionSchema>;

export const wikiSourceLogSchema = z.object({
  log_id: z.string(),
  page_id: z.string(),
  original_chunks: z.array(z.string()).default([]),
  processing_model: z.string(),
  synthesis_prompt_hash: z.string(),
  created_at: z.string().optional(),
  metadata: z.record(z.string(), z.unknown()).optional(),
});
export type WikiSourceLog = z.infer<typeof wikiSourceLogSchema>;

export const wikiMaintenanceLogSchema = z.object({
  log_id: z.string(),
  page_id: z.string(),
  /** stale_content | missing_section | broken_reference | conflicting_info */
  issue_type: z.string(),
  details: z.record(z.string(), z.unknown()).optional(),
  proposed_fix: z.string().optional().nullable(),
  /** open | in_progress | resolved | ignored */
  status: z.string(),
  created_at: z.string().optional(),
  resolved_at: z.string().optional().nullable(),
  metadata: z.record(z.string(), z.unknown()).optional(),
});
export type WikiMaintenanceLog = z.infer<typeof wikiMaintenanceLogSchema>;

// ─── query envelopes ──────────────────────────────────────────────────────

export const getPageResponseSchema = z.object({
  page: wikiPageSchema,
  version: wikiPageVersionSchema,
});
export type GetPageResponse = z.infer<typeof getPageResponseSchema>;

export const listPageVersionsResponseSchema = z.object({
  versions: z.array(wikiPageVersionSchema).default([]),
  total: z.number().int().min(0).default(0),
});
export type ListPageVersionsResponse = z.infer<typeof listPageVersionsResponseSchema>;

export const getPageSourcesResponseSchema = z.object({
  source_log: wikiSourceLogSchema.optional().nullable(),
});
export type GetPageSourcesResponse = z.infer<typeof getPageSourcesResponseSchema>;

export const listMaintenanceIssuesResponseSchema = z.object({
  issues: z.array(wikiMaintenanceLogSchema).default([]),
  total: z.number().int().min(0).default(0),
});
export type ListMaintenanceIssuesResponse = z.infer<typeof listMaintenanceIssuesResponseSchema>;

// ─── velion UI-facing aggregates ──────────────────────────────────────────

/**
 * Light row shape used by the wiki page-tree sidebar.
 */
export const wikiPageRowSchema = z.object({
  page_id: z.string(),
  title: z.string(),
  path: z.string(),
  status: z.string(),
  updated_at: z.string().optional(),
});
export type WikiPageRow = z.infer<typeof wikiPageRowSchema>;

export const wikiListResponseSchema = z.object({
  pages: z.array(wikiPageRowSchema).default([]),
  warning: z.string().optional(),
});
export type WikiListResponse = z.infer<typeof wikiListResponseSchema>;

/**
 * What `/api/knowledge/wiki/[id]` returns — the page + its current
 * version + the source log + backlinks, in one round-trip so the
 * editor can render the right panel without a waterfall.
 */
export const wikiPageDetailSchema = z.object({
  page: wikiPageSchema,
  version: wikiPageVersionSchema,
  source_log: wikiSourceLogSchema.optional().nullable(),
  /** page_ids resolved to (title, path) for the Backlinks panel. */
  backlinks_resolved: z
    .array(
      z.object({
        page_id: z.string(),
        title: z.string(),
        path: z.string(),
      }),
    )
    .default([]),
  warning: z.string().optional(),
});
export type WikiPageDetail = z.infer<typeof wikiPageDetailSchema>;
