// 2D knowledge-graph scene: pill-shaped nodes on a dotted canvas, straight
// animated edges with arrowheads — matches the Polygres reference style
// (`react-flow` visuals) exactly, but hand-built on `d3-force-3d` + plain
// DOM/SVG so no React dependency enters this SolidJS app. Same
// `SourceGraphSceneController` contract as `connectGraphScene.ts`'s 3D
// scene, so `GraphPanel` only needed a one-line swap to use this instead.
import {
  forceCenter,
  forceCollide,
  forceLink,
  forceManyBody,
  forceSimulation,
  type ForceSimulation,
} from 'd3-force-3d'
import type {
  SourceGraphPickResult,
  SourceGraphSceneController,
  SourceGraphVisualEdge,
  SourceGraphVisualNode,
} from '@/features/onboarding/components/steps/connectGraphScene'

type GraphSceneOptions = {
  reducedMotion: boolean
  onHoverNode?: (node?: SourceGraphVisualNode) => void
  onSelectNode?: (pick?: SourceGraphPickResult) => void
  onZoomChange?: (zoomPercent: number) => void
}

type SimNode = SourceGraphVisualNode & {
  x: number
  y: number
  vx: number
  vy: number
  fx: number | null
  fy: number | null
}

type SimLink = {
  source: string | SimNode
  target: string | SimNode
  label: string
}

const DEFAULT_ZOOM = 1
const MIN_ZOOM = 0.4
const MAX_ZOOM = 2.5
const ZOOM_STEP = 1.2
const DOT_SPACING = 22
const CLICK_MOVE_THRESHOLD = 4

export function createKnowledgeGraph2DScene(
  graphElement: HTMLElement,
  host: HTMLElement,
  options: GraphSceneOptions,
): SourceGraphSceneController {
  graphElement.classList.add('kg2d')
  graphElement.innerHTML = ''

  const background = createSvg('kg2d__background')
  background.innerHTML = `
    <defs>
      <pattern id="kg2d-dots" width="${DOT_SPACING}" height="${DOT_SPACING}" patternUnits="userSpaceOnUse">
        <circle cx="1.4" cy="1.4" r="1.4" class="kg2d__dot" />
      </pattern>
    </defs>
    <rect width="100%" height="100%" fill="url(#kg2d-dots)" />
  `
  const dotPattern = background.querySelector('pattern') as SVGPatternElement | null

  const viewport = document.createElement('div')
  viewport.className = 'kg2d__viewport'

  const edgesSvg = createSvg('kg2d__edges')
  edgesSvg.innerHTML = `
    <defs>
      <marker id="kg2d-arrow" markerWidth="8" markerHeight="8" viewBox="-5 -5 10 10"
        markerUnits="strokeWidth" orient="auto-start-reverse" refX="0" refY="0">
        <polyline points="-4,-3.2 0,0 -4,3.2" class="kg2d__arrowhead" />
      </marker>
    </defs>
  `
  const nodesLayer = document.createElement('div')
  nodesLayer.className = 'kg2d__nodes'

  viewport.append(edgesSvg, nodesLayer)
  graphElement.append(background, viewport)

  let simNodes: SimNode[] = []
  let simLinks: SimLink[] = []
  let nodeEls = new Map<string, HTMLDivElement>()
  let edgeEls = new Map<SourceGraphVisualEdge, SVGPathElement>()
  let activeNodeId: string | undefined
  let hoveredNode: SourceGraphVisualNode | undefined
  // Simulation space is centered on its own (0, 0); offsetting the initial
  // pan by half the container puts that origin at the container's visual
  // center instead of its top-left corner.
  let pan = centerPan()
  let scale = DEFAULT_ZOOM
  let disposed = false

  function centerPan(): { x: number; y: number } {
    const rect = host.getBoundingClientRect()
    return { x: rect.width / 2, y: rect.height / 2 }
  }

  const linkForce = forceLink<SimNode, SimLink>([]).id((node) => node.id).distance(120).strength(0.6)
  const chargeForce = forceManyBody<SimNode>().strength(-260).distanceMax(520)
  const collideForce = forceCollide<SimNode>((node) => nodeHalfWidth(node) + 14).strength(0.9)
  const centerForce = forceCenter<SimNode>(0, 0).strength(0.05)

  const simulation: ForceSimulation<SimNode> = forceSimulation<SimNode>([])
    .force('link', linkForce)
    .force('charge', chargeForce)
    .force('collide', collideForce)
    .force('center', centerForce)
    .alphaDecay(options.reducedMotion ? 0.08 : 0.028)
    .velocityDecay(0.35)
    .on('tick', renderPositions)

  const resizeObserver = typeof ResizeObserver === 'undefined'
    ? undefined
    : new ResizeObserver(() => {
        centerForce.x(0).y(0)
        applyTransform()
      })
  resizeObserver?.observe(host)

  function nodeHalfWidth(node: SourceGraphVisualNode): number {
    // Rough pill half-width from label length — good enough for collision
    // padding; exact width comes from the real DOM element's layout.
    return Math.max(28, Math.min(90, 18 + node.label.length * 3.2))
  }

  function applyTransform() {
    viewport.style.transform = `translate(${pan.x}px, ${pan.y}px) scale(${scale})`
    const offsetX = ((pan.x / scale) % DOT_SPACING) + DOT_SPACING
    const offsetY = ((pan.y / scale) % DOT_SPACING) + DOT_SPACING
    dotPattern?.setAttribute(
      'patternTransform',
      `translate(${offsetX % DOT_SPACING}, ${offsetY % DOT_SPACING}) scale(${1})`,
    )
  }

  function renderPositions() {
    for (const node of simNodes) {
      const el = nodeEls.get(node.id)
      // The pill is variable-width (label length), so center it on (x, y) by
      // first shifting -50%/-50% of its OWN box (a CSS-relative offset) and
      // only then translating to the absolute simulation coordinate.
      if (el) el.style.transform = `translate(-50%, -50%) translate(${node.x}px, ${node.y}px)`
    }
    for (const edge of simLinks) {
      const source = typeof edge.source === 'string' ? undefined : edge.source
      const target = typeof edge.target === 'string' ? undefined : edge.target
      if (!source || !target) continue
      const original = findOriginalEdge(source.id, target.id)
      const path = original ? edgeEls.get(original) : undefined
      path?.setAttribute('d', `M${source.x},${source.y} L${target.x},${target.y}`)
    }
  }

  let originalEdges: readonly SourceGraphVisualEdge[] = []
  function findOriginalEdge(fromId: string, toId: string): SourceGraphVisualEdge | undefined {
    return originalEdges.find((edge) => edge.from === fromId && edge.to === toId)
  }

  function setData(nodes: readonly SourceGraphVisualNode[], edges: readonly SourceGraphVisualEdge[]) {
    if (disposed) return
    originalEdges = edges
    const previous = new Map(simNodes.map((node) => [node.id, node]))
    simNodes = nodes.map((node) => {
      const prior = previous.get(node.id)
      return {
        ...node,
        x: prior?.x ?? (Math.random() - 0.5) * 200,
        y: prior?.y ?? (Math.random() - 0.5) * 200,
        vx: prior?.vx ?? 0,
        vy: prior?.vy ?? 0,
        fx: prior?.fx ?? null,
        fy: prior?.fy ?? null,
      }
    })
    simLinks = edges.map((edge) => ({ source: edge.from, target: edge.to, label: edge.label }))

    rebuildDom()

    simulation.nodes(simNodes)
    linkForce.links(simLinks)
    simulation.alpha(0.9).restart()
  }

  function rebuildDom() {
    for (const el of nodeEls.values()) el.remove()
    for (const el of edgeEls.values()) el.remove()
    nodeEls = new Map()
    edgeEls = new Map()

    for (const edge of originalEdges) {
      const path = document.createElementNS('http://www.w3.org/2000/svg', 'path')
      path.setAttribute('class', 'kg2d__edge-path')
      path.setAttribute('marker-end', 'url(#kg2d-arrow)')
      edgesSvg.appendChild(path)
      edgeEls.set(edge, path)
    }

    for (const node of simNodes) {
      // Two layers: the outer `.kg2d__node` is a pure positioning wrapper
      // (JS drives its transform every tick), the inner `.kg2d__node-pill`
      // carries all visual styling — an element can't have both a JS-driven
      // inline `transform` (position) and a CSS `:hover` transform (scale)
      // at once, since the inline style always wins.
      const el = document.createElement('div')
      el.className = 'kg2d__node'
      el.dataset.nodeId = node.id
      el.innerHTML = `
        <div class="kg2d__node-pill" role="button" tabindex="0" aria-label="${escapeHtml(node.label)}">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"
            stroke-linecap="round" stroke-linejoin="round" class="kg2d__node-icon" aria-hidden="true">
            <path d="M12 7v14"></path>
            <path d="M3 18a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1h5a4 4 0 0 1 4 4 4 4 0 0 1 4-4h5a1 1 0 0 1 1 1v13a1 1 0 0 1-1 1h-6a3 3 0 0 0-3 3 3 3 0 0 0-3-3z"></path>
          </svg>
          <span class="kg2d__node-label"></span>
        </div>
      `
      if (node.id === activeNodeId) el.classList.add('kg2d__node--active')
      const label = el.querySelector('.kg2d__node-label')
      if (label) label.textContent = node.label
      const pill = el.querySelector('.kg2d__node-pill') as HTMLDivElement
      attachNodeInteractions(pill, node)
      nodesLayer.appendChild(el)
      nodeEls.set(node.id, el)
    }
  }

  function attachNodeInteractions(el: HTMLDivElement, node: SimNode) {
    let dragging = false
    let moved = false
    let startClientX = 0
    let startClientY = 0

    el.addEventListener('pointerenter', () => {
      hoveredNode = node
      options.onHoverNode?.(node)
    })
    el.addEventListener('pointerleave', () => {
      if (hoveredNode === node) hoveredNode = undefined
      options.onHoverNode?.(undefined)
    })
    el.addEventListener('pointerdown', (event) => {
      event.stopPropagation()
      dragging = true
      moved = false
      startClientX = event.clientX
      startClientY = event.clientY
      el.setPointerCapture(event.pointerId)
    })
    el.addEventListener('pointermove', (event) => {
      if (!dragging) return
      const dx = (event.clientX - startClientX) / scale
      const dy = (event.clientY - startClientY) / scale
      if (Math.abs(event.clientX - startClientX) > CLICK_MOVE_THRESHOLD
        || Math.abs(event.clientY - startClientY) > CLICK_MOVE_THRESHOLD) {
        moved = true
      }
      node.fx = node.x + dx
      node.fy = node.y + dy
      node.x = node.fx
      node.y = node.fy
      renderPositions()
      if (simulation.alpha() < 0.3) simulation.alpha(0.3)
      simulation.restart()
    })
    el.addEventListener('pointerup', (event) => {
      dragging = false
      el.releasePointerCapture(event.pointerId)
      if (!moved) {
        setActiveNode(node.id)
        options.onSelectNode?.({ node, x: node.x, y: node.y })
      } else {
        simulation.alpha(0.4).restart()
      }
    })
    el.addEventListener('keydown', (event) => {
      if (event.key === 'Enter' || event.key === ' ') {
        event.preventDefault()
        setActiveNode(node.id)
        options.onSelectNode?.({ node, x: node.x, y: node.y })
      }
    })
  }

  function setActiveNode(nodeId?: string) {
    if (activeNodeId) nodeEls.get(activeNodeId)?.classList.remove('kg2d__node--active')
    activeNodeId = nodeId
    if (nodeId) nodeEls.get(nodeId)?.classList.add('kg2d__node--active')
  }

  function pick(): SourceGraphPickResult | undefined {
    if (!hoveredNode) return undefined
    const node = simNodes.find((candidate) => candidate.id === hoveredNode?.id)
    if (!node) return undefined
    return { node, x: node.x, y: node.y }
  }

  function zoomPercent(): number {
    return Math.round(scale * 100)
  }

  function setScale(next: number) {
    scale = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, next))
    applyTransform()
    options.onZoomChange?.(zoomPercent())
  }

  function zoom(direction: 1 | -1): number {
    setScale(direction > 0 ? scale * ZOOM_STEP : scale / ZOOM_STEP)
    return zoomPercent()
  }

  function reset(): number {
    pan = centerPan()
    setScale(DEFAULT_ZOOM)
    for (const node of simNodes) {
      node.fx = null
      node.fy = null
    }
    simulation.alpha(0.6).restart()
    return zoomPercent()
  }

  // Background drag pans the viewport; wheel zooms around the pointer.
  let panning = false
  let panStart = { x: 0, y: 0 }
  let panOrigin = { x: 0, y: 0 }
  graphElement.addEventListener('pointerdown', (event) => {
    if (event.target !== graphElement && event.target !== background && event.target !== viewport) return
    panning = true
    panStart = { x: event.clientX, y: event.clientY }
    panOrigin = { ...pan }
  })
  window.addEventListener('pointermove', (event) => {
    if (!panning) return
    pan = {
      x: panOrigin.x + (event.clientX - panStart.x),
      y: panOrigin.y + (event.clientY - panStart.y),
    }
    applyTransform()
  })
  window.addEventListener('pointerup', () => {
    panning = false
  })
  graphElement.addEventListener(
    'wheel',
    (event) => {
      event.preventDefault()
      const direction = event.deltaY < 0 ? 1 : -1
      setScale(direction > 0 ? scale * 1.06 : scale / 1.06)
    },
    { passive: false },
  )

  applyTransform()

  return {
    setData,
    setActiveNode,
    pick,
    zoom,
    reset,
    zoomPercent,
    dispose() {
      disposed = true
      simulation.stop()
      resizeObserver?.disconnect()
      graphElement.innerHTML = ''
      graphElement.classList.remove('kg2d')
    },
  }
}

function createSvg(className: string): SVGSVGElement {
  const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg')
  svg.setAttribute('class', className)
  return svg
}

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;')
}
