import { describe, expect, it } from 'vitest'
import { knowledgeSnapshotResult } from '@/features/insights/lib/knowledge-insights'

describe('knowledgeSnapshotResult', () => {
  it('labels a reachable Data Plane result as a live permission-scoped snapshot', () => {
    const result = knowledgeSnapshotResult({
      dataPlane: { available: true, documentCount: 12, indexedCount: 9 },
      orgId: 'org-1',
      sources: [{ id: 'source-1' }, { id: 'source-2' }],
    })

    expect(result.state).toBe('live')
    expect(result.data).toEqual({ documentCount: 12, indexedCount: 9, sourceCount: 2 })
  })

  it('keeps an available but empty knowledge store honest', () => {
    const result = knowledgeSnapshotResult({
      dataPlane: { available: true, documentCount: 0, indexedCount: 0 },
      orgId: 'org-1',
      sources: [],
    })

    expect(result.state).toBe('empty')
  })

  it('does not render a snapshot when the Data Plane is unavailable', () => {
    const result = knowledgeSnapshotResult({
      dataPlane: { available: false, documentCount: 0, indexedCount: 0 },
      orgId: 'org-1',
      sources: [],
    })

    expect(result.state).toBe('unavailable')
  })
})
