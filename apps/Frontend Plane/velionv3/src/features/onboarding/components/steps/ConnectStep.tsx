import { Minus, Plus, RotateCcw } from 'lucide-solid'
import { createEffect, createMemo, createSignal, For, onCleanup, onMount, Show } from 'solid-js'
import { OnboardingLinkButton } from '@/features/onboarding/components/shared/OnboardingLinkButton'
import {
  type ConnectorCategory,
  type ConnectorOption,
  type GraphDisplayNode,
  type OnboardingState,
  onboardingConnectorOptions,
} from '@/features/onboarding/lib/model'
import { truncateGraphLabel } from '@/features/onboarding/lib/view'
import { Badge } from '@/shared/ui/Badge'
import { Button } from '@/shared/ui/Button'
import { VelionIconButton } from '@/shared/ui/velion/VelionIconButton'
import { VelionSelectableRow } from '@/shared/ui/velion/VelionSelectableRow'
import {
  createConnectGraphScene,
  GRAPH_CORE_COLOR,
  hueForKey,
  lightenHex,
  type SourceGraphSceneController,
  type SourceGraphVisualEdge,
  type SourceGraphVisualNode,
} from './connectGraphScene'

const connectorTabs: Array<{
  id: ConnectorCategory
  label: string
  description: string
}> = [
  {
    id: 'work',
    label: 'Work systems',
    description: 'Slack, Microsoft, Google, Notion, GitHub',
  },
  {
    id: 'social',
    label: 'Social channels',
    description: 'Meta (Facebook, Instagram, WhatsApp, Ads), LinkedIn, TikTok, X',
  },
  {
    id: 'other',
    label: 'Other apps',
    description: 'Commerce, billing, shipping and operational sources',
  },
]

type ConnectStepContentProps = {
  connectedSources: OnboardingState['connectors']
  connectingId?: string
  onConnect: (option: ConnectorOption) => void | Promise<void>
  onContinue: () => void
  onSkip: () => void
  /** Fired on hover/focus of the advance buttons so the plan recommendation
   * can start fetching early (only armed by the caller when a source is
   * connected), so it's ready by the time the user reaches the plan step. */
  onPrefetch?: () => void
}

export function ConnectStepContent(props: ConnectStepContentProps) {
  const [activeTab, setActiveTab] = createSignal<ConnectorCategory>('work')
  const visibleConnectors = createMemo(() =>
    onboardingConnectorOptions.filter((item) => item.category === activeTab()),
  )

  return (
    <section class="onboarding-copy onboarding-copy--connect">
      <p class="onboarding-eyebrow">Integrations</p>
      <h1>Koble systemer</h1>
      <p>Velg systemene Velion skal lære fra, svare på vegne av, eller bruke som signaler for automasjon.</p>

      <div class="onboarding-connector-tabs" role="tablist" aria-label="Integration categories">
        <For each={connectorTabs}>
          {(tab) => (
            <button
              type="button"
              role="tab"
              aria-selected={activeTab() === tab.id}
              class="onboarding-connector-tab"
              classList={{ 'onboarding-connector-tab--active': activeTab() === tab.id }}
              onClick={() => setActiveTab(tab.id)}
            >
              <span>{tab.label}</span>
              <small>{tab.description}</small>
            </button>
          )}
        </For>
      </div>

      <div class="onboarding-connector-group" role="tabpanel">
        <h3>{connectorTabs.find((tab) => tab.id === activeTab())?.label}</h3>
        <div class="onboarding-connector-list">
          <For each={visibleConnectors()}>
            {(item) => {
              const status = () => props.connectedSources.find((connector) => connector.id === item.id)?.status

              return (
                <VelionSelectableRow
                  compact
                  onClick={() => void props.onConnect(item)}
                  disabled={props.connectingId === item.id}
                  title={item.label}
                  description={item.hint}
                  meta={
                    <Badge tone={status() === 'connected' ? 'accent' : 'neutral'}>
                      {connectorStatusLabel(status(), props.connectingId === item.id)}
                    </Badge>
                  }
                />
              )
            }}
          </For>
        </div>
      </div>

      <div class="onboarding-actions onboarding-actions--connect">
        <Button
          variant="primary"
          size="sm"
          onClick={props.onContinue}
          onMouseEnter={() => props.onPrefetch?.()}
          onFocus={() => props.onPrefetch?.()}
        >
          Fortsett
        </Button>
        <OnboardingLinkButton
          onClick={props.onSkip}
          onMouseEnter={() => props.onPrefetch?.()}
          onFocus={() => props.onPrefetch?.()}
        >
          Hopp over
        </OnboardingLinkButton>
      </div>
    </section>
  )
}

function connectorStatusLabel(
  status: OnboardingState['connectors'][number]['status'] | undefined,
  connecting: boolean,
) {
  if (connecting) return 'Åpner'
  if (status === 'connected') return 'Tilkoblet'
  if (status === 'partial') return 'Delvis'
  if (status === 'pending') return 'Venter'
  return 'Legg til'
}

type ConnectStepVisualProps = {
  graphNodes: GraphDisplayNode[]
  connectedSources: OnboardingState['connectors']
  websiteUrl?: string
  organizationName?: string
}

type SourceGraphModel = {
  nodes: SourceGraphVisualNode[]
  edges: SourceGraphVisualEdge[]
  connectedSourceCount: number
  knowledgeNodeCount: number
}

const KNOWLEDGE_NODE_LIMIT = 8
const CONNECTED_SOURCE_LIMIT = 8
const FALLBACK_CONNECTOR_IDS = ['slack', 'microsoft365', 'gdrive', 'meta', 'shopify']

export function ConnectStepVisual(props: ConnectStepVisualProps) {
  let hostRef!: HTMLDivElement
  let graphRef!: HTMLDivElement
  let sceneController: SourceGraphSceneController | undefined
  const [graphReady, setGraphReady] = createSignal(true)
  const [zoomPercent, setZoomPercent] = createSignal(100)
  const [hoveringNode, setHoveringNode] = createSignal(false)
  const [selectedNode, setSelectedNode] = createSignal<SourceGraphVisualNode>()
  const graphModel = createMemo(() =>
    buildSourceGraphModel({
      connectedSources: props.connectedSources,
      graphNodes: props.graphNodes,
      organizationName: props.organizationName,
      websiteUrl: props.websiteUrl,
    }),
  )

  onMount(() => {
    const reducedMotionQuery = getReducedMotionQuery()
    let startAttempts = 0
    let startFrame: number | undefined
    const startScene = () => {
      if (!graphRef || !hostRef) {
        if (startAttempts < 6) {
          startAttempts += 1
          startFrame = window.requestAnimationFrame(startScene)
        } else {
          setGraphReady(false)
        }
        return
      }
      startAttempts = 0
      sceneController?.dispose()
      sceneController = createConnectGraphScene(graphRef, hostRef, {
        reducedMotion: reducedMotionQuery?.matches ?? false,
        onHoverNode: (node) => setHoveringNode(Boolean(node)),
        onSelectNode: (pick) => setSelectedNode(pick?.node),
      })
      setGraphReady(Boolean(sceneController))
      if (!sceneController) return

      const model = graphModel()
      sceneController.setData(model.nodes, model.edges)
      setZoomPercent(sceneController.zoomPercent())
    }
    const handleReducedMotionChange = () => startScene()

    startScene()

    if (!reducedMotionQuery) {
      onCleanup(() => {
        if (startFrame !== undefined) window.cancelAnimationFrame(startFrame)
        sceneController?.dispose()
        sceneController = undefined
      })
      return
    }

    if (typeof reducedMotionQuery.addEventListener === 'function') {
      reducedMotionQuery.addEventListener('change', handleReducedMotionChange)
    } else {
      reducedMotionQuery.addListener(handleReducedMotionChange)
    }

    onCleanup(() => {
      if (startFrame !== undefined) window.cancelAnimationFrame(startFrame)
      if (typeof reducedMotionQuery.removeEventListener === 'function') {
        reducedMotionQuery.removeEventListener('change', handleReducedMotionChange)
      } else {
        reducedMotionQuery.removeListener(handleReducedMotionChange)
      }
      sceneController?.dispose()
      sceneController = undefined
    })
  })

  createEffect(() => {
    const model = graphModel()
    sceneController?.setData(model.nodes, model.edges)
    const activeNode = selectedNode()
    if (activeNode && !model.nodes.some((node) => node.id === activeNode.id)) {
      setSelectedNode(undefined)
      sceneController?.setActiveNode(undefined)
    }
  })

  const zoom = (direction: 1 | -1) => {
    const nextZoom = sceneController?.zoom(direction)
    if (nextZoom) setZoomPercent(nextZoom)
  }

  const reset = () => {
    const nextZoom = sceneController?.reset()
    if (nextZoom) setZoomPercent(nextZoom)
    sceneController?.setActiveNode(selectedNode()?.id)
  }

  const closeSelectedCard = () => {
    setSelectedNode(undefined)
    sceneController?.setActiveNode(undefined)
  }

  return (
    <div
      ref={hostRef}
      class="onboarding-source-graph"
      classList={{ 'onboarding-source-graph--fallback': !graphReady() }}
      aria-label="Integration knowledge graph"
    >
      <div
        ref={graphRef}
        class="onboarding-source-graph__engine"
        classList={{ 'onboarding-source-graph__engine--hovering': hoveringNode() }}
        aria-label="Clickable 3D integration graph nodes"
        role="img"
      />
      <div class="onboarding-source-graph__glow" aria-hidden="true" />
      <div class="onboarding-source-graph__scanline" aria-hidden="true" />

      <div class="onboarding-source-graph__controls">
        <VelionIconButton aria-label="Zoom out" size="sm" shape="rounded" tone="inverted" onClick={() => zoom(-1)}>
          <Minus size={14} />
        </VelionIconButton>
        <span>{zoomPercent()}%</span>
        <VelionIconButton aria-label="Zoom in" size="sm" shape="rounded" tone="inverted" onClick={() => zoom(1)}>
          <Plus size={14} />
        </VelionIconButton>
        <VelionIconButton aria-label="Reset graph" size="sm" shape="rounded" tone="inverted" onClick={reset}>
          <RotateCcw size={14} />
        </VelionIconButton>
      </div>

      <div class="onboarding-source-graph__fallback-nodes" aria-hidden="true">
        <For each={graphModel().nodes.filter((node) => node.kind !== 'core').slice(0, 10)}>
          {(node) => (
            <div
              class="onboarding-source-graph__node"
              classList={{
                'onboarding-source-graph__node--integration': node.kind === 'integration',
                'onboarding-source-graph__node--service': node.kind === 'service',
                'onboarding-source-graph__node--knowledge': node.kind === 'knowledge',
                'onboarding-source-graph__node--signal': node.kind === 'signal',
              }}
            >
              <span />
              <small>{truncateGraphLabel(node.label)}</small>
            </div>
          )}
        </For>
      </div>

      <Show when={selectedNode()}>
        {(node) => (
          <article class="onboarding-source-graph__detail">
            <button
              type="button"
              class="onboarding-source-graph__detail-close"
              aria-label="Close node details"
              onClick={closeSelectedCard}
            >
              ×
            </button>
            <span>{nodeKindLabel(node().kind)}</span>
            <strong>{node().label}</strong>
            <small>{node().detail}</small>
            <em>{node().connected ? 'Connected' : 'Ready to connect'}</em>
          </article>
        )}
      </Show>
    </div>
  )
}

function buildSourceGraphModel(input: {
  connectedSources: OnboardingState['connectors']
  graphNodes: GraphDisplayNode[]
  organizationName?: string
  websiteUrl?: string
}): SourceGraphModel {
  const organizationLabel = input.organizationName?.trim() || graphOrgLabel(input.graphNodes) || 'Organization'
  const websiteLabel = formatWebsiteLabel(input.websiteUrl)
  const { hubs: sourceHubs, leaves: sourceLeaves, edges: sourceEdges } = buildSourceClusters(input.connectedSources)
  const knowledgeItems = input.graphNodes.filter((node) => node.group !== 'org').slice(0, KNOWLEDGE_NODE_LIMIT)
  const knowledgeCluster = buildKnowledgeCluster(knowledgeItems)
  const signalEntries = buildSignalNodes()
  const connectedSourceCount = totalConnectedSources(input.connectedSources)
  const knowledgeNodeCount = Math.max(knowledgeItems.length, input.graphNodes.length, connectedSourceCount)

  const core: SourceGraphVisualNode = {
    id: 'knowledge-base',
    label: 'Velion Knowledge Base',
    detail: `Unified evidence graph - ${knowledgeNodeCount} nodes`,
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
    label: organizationLabel,
    detail: 'Company identity and context',
    kind: 'service',
    strength: 0.72,
    connected: Boolean(input.organizationName),
    color: hueForKey('organization'),
    sizeWeight: 1.5,
    role: 'standalone',
    clusterKey: 'organization',
  }

  const website: SourceGraphVisualNode = {
    id: 'website',
    label: websiteLabel,
    detail: input.websiteUrl ? 'Crawled pages and product content' : 'Website crawl source',
    kind: 'service',
    strength: input.websiteUrl ? 0.88 : 0.56,
    connected: Boolean(input.websiteUrl),
    color: hueForKey('website'),
    sizeWeight: 1.5,
    role: 'standalone',
    clusterKey: 'website',
  }

  const nodes: SourceGraphVisualNode[] = [
    core,
    organization,
    website,
    ...sourceHubs,
    ...sourceLeaves,
    ...(knowledgeCluster.hub ? [knowledgeCluster.hub] : []),
    ...knowledgeCluster.leaves,
    ...signalEntries.map((entry) => entry.node),
  ]

  const edges: SourceGraphVisualEdge[] = [
    { from: 'organization', to: 'knowledge-base', label: 'identity' },
    { from: 'website', to: 'knowledge-base', label: 'crawl' },
    ...sourceEdges,
    ...(knowledgeCluster.hub ? [{ from: 'knowledge-base', to: knowledgeCluster.hub.id, label: 'evidence' }] : []),
    ...knowledgeCluster.edges,
    ...signalEntries.map((entry) => ({ from: 'knowledge-base', to: entry.node.id, label: entry.edgeLabel })),
  ]

  return {
    nodes,
    edges,
    connectedSourceCount,
    knowledgeNodeCount,
  }
}

function buildSourceClusters(connectedSources: OnboardingState['connectors']) {
  const sourceInputs =
    connectedSources.length > 0
      ? connectedSources
      : onboardingConnectorOptions
          .filter((option) => FALLBACK_CONNECTOR_IDS.includes(option.id))
          .map((option) => ({
            id: option.id,
            label: option.label,
            status: 'pending' as const,
            sources: option.sources,
            sourceCount: option.sources.length,
          }))

  const visibleSources = sourceInputs.slice(0, CONNECTED_SOURCE_LIMIT)
  const hubs: SourceGraphVisualNode[] = []
  const leaves: SourceGraphVisualNode[] = []
  const edges: SourceGraphVisualEdge[] = []

  visibleSources.forEach((source) => {
    const hubId = `source:${source.id}`
    const items = source.sources && source.sources.length > 0 ? source.sources : [source.label]
    const connected = source.status === 'connected'
    const hue = hueForKey(hubId)
    const childCount = items.length
    const itemNames = items.map((item) => humanizeSourceItem(item)).join(', ')

    hubs.push({
      id: hubId,
      label: source.label,
      detail: `${itemNames} - ${source.status}`,
      kind: 'integration',
      strength: connected ? 0.95 : source.status === 'partial' ? 0.72 : 0.58,
      connected,
      color: hue,
      sizeWeight: 1.05 + Math.min(childCount, 6) * 0.17,
      role: 'hub',
      clusterKey: hubId,
    })
    edges.push({ from: hubId, to: 'knowledge-base', label: 'sync' })

    items.forEach((item, itemIndex) => {
      const leafId = `${hubId}:${item}`
      leaves.push({
        id: leafId,
        label: truncateGraphLabel(humanizeSourceItem(item)),
        detail: `${source.label} - ${humanizeSourceItem(item)}`,
        kind: 'integration',
        strength: connected ? 0.7 : 0.45,
        connected,
        color: lightenHex(hue, 0.56),
        sizeWeight: 0.3 + (itemIndex % 3) * 0.03,
        role: 'leaf',
        clusterKey: hubId,
      })
      edges.push({ from: hubId, to: leafId, label: 'item' })
    })
  })

  if (sourceInputs.length > CONNECTED_SOURCE_LIMIT) {
    const hubId = 'source:more'
    const remaining = sourceInputs.length - CONNECTED_SOURCE_LIMIT
    hubs.push({
      id: hubId,
      label: `${remaining} more systems`,
      detail: 'Additional connected sources',
      kind: 'integration',
      strength: 0.66,
      connected: true,
      color: hueForKey(hubId),
      sizeWeight: 1.1,
      role: 'hub',
      clusterKey: hubId,
    })
    edges.push({ from: hubId, to: 'knowledge-base', label: 'sync' })
  }

  return { hubs, leaves, edges }
}

function buildKnowledgeCluster(items: readonly GraphDisplayNode[]) {
  if (items.length === 0) {
    return {
      hub: undefined as SourceGraphVisualNode | undefined,
      leaves: [] as SourceGraphVisualNode[],
      edges: [] as SourceGraphVisualEdge[],
    }
  }

  const hubId = 'knowledge-hub'
  const hue = hueForKey(hubId)
  const hub: SourceGraphVisualNode = {
    id: hubId,
    label: 'Indexed knowledge',
    detail: `${items.length} evidence ${items.length === 1 ? 'node' : 'nodes'}`,
    kind: 'knowledge',
    strength: 0.7,
    connected: true,
    color: hue,
    sizeWeight: 1.05 + Math.min(items.length, 8) * 0.11,
    role: 'hub',
    clusterKey: hubId,
  }

  const leaves = items.map<SourceGraphVisualNode>((node) => ({
    id: `knowledge:${node.id}`,
    label: truncateGraphLabel(node.label),
    detail: node.group === 'integration' ? 'Imported integration record' : 'Indexed knowledge node',
    kind: 'knowledge',
    strength: 0.62,
    connected: true,
    color: lightenHex(hue, 0.56),
    sizeWeight: 0.32,
    role: 'leaf',
    clusterKey: hubId,
  }))

  const edges = leaves.map((leaf) => ({ from: hubId, to: leaf.id, label: 'evidence' }))

  return { hub, leaves, edges }
}

function buildSignalNodes() {
  return [
    {
      node: {
        id: 'shared-inbox',
        label: 'Shared inbox',
        detail: 'Human handoff and conversations',
        kind: 'signal' as const,
        strength: 0.64,
        connected: true,
        color: hueForKey('shared-inbox'),
        sizeWeight: 0.82,
        role: 'standalone' as const,
        clusterKey: 'shared-inbox',
      },
      edgeLabel: 'handoff',
    },
    {
      node: {
        id: 'automation',
        label: 'Routing automation',
        detail: 'Intent, policy and workflow signals',
        kind: 'signal' as const,
        strength: 0.68,
        connected: true,
        color: hueForKey('automation'),
        sizeWeight: 0.82,
        role: 'standalone' as const,
        clusterKey: 'automation',
      },
      edgeLabel: 'routing',
    },
    {
      node: {
        id: 'answers',
        label: 'Grounded answers',
        detail: 'Customer replies with source traces',
        kind: 'signal' as const,
        strength: 0.82,
        connected: true,
        color: hueForKey('answers'),
        sizeWeight: 0.9,
        role: 'standalone' as const,
        clusterKey: 'answers',
      },
      edgeLabel: 'grounding',
    },
  ]
}

function humanizeSourceItem(value: string): string {
  return value
    .split('_')
    .map((word) => word.charAt(0).toUpperCase() + word.slice(1))
    .join(' ')
}

function totalConnectedSources(connectedSources: OnboardingState['connectors']) {
  if (connectedSources.length === 0) {
    return onboardingConnectorOptions
      .filter((option) => FALLBACK_CONNECTOR_IDS.includes(option.id))
      .reduce((total, option) => total + option.sources.length, 0)
  }

  return connectedSources.reduce((total, source) => total + (source.sourceCount ?? source.sources?.length ?? 1), 0)
}

function graphOrgLabel(nodes: readonly GraphDisplayNode[]) {
  return nodes.find((node) => node.group === 'org')?.label
}

function formatWebsiteLabel(websiteUrl?: string) {
  const trimmed = websiteUrl?.trim()
  if (!trimmed) return 'Company website'

  try {
    const withProtocol = /^[a-z][a-z\d+.-]*:\/\//i.test(trimmed) ? trimmed : `https://${trimmed}`
    return new URL(withProtocol).hostname.replace(/^www\./i, '') || trimmed
  } catch {
    return trimmed.replace(/^https?:\/\//i, '').replace(/^www\./i, '') || 'Company website'
  }
}

function getReducedMotionQuery() {
  if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') return undefined
  return window.matchMedia('(prefers-reduced-motion: reduce)')
}

function nodeKindLabel(kind: SourceGraphVisualNode['kind']) {
  if (kind === 'core') return 'Knowledge base'
  if (kind === 'integration') return 'Integration'
  if (kind === 'service') return 'Service'
  if (kind === 'signal') return 'Output'
  return 'Knowledge node'
}
