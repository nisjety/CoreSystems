import type { OnboardingState } from '@/features/onboarding/lib/model'
import { truncateGraphLabel } from '@/features/onboarding/lib/view'
import {
  GRAPH_CORE_COLOR,
  hueForKey,
  lightenHex,
  type SourceGraphVisualEdge,
  type SourceGraphVisualNode,
} from './connectGraphScene'

export type SourceGraphModel = {
  nodes: SourceGraphVisualNode[]
  edges: SourceGraphVisualEdge[]
  connectedSourceCount: number
  knowledgeNodeCount: number
}

type ConnectedSource = OnboardingState['connectors'][number]

export function buildSourceGraphModel(input: {
  connectedSources: OnboardingState['connectors']
  organizationName: string
  currentUserId: string
  currentUserName: string
}): SourceGraphModel {
  const connectedSources = Array.isArray(input.connectedSources)
    ? input.connectedSources.filter(isConnectedSource)
    : []
  const organizationName = normalizeRequiredText(input.organizationName)
  const currentUserId = normalizeRequiredText(input.currentUserId)
  const currentUserName = normalizeRequiredText(input.currentUserName)

  if (connectedSources.length === 0 || !organizationName || !currentUserId || !currentUserName) {
    return emptySourceGraphModel()
  }

  const { hubs, leaves, edges: sourceEdges } = buildConnectedSourceClusters(connectedSources)
  const core: SourceGraphVisualNode = {
    id: 'knowledge-base',
    label: 'Verevon Knowledge Base',
    detail: `${connectedSources.length} connected ${connectedSources.length === 1 ? 'integration' : 'integrations'}`,
    kind: 'core',
    strength: 1,
    connected: true,
    color: GRAPH_CORE_COLOR,
    sizeWeight: 3.1,
    role: 'core',
    clusterKey: 'knowledge-base',
  }
  const organization: SourceGraphVisualNode = {
    id: 'organization',
    label: organizationName,
    detail: 'Organization or personal workspace identity',
    kind: 'service',
    strength: 0.82,
    connected: true,
    color: hueForKey('organization'),
    sizeWeight: 1.4,
    role: 'standalone',
    clusterKey: 'organization',
  }
  const currentUser: SourceGraphVisualNode = {
    id: 'current-user',
    label: currentUserName,
    detail: 'Signed-in Verevon user',
    kind: 'service',
    strength: 0.74,
    connected: true,
    color: hueForKey(`user:${currentUserId}`),
    sizeWeight: 1.1,
    role: 'standalone',
    clusterKey: 'current-user',
  }
  const nodes = dedupeNodes([core, organization, currentUser, ...hubs, ...leaves])
  const edges = normalizeEdges(nodes, [
    { from: 'organization', to: 'knowledge-base', label: 'identity' },
    { from: 'current-user', to: 'organization', label: 'member' },
    ...sourceEdges,
  ])

  return {
    nodes: applyDegreeWeights(nodes, edges),
    edges,
    connectedSourceCount: connectedSources.length,
    knowledgeNodeCount: leaves.length,
  }
}

function emptySourceGraphModel(): SourceGraphModel {
  return {
    nodes: [],
    edges: [],
    connectedSourceCount: 0,
    knowledgeNodeCount: 0,
  }
}

function buildConnectedSourceClusters(connectedSources: OnboardingState['connectors']) {
  const hubs: SourceGraphVisualNode[] = []
  const leaves: SourceGraphVisualNode[] = []
  const edges: SourceGraphVisualEdge[] = []

  connectedSources.forEach((source) => {
    const hubId = `source:${source.id}`
    const items = [...new Set(
      (Array.isArray(source.sources) ? source.sources : [])
        .filter((item): item is string => typeof item === 'string')
        .map((item) => item.trim())
        .filter(Boolean),
    )]
    const hue = hueForKey(hubId)
    const itemNames = items.map((item) => humanizeSourceItem(item)).join(', ')

    hubs.push({
      id: hubId,
      label: source.label,
      detail: itemNames ? `${itemNames} - connected` : 'Connected integration',
      kind: 'integration',
      strength: 0.95,
      connected: true,
      color: hue,
      sizeWeight: 1,
      role: 'hub',
      clusterKey: hubId,
    })
    edges.push({ from: hubId, to: 'knowledge-base', label: 'sync' })

    items.forEach((item) => {
      const leafId = `${hubId}:${item}`
      leaves.push({
        id: leafId,
        label: truncateGraphLabel(humanizeSourceItem(item)),
        detail: `${source.label} - ${humanizeSourceItem(item)}`,
        kind: 'integration',
        strength: 0.7,
        connected: true,
        color: lightenHex(hue, 0.46),
        sizeWeight: 0.3,
        role: 'leaf',
        clusterKey: hubId,
      })
      edges.push({ from: hubId, to: leafId, label: 'item' })
    })
  })

  return { hubs, leaves, edges }
}

function isConnectedSource(value: unknown): value is ConnectedSource {
  if (!value || typeof value !== 'object') return false
  const source = value as Partial<ConnectedSource>

  return source.status === 'connected'
    && normalizeRequiredText(source.id).length > 0
    && normalizeRequiredText(source.label).length > 0
    && (source.sources === undefined || Array.isArray(source.sources))
}

function normalizeRequiredText(value: unknown): string {
  return typeof value === 'string' ? value.trim() : ''
}

function applyDegreeWeights(nodes: readonly SourceGraphVisualNode[], edges: readonly SourceGraphVisualEdge[]) {
  const degrees = new Map(nodes.map((node) => [node.id, 0]))
  edges.forEach((edge) => {
    degrees.set(edge.from, (degrees.get(edge.from) ?? 0) + 1)
    degrees.set(edge.to, (degrees.get(edge.to) ?? 0) + 1)
  })

  return nodes.map((node) => {
    const degree = degrees.get(node.id) ?? 0
    const sizeWeight =
      node.role === 'core'
        ? 3.25 + Math.min(degree, 12) * 0.08
        : node.role === 'hub'
          ? 1.05 + Math.min(degree, 8) * 0.24
          : node.role === 'standalone'
            ? 0.78 + Math.min(degree, 5) * 0.18
            : 0.3 + Math.min(degree, 4) * 0.05

    return { ...node, sizeWeight }
  })
}

function normalizeEdges(
  nodes: readonly SourceGraphVisualNode[],
  edges: readonly SourceGraphVisualEdge[],
): SourceGraphVisualEdge[] {
  const nodeIds = new Set(nodes.map((node) => node.id))
  const seen = new Set<string>()

  return edges.filter((edge) => {
    if (!nodeIds.has(edge.from) || !nodeIds.has(edge.to) || edge.from === edge.to) return false
    const key = [edge.from, edge.to].sort().join('\u0000')
    if (seen.has(key)) return false
    seen.add(key)
    return true
  })
}

function dedupeNodes(nodes: readonly SourceGraphVisualNode[]) {
  const seen = new Set<string>()
  return nodes.filter((node) => {
    if (seen.has(node.id)) return false
    seen.add(node.id)
    return true
  })
}

function humanizeSourceItem(value: string): string {
  return value
    .split('_')
    .map((word) => word.charAt(0).toUpperCase() + word.slice(1))
    .join(' ')
}
