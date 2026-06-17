import { type ReactNode } from 'react';
import { KnowledgeShell } from '@/components/knowledge/KnowledgeShell';
import { loadKnowledgeStats } from '@/components/knowledge/server/load-knowledge-stats';
import { requireEdgeUser } from '@/components/auth/lib/edge-session';
import { KNOWLEDGE_FEATURE_FLAGS } from '@/lib/knowledge/feature-flags';

export const dynamic = 'force-dynamic';

/**
 * Wave 11: every page under /knowledge/* renders inside the shell —
 * stats are computed server-side from real documents-service +
 * integration-engine + Quarry data; no mocks, no fixtures.
 *
 * Wave 11.C-a: feature flags for the GraphRAG viewer + LLM Wiki editor
 * are evaluated server-side here and passed to the shell so the
 * sub-nav surfaces those entries only when the flags are on.
 */
export default async function KnowledgeLayout({ children }: { children: ReactNode }) {
  await requireEdgeUser('/knowledge');
  const stats = await loadKnowledgeStats();
  return (
    <KnowledgeShell
      stats={stats}
      flags={{
        graph: KNOWLEDGE_FEATURE_FLAGS.graph,
        wiki: KNOWLEDGE_FEATURE_FLAGS.wiki,
      }}
    >
      {children}
    </KnowledgeShell>
  );
}
