import { describe, expect, it } from 'vitest'
import {
  buildKnowledgeHierarchy,
  collapseHierarchyToScene,
  isExpandable,
  KNOWLEDGE_GRAPH_ROOT_ID,
} from '@/features/knowledge/components/knowledgeGraphHierarchy'
import type { LiveKnowledgePayload } from '@/shared/api/knowledge-live-client'

const labels = {
  root: 'Knowledge base',
  documents: (count: number) => `${count} documents`,
  chunks: (count: number) => `${count} chunks`,
}

function makeSource(id: string, providerKey: string, provider: string) {
  return {
    category: 'general',
    chunks: 3,
    chunksPreview: [],
    coverage: '100%',
    description: '',
    hitRate: '0%',
    id,
    owner: 'Ima Fernandes da Costa',
    provider,
    providerKey,
    related: [],
    similarity: '0',
    size: '1 MB',
    status: 'Indexed',
    tags: [],
    title: `Title ${id}`,
    type: 'PDF',
    updated: '2026-08-07',
  }
}

function makePayload(overrides: Partial<LiveKnowledgePayload> = {}): LiveKnowledgePayload {
  return {
    sources: [
      makeSource('doc-a', 'sharepoint', 'SharePoint'),
      makeSource('doc-b', 'sharepoint', 'SharePoint'),
      makeSource('doc-c', 'quarry', 'Web crawl'),
    ],
    integrations: [],
    graph: { available: false, edgeCount: 0, groups: [], links: [], nodeCount: 0, nodes: [], truncated: false },
    ...overrides,
  } as unknown as LiveKnowledgePayload
}

describe('buildKnowledgeHierarchy', () => {
  it('groups documents under one provider node per provider', () => {
    const hierarchy = buildKnowledgeHierarchy(makePayload(), labels)
    const root = hierarchy.byId.get(hierarchy.rootId)!

    expect(root.childIds).toHaveLength(2)
    const providers = root.childIds.map((id) => hierarchy.byId.get(id)!)
    expect(providers.map((node) => node.label).sort()).toEqual(['SharePoint', 'Web crawl'])

    const sharepoint = providers.find((node) => node.label === 'SharePoint')!
    expect(sharepoint.childIds).toHaveLength(2)
    expect(sharepoint.detail).toBe('2 documents')
    expect(sharepoint.sourceIds).toEqual(['doc-a', 'doc-b'])
  })

  it('keeps connected integrations that have no documents as empty tiers', () => {
    const hierarchy = buildKnowledgeHierarchy(
      makePayload({
        integrations: [
          { documents: '0', freshness: 'never', id: 'i1', name: 'Brreg', providerKey: 'brreg', status: 'Connected' },
          // Already represented by a source — must not be duplicated.
          { documents: '2', freshness: 'now', id: 'i2', name: 'SharePoint', providerKey: 'sharepoint', status: 'Connected' },
        ] as LiveKnowledgePayload['integrations'],
      }),
      labels,
    )
    const root = hierarchy.byId.get(hierarchy.rootId)!
    const labelsSeen = root.childIds.map((id) => hierarchy.byId.get(id)!.label).sort()

    expect(labelsSeen).toEqual(['Brreg', 'SharePoint', 'Web crawl'])
    expect(isExpandable(hierarchy, 'kg:provider:brreg')).toBe(false)
  })

  it('attaches extracted entities beneath the document that cites them', () => {
    const hierarchy = buildKnowledgeHierarchy(
      makePayload({
        graph: {
          available: true,
          edgeCount: 0,
          groups: ['policy'],
          links: [],
          nodeCount: 1,
          truncated: false,
          nodes: [
            {
              group: 'policy',
              id: 'entity-1',
              label: 'Shipping policy',
              radius: 4,
              sourceIds: ['doc-a'],
              sourceRefs: [],
              tone: 'policy',
              x: 0,
              y: 0,
            },
          ],
        },
      }),
      labels,
    )

    expect(hierarchy.byId.get('entity-1')!.parentId).toBe('kg:doc:doc-a')
    expect(hierarchy.byId.get('kg:doc:doc-a')!.childIds).toEqual(['entity-1'])
  })

  it('keeps entities whose citation resolves to no loaded document reachable from the root', () => {
    const hierarchy = buildKnowledgeHierarchy(
      makePayload({
        graph: {
          available: true,
          edgeCount: 0,
          groups: [],
          links: [],
          nodeCount: 1,
          truncated: false,
          nodes: [
            {
              group: 'policy',
              id: 'orphan',
              label: 'Orphan',
              radius: 2,
              sourceIds: ['missing-doc'],
              sourceRefs: [],
              tone: 'policy',
              x: 0,
              y: 0,
            },
          ],
        },
      }),
      labels,
    )

    expect(hierarchy.byId.get('orphan')!.parentId).toBe(KNOWLEDGE_GRAPH_ROOT_ID)
  })
})

describe('collapseHierarchyToScene', () => {
  it('draws only the top level when nothing but the root is expanded', () => {
    const hierarchy = buildKnowledgeHierarchy(makePayload(), labels)
    const scene = collapseHierarchyToScene(hierarchy, new Set([KNOWLEDGE_GRAPH_ROOT_ID]))

    expect(scene.nodes).toHaveLength(3) // root + 2 providers
    expect(scene.nodes.some((node) => node.id.startsWith('kg:doc:'))).toBe(false)
    expect(scene.edges).toHaveLength(2)
  })

  it('reveals a provider’s documents once that provider is expanded', () => {
    const hierarchy = buildKnowledgeHierarchy(makePayload(), labels)
    const scene = collapseHierarchyToScene(
      hierarchy,
      new Set([KNOWLEDGE_GRAPH_ROOT_ID, 'kg:provider:sharepoint']),
    )

    const ids = scene.nodes.map((node) => node.id)
    expect(ids).toContain('kg:doc:doc-a')
    expect(ids).toContain('kg:doc:doc-b')
    // The unexpanded provider keeps its child hidden.
    expect(ids).not.toContain('kg:doc:doc-c')
  })

  it('advertises hidden child counts on collapsed nodes only', () => {
    const hierarchy = buildKnowledgeHierarchy(makePayload(), labels)
    const scene = collapseHierarchyToScene(
      hierarchy,
      new Set([KNOWLEDGE_GRAPH_ROOT_ID, 'kg:provider:sharepoint']),
    )

    const collapsed = scene.nodes.find((node) => node.id === 'kg:provider:quarry')!
    const expanded = scene.nodes.find((node) => node.id === 'kg:provider:sharepoint')!
    expect(collapsed.label).toBe('Web crawl  +1')
    expect(expanded.label).toBe('SharePoint')
  })

  it('renders nothing but the root when the workspace has no sources at all', () => {
    const hierarchy = buildKnowledgeHierarchy(
      makePayload({ sources: [], integrations: [] }),
      labels,
    )
    const scene = collapseHierarchyToScene(hierarchy, new Set([KNOWLEDGE_GRAPH_ROOT_ID]))

    expect(scene.nodes).toHaveLength(1)
    expect(scene.edges).toHaveLength(0)
  })
})
