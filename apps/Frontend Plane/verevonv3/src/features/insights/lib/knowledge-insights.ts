import { loadKnowledgeSources, type LiveKnowledgePayload } from '@/shared/api/knowledge-live-client'
import type { ResourceResult } from '@/shared/read-data'

export type KnowledgeInsightsSnapshot = {
  documentCount: number
  indexedCount: number
  sourceCount: number
}

type KnowledgeSnapshotInput = Pick<LiveKnowledgePayload, 'dataPlane' | 'orgId'> & {
  sources: readonly unknown[]
}

// The Data Plane endpoint is already protected by a gateway-minted user token
// and active organization scope. This is intentionally a current-state snapshot
// of documents visible to that user — not a fabricated historical time series.
export function knowledgeSnapshotResult(
  payload: KnowledgeSnapshotInput,
): ResourceResult<KnowledgeInsightsSnapshot> {
  const data = {
    documentCount: payload.dataPlane.documentCount,
    indexedCount: payload.dataPlane.indexedCount,
    sourceCount: payload.sources.length,
  }
  if (!payload.dataPlane.available) {
    return {
      data,
      message: 'The permission-scoped Data Plane snapshot is unavailable. No knowledge counts are shown as live data.',
      state: 'unavailable',
    }
  }
  if (data.documentCount === 0 && data.indexedCount === 0 && data.sourceCount === 0) {
    return {
      data,
      message: 'The Data Plane is reachable, but this organization and user scope currently expose no knowledge sources.',
      state: 'empty',
    }
  }
  return {
    data,
    message: 'Showing the current Data Plane snapshot for the active organization and signed-in user permissions.',
    state: 'live',
  }
}

export async function loadKnowledgeInsightsSnapshot(): Promise<ResourceResult<KnowledgeInsightsSnapshot>> {
  try {
    return knowledgeSnapshotResult(await loadKnowledgeSources())
  } catch {
    return {
      data: { documentCount: 0, indexedCount: 0, sourceCount: 0 },
      message: 'The permission-scoped Data Plane snapshot did not respond. No knowledge counts are shown.',
      state: 'unavailable',
    }
  }
}
