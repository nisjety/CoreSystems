import {
  GRAPH_CORE_COLOR,
  hueForKey,
  type SourceGraphVisualEdge,
  type SourceGraphVisualNode,
} from '@/features/onboarding/components/steps/connectGraphScene'
import type {
  LiveKnowledgeGraphNode,
  LiveKnowledgePayload,
} from '@/shared/api/knowledge-live-client'

/**
 * The Graf tab renders the workspace as a progressively-disclosed tree rather
 * than dumping every entity at once:
 *
 *   root (workspace) → provider (SharePoint, web crawl, …) → document → entity
 *
 * Only the first tier is drawn on load; deeper tiers materialise when the user
 * expands a node. The top two tiers are derived from data the workspace payload
 * always carries (sources/integrations), so the graph shows the real corpus even
 * before entity extraction has produced a single `graph.nodes` entry — the
 * extracted entities simply become the deepest tier once they exist.
 */
export const KNOWLEDGE_GRAPH_ROOT_ID = 'kg:root'

export type KnowledgeGraphTier = 'root' | 'provider' | 'document' | 'entity'

export type KnowledgeHierarchyNode = {
  /** Stable synthetic id. Entity tiers reuse the live graph node id verbatim. */
  id: string
  label: string
  detail: string
  tier: KnowledgeGraphTier
  parentId: string | null
  childIds: string[]
  /** Live source ids this node covers, so the inspector can resolve documents. */
  sourceIds: string[]
  clusterKey: string
  /** Present only for entity tiers, so the inspector keeps its richer payload. */
  liveNode?: LiveKnowledgeGraphNode
}

export type KnowledgeHierarchy = {
  byId: Map<string, KnowledgeHierarchyNode>
  rootId: string
}

function providerNodeId(providerKey: string): string {
  return `kg:provider:${providerKey}`
}

function documentNodeId(sourceId: string): string {
  return `kg:doc:${sourceId}`
}

/**
 * Build the full tree once per payload. Every tier is materialised here (it is
 * cheap, in-memory, and already-loaded data); what the user sees is decided
 * later by {@link collapseHierarchyToScene}, which walks only expanded nodes.
 */
export function buildKnowledgeHierarchy(
  payload: LiveKnowledgePayload,
  labels: { root: string; documents: (count: number) => string; chunks: (count: number) => string },
): KnowledgeHierarchy {
  const byId = new Map<string, KnowledgeHierarchyNode>()

  const root: KnowledgeHierarchyNode = {
    id: KNOWLEDGE_GRAPH_ROOT_ID,
    label: labels.root,
    detail: labels.documents(payload.sources.length),
    tier: 'root',
    parentId: null,
    childIds: [],
    sourceIds: payload.sources.map((source) => source.id),
    clusterKey: 'root',
  }
  byId.set(root.id, root)

  // Tier 1 — one node per provider that actually owns documents.
  for (const source of payload.sources) {
    const providerKey = source.providerKey || source.provider || 'other'
    const id = providerNodeId(providerKey)
    let provider = byId.get(id)
    if (!provider) {
      provider = {
        id,
        label: source.provider || providerKey,
        detail: '',
        tier: 'provider',
        parentId: root.id,
        childIds: [],
        sourceIds: [],
        clusterKey: providerKey,
      }
      byId.set(id, provider)
      root.childIds.push(id)
    }
    provider.sourceIds.push(source.id)

    const docId = documentNodeId(source.id)
    byId.set(docId, {
      id: docId,
      label: source.title,
      detail: `${source.type} · ${labels.chunks(source.chunks)}`,
      tier: 'document',
      parentId: id,
      childIds: [],
      sourceIds: [source.id],
      clusterKey: providerKey,
    })
    provider.childIds.push(docId)
  }

  // Connected integrations that have not produced documents yet are still real
  // parts of the corpus surface, so they appear as empty (non-expandable) tiers
  // rather than being silently dropped.
  for (const integration of payload.integrations) {
    const id = providerNodeId(integration.providerKey)
    if (byId.has(id)) continue
    byId.set(id, {
      id,
      label: integration.name,
      detail: integration.status,
      tier: 'provider',
      parentId: root.id,
      childIds: [],
      sourceIds: [],
      clusterKey: integration.providerKey,
    })
    root.childIds.push(id)
  }

  for (const provider of byId.values()) {
    if (provider.tier === 'provider' && provider.childIds.length > 0) {
      provider.detail = labels.documents(provider.childIds.length)
    }
  }

  // Tier 3 — extracted entities, attached to the first document they cite.
  // Entities whose citation does not resolve to a loaded document hang off the
  // root so they stay reachable instead of vanishing.
  for (const node of payload.graph.nodes) {
    const parentDocId = node.sourceIds
      .map((sourceId) => documentNodeId(sourceId))
      .find((candidate) => byId.get(candidate)?.tier === 'document')
    const parent = byId.get(parentDocId ?? root.id) ?? root
    if (byId.has(node.id)) continue
    byId.set(node.id, {
      id: node.id,
      label: node.label,
      detail: node.group,
      tier: 'entity',
      parentId: parent.id,
      childIds: [],
      sourceIds: node.sourceIds,
      clusterKey: node.group,
      liveNode: node,
    })
    parent.childIds.push(node.id)
  }

  return { byId, rootId: root.id }
}

const TIER_VISUALS: Record<
  KnowledgeGraphTier,
  { kind: SourceGraphVisualNode['kind']; role: SourceGraphVisualNode['role']; sizeWeight: number }
> = {
  root: { kind: 'core', role: 'core', sizeWeight: 1 },
  provider: { kind: 'integration', role: 'hub', sizeWeight: 0.74 },
  document: { kind: 'knowledge', role: 'leaf', sizeWeight: 0.52 },
  entity: { kind: 'signal', role: 'leaf', sizeWeight: 0.38 },
}

/**
 * Project the tree down to what is currently on screen: the root, plus the
 * children of every expanded node reachable from it. A collapsed node that has
 * children advertises the count in its label so the affordance to click is
 * visible without a second UI element.
 */
export function collapseHierarchyToScene(
  hierarchy: KnowledgeHierarchy,
  expandedIds: ReadonlySet<string>,
): { nodes: SourceGraphVisualNode[]; edges: SourceGraphVisualEdge[] } {
  const nodes: SourceGraphVisualNode[] = []
  const edges: SourceGraphVisualEdge[] = []
  const visible = new Set<string>()

  const queue: string[] = [hierarchy.rootId]
  while (queue.length > 0) {
    const id = queue.shift()!
    const node = hierarchy.byId.get(id)
    if (!node || visible.has(id)) continue
    visible.add(id)

    const expanded = expandedIds.has(id)
    const visuals = TIER_VISUALS[node.tier]
    const childCount = node.childIds.length
    nodes.push({
      id: node.id,
      label: childCount > 0 && !expanded ? `${node.label}  +${childCount}` : node.label,
      detail: node.detail,
      kind: visuals.kind,
      strength: childCount > 0 ? Math.min(1, childCount / 8) : 0.2,
      connected: node.parentId !== null || childCount > 0,
      color: node.tier === 'root' ? GRAPH_CORE_COLOR : hueForKey(node.clusterKey),
      sizeWeight: visuals.sizeWeight,
      role: visuals.role,
      clusterKey: node.clusterKey,
    })

    if (node.parentId && visible.has(node.parentId)) {
      edges.push({ from: node.parentId, to: node.id, label: '' })
    }
    if (expanded) queue.push(...node.childIds)
  }

  return { nodes, edges }
}

/** Nodes with children are the only ones a click can meaningfully expand. */
export function isExpandable(hierarchy: KnowledgeHierarchy, nodeId: string): boolean {
  return (hierarchy.byId.get(nodeId)?.childIds.length ?? 0) > 0
}
