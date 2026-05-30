/**
 * Wave 11: shared shapes used across the /knowledge surface.
 * Server fetchers normalize backend payloads into these — UI components
 * never see raw documents-service / integration-engine / Quarry shapes.
 */

export type KnowledgeMode = 'files' | 'text' | 'website' | 'qa' | 'integrations';

export interface KnowledgeStats {
  totalDocuments: number;
  totalSources: number;
  totalSizeBytes: number;
  /** Bytes ceiling for the org's plan. 0 = unlimited. */
  storageQuotaBytes: number;
  /**
   * True when the org has unsaved knowledge changes that agents haven't
   * been re-indexed against yet. Mirrors Chatbase's "Retraining required"
   * chip. Cleared when the orchestrator finishes a reindex sweep.
   */
  dirty: boolean;
  /** Per-mode breakdown for the right-rail mini-stats. */
  byMode: Record<KnowledgeMode, number>;
  /**
   * Phase 4 — onboarding auto-ingest banner. Set when one or more docs
   * are still pending/processing/indexing. The shell renders a sticky
   * top banner ("Your content is currently being ingested…") that
   * clears when the count returns to zero.
   */
  ingestionInProgress: {
    active: boolean;
    pendingCount: number;
  };
}

export interface KnowledgeQnAEntry {
  id: string;
  orgId: string;
  question: string;
  answer: string;
  /** draft = not yet usable by agents; published = retrievable. */
  status: 'draft' | 'published' | 'deprecated';
  /** Operator rating from playground turns where this Q&A was cited. */
  rating?: 'good' | 'acceptable' | 'poor';
  /** Number of agent answers that have cited this entry. */
  citationCount: number;
  createdAt: number;
  updatedAt: number;
}
