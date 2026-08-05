import { describe, expect, it } from 'vitest'
import { buildSourceGraphModel } from './connectGraphModel'

const identity = {
  organizationName: 'AQUATIQ AS',
  currentUserId: 'user-42',
  currentUserName: 'Ima Fernandes',
}

const disconnectedContext = {
  graphNodes: [
    {
      id: 'knowledge-1',
      label: 'Existing preview data',
      group: 'knowledge',
      position: { left: '50%', top: '50%' },
    },
  ],
  graphEdges: [{ a: 'org', b: 'knowledge-1' }],
  websiteUrl: 'https://aquatiq.com',
}

describe('buildSourceGraphModel', () => {
  it.each([
    { label: 'no connectors', connectedSources: [] },
    {
      label: 'pending connector',
      connectedSources: [{ id: 'slack', label: 'Slack', status: 'pending' as const, sources: ['messages'] }],
    },
    {
      label: 'partial connector',
      connectedSources: [{ id: 'notion', label: 'Notion', status: 'partial' as const, sources: ['pages'] }],
    },
    {
      label: 'pending and partial connectors',
      connectedSources: [
        { id: 'slack', label: 'Slack', status: 'pending' as const, sources: ['messages'] },
        { id: 'notion', label: 'Notion', status: 'partial' as const, sources: ['pages'] },
      ],
    },
  ])('returns a completely empty graph for $label', ({ connectedSources }) => {
    const model = buildSourceGraphModel({
      ...disconnectedContext,
      ...identity,
      connectedSources,
    })

    expect(model).toEqual({
      nodes: [],
      edges: [],
      connectedSourceCount: 0,
      knowledgeNodeCount: 0,
    })
  })

  it.each([
    { organizationName: '', currentUserId: 'user-42', currentUserName: 'Ima Fernandes' },
    { organizationName: 'AQUATIQ AS', currentUserId: '', currentUserName: 'Ima Fernandes' },
    { organizationName: 'AQUATIQ AS', currentUserId: 'user-42', currentUserName: '' },
  ])('stays empty when required identity is missing', (missingIdentity) => {
    const model = buildSourceGraphModel({
      ...missingIdentity,
      connectedSources: [
        { id: 'slack', label: 'Slack', status: 'connected', sources: ['messages'] },
      ],
    })

    expect(model.nodes).toEqual([])
    expect(model.edges).toEqual([])
  })

  it('fails closed when runtime identity data is absent', () => {
    const model = buildSourceGraphModel({
      connectedSources: [
        { id: 'slack', label: 'Slack', status: 'connected', sources: ['messages'] },
      ],
      organizationName: undefined,
      currentUserId: undefined,
      currentUserName: undefined,
    } as unknown as Parameters<typeof buildSourceGraphModel>[0])

    expect(model).toEqual({
      nodes: [],
      edges: [],
      connectedSourceCount: 0,
      knowledgeNodeCount: 0,
    })
  })

  it('fails closed when runtime identity data is not a string', () => {
    const model = buildSourceGraphModel({
      connectedSources: [
        { id: 'slack', label: 'Slack', status: 'connected', sources: ['messages'] },
      ],
      organizationName: { unexpected: true },
      currentUserId: 42,
      currentUserName: ['Ima'],
    } as unknown as Parameters<typeof buildSourceGraphModel>[0])

    expect(model.nodes).toEqual([])
    expect(model.edges).toEqual([])
  })

  it('ignores malformed connector entries and non-string source values', () => {
    const model = buildSourceGraphModel({
      ...identity,
      connectedSources: [
        null,
        { id: 'slack', label: 'Slack', status: 'connected', sources: ['messages', 42, null] },
        { id: 12, label: 'Invalid', status: 'connected', sources: ['pages'] },
      ],
    } as unknown as Parameters<typeof buildSourceGraphModel>[0])

    expect(model.connectedSourceCount).toBe(1)
    expect(model.nodes.map((node) => node.id)).toEqual(expect.arrayContaining([
      'knowledge-base',
      'organization',
      'current-user',
      'source:slack',
      'source:slack:messages',
    ]))
    expect(model.nodes.some((node) => node.id.includes('42') || node.id.includes('null'))).toBe(false)
  })

  it('includes every genuinely connected integration', () => {
    const connectedSources = Array.from({ length: 9 }, (_, index) => ({
      id: `source-${index + 1}`,
      label: `Source ${index + 1}`,
      status: 'connected' as const,
      sources: [`item-${index + 1}`],
    }))

    const model = buildSourceGraphModel({
      ...identity,
      connectedSources,
    })

    expect(model.connectedSourceCount).toBe(9)
    expect(model.nodes.filter((node) => node.role === 'hub')).toHaveLength(9)
  })

  it('shows identity and only genuinely connected integrations with their real source children', () => {
    const connectedSources = [
      {
        id: 'slack',
        label: 'Slack',
        status: 'connected' as const,
        sources: ['messages', 'threads', 'messages'],
        sourceCount: 3,
      },
      {
        id: 'notion',
        label: 'Notion',
        status: 'pending' as const,
        sources: ['pages'],
      },
      {
        id: 'microsoft365',
        label: 'Microsoft 365',
        status: 'partial' as const,
        sources: ['teams'],
      },
    ]
    const sourcesSnapshot = structuredClone(connectedSources)

    const model = buildSourceGraphModel({
      ...disconnectedContext,
      ...identity,
      currentUserName: '  Ima Fernandes  ',
      connectedSources,
    })

    expect(model.nodes.map((node) => node.id).sort()).toEqual([
      'current-user',
      'knowledge-base',
      'organization',
      'source:slack',
      'source:slack:messages',
      'source:slack:threads',
    ])
    expect(model.edges).toHaveLength(5)
    expect(model.edges).toEqual(expect.arrayContaining([
      { from: 'organization', to: 'knowledge-base', label: 'identity' },
      { from: 'current-user', to: 'organization', label: 'member' },
      { from: 'source:slack', to: 'knowledge-base', label: 'sync' },
      { from: 'source:slack', to: 'source:slack:messages', label: 'item' },
      { from: 'source:slack', to: 'source:slack:threads', label: 'item' },
    ]))
    expect(model.nodes.find((node) => node.id === 'current-user')?.label).toBe('Ima Fernandes')
    expect(model.nodes.some((node) => ['website', 'knowledge-hub', 'shared-inbox', 'automation', 'answers'].includes(node.id))).toBe(false)
    expect(model.nodes.some((node) => node.id.startsWith('source:notion') || node.id.startsWith('source:microsoft365'))).toBe(false)

    const hub = model.nodes.find((node) => node.id === 'source:slack')
    const leaves = model.nodes.filter((node) => node.clusterKey === 'source:slack' && node.role === 'leaf')
    expect(leaves).toHaveLength(2)
    expect(leaves.every((leaf) => leaf.sizeWeight < (hub?.sizeWeight ?? 0))).toBe(true)
    expect(model.edges.every((edge) => model.nodes.some((node) => node.id === edge.from) && model.nodes.some((node) => node.id === edge.to))).toBe(true)
    expect(connectedSources).toEqual(sourcesSnapshot)
  })
})
