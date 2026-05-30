import 'server-only';
import {
  getKnowledgeDocuments,
  getKnowledgeIntegrations,
} from '@/app/api/knowledge/_lib/knowledge-data';
import { listQnAEntries } from '@/components/knowledge/server/qa-store';
import type { KnowledgeStats, KnowledgeMode } from '@/components/knowledge/types';

interface KnowledgeDocumentForStats {
  id: string;
  source: string;
  type: string;
  status: string;
  /** Optional — documents-service may not always report sizes. */
  sizeBytes?: number;
  /** Optional — comes from doc metadata when present. */
  isDirty?: boolean;
}

function modeForDocument(doc: KnowledgeDocumentForStats): KnowledgeMode {
  if (doc.source.startsWith('website-crawl:')) return 'website';
  if (doc.type === 'qa') return 'qa';
  if (doc.type === 'text') return 'text';
  if (doc.source.startsWith('integration:')) return 'integrations';
  return 'files';
}

/**
 * Wave 11 — single read pass that the layout uses to render the right
 * rail. Pulls real counts from documents-service + integration-engine +
 * Convex Q&A; nothing here is synthetic. When a service is unreachable
 * we surface zeros for that slice (documents-data.ts already swallows
 * ECONNREFUSED in Phase 1).
 */
export async function loadKnowledgeStats(): Promise<KnowledgeStats> {
  const [docsResult, integrationsResult, qaResult] = await Promise.allSettled([
    getKnowledgeDocuments({ limit: 500 }),
    getKnowledgeIntegrations(),
    listQnAEntries(),
  ]);

  const docs =
    docsResult.status === 'fulfilled'
      ? (docsResult.value.documents as KnowledgeDocumentForStats[])
      : [];
  const integrations =
    integrationsResult.status === 'fulfilled' ? integrationsResult.value : null;
  const qa = qaResult.status === 'fulfilled' ? qaResult.value : [];

  const byMode: Record<KnowledgeMode, number> = {
    files: 0,
    text: 0,
    website: 0,
    qa: qa.length,
    integrations: 0,
  };

  let totalSizeBytes = 0;
  let dirty = false;
  let pendingCount = 0;

  for (const doc of docs) {
    const mode = modeForDocument(doc);
    byMode[mode] = (byMode[mode] ?? 0) + 1;
    if (typeof doc.sizeBytes === 'number') totalSizeBytes += doc.sizeBytes;
    if (doc.isDirty) dirty = true;
    // Phase 4: any doc still being processed counts toward the
    // "Your content is currently being ingested" banner.
    const status = (doc.status ?? '').toLowerCase();
    if (status === 'pending' || status === 'processing' || status === 'indexing' || status === 'queued') {
      pendingCount += 1;
    }
  }

  const totalConnected = integrations?.totalConnected ?? 0;
  byMode.integrations = totalConnected;

  // The "Sources" stat is logical, not physical: each website-crawl
  // collapses to one source, integrations count as one each, and every
  // Q&A entry plus file/text doc is its own source.
  const uniqueWebsiteSources = new Set(
    docs
      .filter((doc) => doc.source.startsWith('website-crawl:'))
      .map((doc) => doc.source),
  );
  const totalSources =
    uniqueWebsiteSources.size +
    byMode.files +
    byMode.text +
    qa.length +
    totalConnected;

  // Storage quota: take from env (per-plan) or default to 20 MB free tier.
  const storageQuotaBytes = Number(process.env.KNOWLEDGE_STORAGE_QUOTA_BYTES ?? 20 * 1024 * 1024);

  return {
    totalDocuments: docs.length + qa.length,
    totalSources,
    totalSizeBytes,
    storageQuotaBytes,
    dirty,
    byMode,
    ingestionInProgress: {
      active: pendingCount > 0,
      pendingCount,
    },
  };
}
