import { Minus, Plus, RotateCcw } from 'lucide-solid'
import { createEffect, createMemo, createSignal, For, on, onCleanup, onMount, Show } from 'solid-js'
import { OnboardingLinkButton } from '@/features/onboarding/components/shared/OnboardingLinkButton'
import {
  type ConnectorCategory,
  type ConnectorOption,
  type OnboardingState,
  onboardingConnectorOptions,
} from '@/features/onboarding/lib/model'
import { truncateGraphLabel } from '@/features/onboarding/lib/view'
import { Badge } from '@/shared/ui/Badge'
import { Button } from '@/shared/ui/Button'
import { VerevonIconButton } from '@/shared/ui/verevon/VerevonIconButton'
import { VerevonSelectableRow } from '@/shared/ui/verevon/VerevonSelectableRow'
import { buildSourceGraphModel } from './connectGraphModel'
import {
  createConnectGraphScene,
  type SourceGraphSceneController,
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
      <p>Velg systemene Verevon skal lære fra, svare på vegne av, eller bruke som signaler for automasjon.</p>

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
                <VerevonSelectableRow
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
  connectedSources: OnboardingState['connectors']
  organizationName: string
  currentUserId: string
  currentUserName: string
}

export function ConnectStepVisual(props: ConnectStepVisualProps) {
  let hostRef!: HTMLDivElement
  let graphRef!: HTMLDivElement
  let sceneController: SourceGraphSceneController | undefined
  const [graphReady, setGraphReady] = createSignal(true)
  const [zoomPercent, setZoomPercent] = createSignal(100)
  const [hoveringNode, setHoveringNode] = createSignal(false)
  const [selectedNode, setSelectedNode] = createSignal<SourceGraphVisualNode>()
  const [keyboardNodeIndex, setKeyboardNodeIndex] = createSignal(0)
  const graphModel = createMemo(() =>
    buildSourceGraphModel({
      connectedSources: props.connectedSources,
      organizationName: props.organizationName,
      currentUserId: props.currentUserId,
      currentUserName: props.currentUserName,
    }),
  )
  const hasGraphNodes = createMemo(() => graphModel().nodes.length > 0)

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
        onZoomChange: setZoomPercent,
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

  createEffect(
    on(graphModel, (model) => {
      sceneController?.setData(model.nodes, model.edges)
      const activeNode = selectedNode()
      if (activeNode && !model.nodes.some((node) => node.id === activeNode.id)) {
        setSelectedNode(undefined)
        sceneController?.setActiveNode(undefined)
      }
    }),
  )

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

  const keyboardNodes = createMemo(() => graphModel().nodes.filter((node) => node.role !== 'leaf'))
  const selectKeyboardNode = (index: number) => {
    const nodes = keyboardNodes()
    if (nodes.length === 0) return
    const normalizedIndex = (index + nodes.length) % nodes.length
    const node = nodes[normalizedIndex]
    if (!node) return
    setKeyboardNodeIndex(normalizedIndex)
    setSelectedNode(node)
    sceneController?.setActiveNode(node.id)
  }
  const handleGraphKeyDown = (event: KeyboardEvent) => {
    if (event.key === 'Escape') {
      event.preventDefault()
      closeSelectedCard()
      return
    }
    if (event.key === 'Home' || event.key === 'End') {
      event.preventDefault()
      selectKeyboardNode(event.key === 'Home' ? 0 : keyboardNodes().length - 1)
      return
    }
    if (['ArrowRight', 'ArrowDown', 'ArrowLeft', 'ArrowUp'].includes(event.key)) {
      event.preventDefault()
      selectKeyboardNode(keyboardNodeIndex() + (event.key === 'ArrowRight' || event.key === 'ArrowDown' ? 1 : -1))
      return
    }
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault()
      selectKeyboardNode(keyboardNodeIndex())
    }
  }

  return (
    <div
      ref={hostRef}
      class="onboarding-source-graph"
      classList={{
        'onboarding-source-graph--empty': !hasGraphNodes(),
        'onboarding-source-graph--fallback': !graphReady(),
      }}
      aria-label="Integration knowledge graph"
      role="region"
    >
      <p id="integration-graph-instructions" class="sr-only">
        Rotate, pan, or zoom the graph with a pointer. Use arrow keys to inspect major nodes and Escape to clear selection.
      </p>
      <div
        ref={graphRef}
        class="onboarding-source-graph__engine"
        classList={{ 'onboarding-source-graph__engine--hovering': hoveringNode() }}
        aria-describedby="integration-graph-instructions"
        aria-label={hasGraphNodes() ? 'Interactive 3D integration graph' : 'Integration graph with no connected sources'}
        onKeyDown={handleGraphKeyDown}
        role="group"
        tabIndex={hasGraphNodes() ? 0 : -1}
      />
      <p class="sr-only" role="status" aria-live="polite">
        {selectedNode()
          ? `${nodeKindLabel(selectedNode()!.kind)} selected: ${selectedNode()!.label}. ${selectedNode()!.detail}`
          : hasGraphNodes()
            ? 'No graph node selected.'
            : 'No connected integrations. The graph is empty.'}
      </p>
      <div class="onboarding-source-graph__glow" aria-hidden="true" />
      <div class="onboarding-source-graph__scanline" aria-hidden="true" />

      <Show when={hasGraphNodes()}>
        <div class="onboarding-source-graph__controls">
          <VerevonIconButton aria-label="Zoom out" size="sm" shape="rounded" tone="inverted" onClick={() => zoom(-1)}>
            <Minus size={14} />
          </VerevonIconButton>
          <output aria-label="Graph zoom" aria-live="polite">{zoomPercent()}%</output>
          <VerevonIconButton aria-label="Zoom in" size="sm" shape="rounded" tone="inverted" onClick={() => zoom(1)}>
            <Plus size={14} />
          </VerevonIconButton>
          <VerevonIconButton aria-label="Reset graph" size="sm" shape="rounded" tone="inverted" onClick={reset}>
            <RotateCcw size={14} />
          </VerevonIconButton>
        </div>
      </Show>

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
