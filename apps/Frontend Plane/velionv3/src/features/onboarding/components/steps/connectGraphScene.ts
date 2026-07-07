export type SourceGraphNodeKind = 'core' | 'integration' | 'service' | 'knowledge' | 'signal'

export type SourceGraphNodeRole = 'core' | 'hub' | 'leaf' | 'standalone'

export type SourceGraphVisualNode = {
  id: string
  label: string
  detail: string
  kind: SourceGraphNodeKind
  strength: number
  connected: boolean
  color: string
  sizeWeight: number
  role: SourceGraphNodeRole
  clusterKey: string
}

export type SourceGraphVisualEdge = {
  from: string
  to: string
  label: string
}

export type SourceGraphPickResult = {
  node: SourceGraphVisualNode
  x: number
  y: number
}

export type SourceGraphSceneController = {
  setData: (nodes: readonly SourceGraphVisualNode[], edges: readonly SourceGraphVisualEdge[]) => void
  setActiveNode: (nodeId?: string) => void
  pick: () => SourceGraphPickResult | undefined
  zoom: (direction: 1 | -1) => number
  reset: () => number
  zoomPercent: () => number
  dispose: () => void
}

type GraphSceneOptions = {
  reducedMotion: boolean
  onHoverNode?: (node?: SourceGraphVisualNode) => void
  onSelectNode?: (pick?: SourceGraphPickResult) => void
}

type FallbackNode = SourceGraphVisualNode & {
  radius: number
  x: number
  y: number
  z: number
  screenX: number
  screenY: number
  screenRadius: number
  screenDepth: number
}

type FallbackEdge = SourceGraphVisualEdge & {
  source: FallbackNode
  target: FallbackNode
}

type WebglSceneModule = {
  createConnectGraphScene?: (
    graphElement: HTMLElement,
    host: HTMLElement,
    options: GraphSceneOptions,
  ) => SourceGraphSceneController | undefined
}

const DEFAULT_ZOOM = 1
const MIN_ZOOM = 0.72
const MAX_ZOOM = 1.55
const MAX_DEVICE_PIXEL_RATIO = 1.75
const FALLBACK_GRAPH_RADIUS = 172
const FALLBACK_CAMERA_DISTANCE = 520

export const GRAPH_CORE_COLOR = '#f7f4ee'

const GRAPH_HUE_PALETTE: readonly string[] = [
  '#ee7a50', // velion coral / accent
  '#4f7df3', // velion blue-grey / linear blue
  '#53b75a', // agent-role ecommerce green
  '#b94f9b', // agent-role workflow plum
  '#dd7a1f', // warning amber
  '#29404a', // velion teal-deep ink
  '#3578f6', // info blue
  '#793819', // velion earth
]

export function hueForKey(key: string): string {
  return GRAPH_HUE_PALETTE[hashString(key) % GRAPH_HUE_PALETTE.length] ?? '#ee7a50'
}

export function lightenHex(hexColor: string, amount: number): string {
  const { red, green, blue } = hexToRgb(hexColor)
  const mix = (channel: number) => Math.round(channel + (255 - channel) * amount)
  return `#${[mix(red), mix(green), mix(blue)].map((value) => value.toString(16).padStart(2, '0')).join('')}`
}

function hashString(key: string) {
  let hash = 0
  for (let index = 0; index < key.length; index += 1) {
    hash = (hash * 31 + key.charCodeAt(index)) >>> 0
  }
  return hash
}

function hexToRgb(hexColor: string) {
  const normalized = hexColor.replace('#', '')
  return {
    red: parseInt(normalized.slice(0, 2), 16),
    green: parseInt(normalized.slice(2, 4), 16),
    blue: parseInt(normalized.slice(4, 6), 16),
  }
}

const LINK_COLOR = 'rgba(247, 200, 168, 0.26)'
const LINK_HIGHLIGHT_COLOR = 'rgba(238, 122, 80, 0.78)'

export function createConnectGraphScene(
  graphElement: HTMLElement,
  host: HTMLElement,
  options: GraphSceneOptions,
): SourceGraphSceneController | undefined {
  if (typeof window === 'undefined') return undefined
  if (window.navigator.userAgent.toLowerCase().includes('jsdom')) return undefined

  let disposed = false
  let current = createCanvasGraphScene(graphElement, host, options)
  let latestNodes: readonly SourceGraphVisualNode[] = []
  let latestEdges: readonly SourceGraphVisualEdge[] = []
  let latestActiveNodeId: string | undefined

  void loadWebglSceneFactory().then((factory) => {
    if (disposed || !factory) return

    const fallback = current

    try {
      const webgl = factory(graphElement, host, options)
      if (!webgl) return

      current = webgl
      fallback.dispose()
      current.setData(latestNodes, latestEdges)
      current.setActiveNode(latestActiveNodeId)
    } catch {
      fallback.dispose()
      if (disposed) return
      current = createCanvasGraphScene(graphElement, host, options)
      current.setData(latestNodes, latestEdges)
      current.setActiveNode(latestActiveNodeId)
    }
  })

  return {
    setData(nodes, edges) {
      latestNodes = nodes
      latestEdges = edges
      current.setData(nodes, edges)
    },
    setActiveNode(nodeId) {
      latestActiveNodeId = nodeId
      current.setActiveNode(nodeId)
    },
    pick() {
      return current.pick()
    },
    zoom(direction) {
      return current.zoom(direction)
    },
    reset() {
      return current.reset()
    },
    zoomPercent() {
      return current.zoomPercent()
    },
    dispose() {
      disposed = true
      current.dispose()
    },
  }
}

function createCanvasGraphScene(
  graphElement: HTMLElement,
  host: HTMLElement,
  options: GraphSceneOptions,
): SourceGraphSceneController {
  const canvas = document.createElement('canvas')
  const context = canvas.getContext('2d', { alpha: true })
  const resizeObserver = typeof ResizeObserver === 'undefined' ? undefined : new ResizeObserver(resize)
  let nodes: FallbackNode[] = []
  let edges: FallbackEdge[] = []
  let activeNodeId: string | undefined
  let hoveredNodeId: string | undefined
  let animationFrame: number | undefined
  let width = 1
  let height = 1
  let angle = 0
  let zoom = DEFAULT_ZOOM
  let disposed = false

  canvas.setAttribute('aria-hidden', 'true')
  graphElement.replaceChildren(canvas)

  const handlePointerMove = (event: PointerEvent | MouseEvent) => {
    const node = nodeFromPointer(event)
    setHoveredNode(node?.id)
  }

  const handleClick = (event: PointerEvent | MouseEvent) => {
    const node = nodeFromPointer(event)
    if (node) {
      selectNode(node)
    } else {
      clearSelection()
    }
  }

  graphElement.addEventListener('pointermove', handlePointerMove)
  graphElement.addEventListener('mousemove', handlePointerMove)
  graphElement.addEventListener('click', handleClick)
  window.addEventListener('resize', resize)
  resizeObserver?.observe(host)
  resize()
  renderLoop()

  return {
    setData(nextNodes, nextEdges) {
      const graphData = buildFallbackGraphData(nextNodes, nextEdges)
      nodes = graphData.nodes
      edges = graphData.edges
      if (activeNodeId && !nodes.some((node) => node.id === activeNodeId)) {
        activeNodeId = undefined
        options.onSelectNode?.(undefined)
      }
      draw()
    },
    setActiveNode(nodeId) {
      activeNodeId = nodeId
      const node = nodeId ? nodes.find((item) => item.id === nodeId) : undefined
      options.onSelectNode?.(node ? projectNode(node) : undefined)
      draw()
    },
    pick() {
      const node = activeNodeId ? nodes.find((item) => item.id === activeNodeId) : undefined
      return node ? projectNode(node) : undefined
    },
    zoom(direction) {
      zoom = clamp(zoom + direction * 0.12, MIN_ZOOM, MAX_ZOOM)
      draw()
      return toZoomPercent(zoom)
    },
    reset() {
      zoom = DEFAULT_ZOOM
      angle = 0
      draw()
      return toZoomPercent(zoom)
    },
    zoomPercent() {
      return toZoomPercent(zoom)
    },
    dispose() {
      disposed = true
      if (animationFrame !== undefined) window.cancelAnimationFrame(animationFrame)
      resizeObserver?.disconnect()
      window.removeEventListener('resize', resize)
      graphElement.removeEventListener('pointermove', handlePointerMove)
      graphElement.removeEventListener('mousemove', handlePointerMove)
      graphElement.removeEventListener('click', handleClick)
      if (canvas.parentElement === graphElement) canvas.remove()
    },
  }

  function resize() {
    const bounds = host.getBoundingClientRect()
    width = Math.max(320, Math.floor(bounds.width || host.clientWidth || 640))
    height = Math.max(360, Math.floor(bounds.height || host.clientHeight || 600))
    const pixelRatio = Math.min(window.devicePixelRatio || 1, MAX_DEVICE_PIXEL_RATIO)
    canvas.width = Math.floor(width * pixelRatio)
    canvas.height = Math.floor(height * pixelRatio)
    canvas.style.width = `${width}px`
    canvas.style.height = `${height}px`
    context?.setTransform(pixelRatio, 0, 0, pixelRatio, 0, 0)
    draw()
  }

  function renderLoop() {
    if (disposed) return
    if (!options.reducedMotion) angle += 0.0028
    draw()
    animationFrame = window.requestAnimationFrame(renderLoop)
  }

  function draw() {
    if (!context) return
    context.clearRect(0, 0, width, height)
    projectNodes()
    drawEdges()
    drawNodes()
  }

  function projectNodes() {
    const tilt = -0.24
    const sinY = Math.sin(angle)
    const cosY = Math.cos(angle)
    const sinX = Math.sin(tilt)
    const cosX = Math.cos(tilt)

    for (const node of nodes) {
      const rotatedX = node.x * cosY - node.z * sinY
      const rotatedZ = node.x * sinY + node.z * cosY
      const rotatedY = node.y * cosX - rotatedZ * sinX
      const depth = node.y * sinX + rotatedZ * cosX
      const perspective = FALLBACK_CAMERA_DISTANCE / (FALLBACK_CAMERA_DISTANCE - depth)
      node.screenX = width / 2 + rotatedX * perspective * zoom
      node.screenY = height / 2 + rotatedY * perspective * zoom * 0.92
      node.screenRadius = node.radius * perspective * zoom
      node.screenDepth = depth
    }
  }

  function drawEdges() {
    const focusId = activeNodeId ?? hoveredNodeId

    for (const edge of edges) {
      const highlighted = Boolean(
        focusId && (edge.source.id === focusId || edge.target.id === focusId),
      )
      const dimmed = Boolean(focusId && !highlighted)

      context!.beginPath()
      context!.moveTo(edge.source.screenX, edge.source.screenY)
      context!.lineTo(edge.target.screenX, edge.target.screenY)
      context!.lineWidth = highlighted ? 1.8 : 0.72
      context!.strokeStyle = dimmed ? 'rgba(247, 200, 168, 0.08)' : highlighted ? LINK_HIGHLIGHT_COLOR : LINK_COLOR
      context!.stroke()
    }
  }

  function drawNodes() {
    const sortedNodes = [...nodes].sort((left, right) => left.screenDepth - right.screenDepth)

    for (const node of sortedNodes) {
      const state = nodeState(node.id)
      const radius = Math.max(3.6, node.screenRadius * (state.active ? 1.18 : state.hovered ? 1.08 : 1))
      const alpha = state.dimmed ? 0.3 : node.connected ? 0.95 : 0.64
      const color = state.highlighted ? '#f7f4ee' : node.color

      drawGlow(node, radius, color, state.highlighted ? 0.34 : 0.16)
      context!.beginPath()
      context!.arc(node.screenX, node.screenY, radius, 0, Math.PI * 2)
      context!.fillStyle = withAlpha(color, alpha)
      context!.fill()
      context!.lineWidth = state.highlighted ? 1.4 : 0.7
      context!.strokeStyle = state.active ? 'rgba(238, 122, 80, 0.88)' : 'rgba(247, 244, 238, 0.42)'
      context!.stroke()

      if (state.highlighted || node.role === 'core') {
        drawLabel(node, radius, state)
      }
    }
  }

  function drawGlow(node: FallbackNode, radius: number, color: string, opacity: number) {
    const gradient = context!.createRadialGradient(
      node.screenX,
      node.screenY,
      0,
      node.screenX,
      node.screenY,
      radius * 4.8,
    )
    gradient.addColorStop(0, withAlpha(color, opacity))
    gradient.addColorStop(1, 'rgba(0, 0, 0, 0)')
    context!.fillStyle = gradient
    context!.beginPath()
    context!.arc(node.screenX, node.screenY, radius * 4.8, 0, Math.PI * 2)
    context!.fill()
  }

  function drawLabel(
    node: FallbackNode,
    radius: number,
    state: { active: boolean; hovered: boolean; highlighted: boolean; dimmed: boolean },
  ) {
    const label = truncateLabel(node.label)
    const fontSize = node.kind === 'core' ? 13 : 11
    context!.font = `700 ${fontSize}px Inter, ui-sans-serif, system-ui`
    const labelWidth = Math.min(context!.measureText(label).width, 184)
    const boxWidth = labelWidth + 18
    const boxHeight = node.kind === 'core' ? 34 : 26
    const x = clamp(node.screenX + radius + 8, 12, width - boxWidth - 12)
    const y = clamp(node.screenY - boxHeight / 2, 54, height - boxHeight - 12)

    roundRect(context!, x, y, boxWidth, boxHeight, 7)
    context!.fillStyle = state.active ? 'rgba(41, 64, 74, 0.86)' : 'rgba(3, 6, 7, 0.74)'
    context!.fill()
    context!.strokeStyle = state.active ? 'rgba(238, 122, 80, 0.72)' : 'rgba(247, 200, 168, 0.26)'
    context!.lineWidth = 1
    context!.stroke()
    context!.fillStyle = state.dimmed ? 'rgba(247, 244, 238, 0.38)' : 'rgba(247, 244, 238, 0.92)'
    context!.fillText(label, x + 9, y + boxHeight / 2 + fontSize * 0.34, boxWidth - 18)
  }

  function nodeFromPointer(event: PointerEvent | MouseEvent) {
    const bounds = canvas.getBoundingClientRect()
    const pointerX = event.clientX - bounds.left
    const pointerY = event.clientY - bounds.top

    return [...nodes]
      .sort((left, right) => right.screenDepth - left.screenDepth)
      .find((node) => {
        const hitRadius = Math.max(12, node.screenRadius + 5)
        return Math.hypot(node.screenX - pointerX, node.screenY - pointerY) <= hitRadius
      })
  }

  function setHoveredNode(nodeId: string | undefined) {
    if (hoveredNodeId === nodeId) return
    hoveredNodeId = nodeId
    graphElement.classList.toggle('onboarding-source-graph__engine--hovering', Boolean(nodeId))
    options.onHoverNode?.(nodeId ? nodes.find((node) => node.id === nodeId) : undefined)
    draw()
  }

  function selectNode(node: FallbackNode) {
    activeNodeId = node.id
    options.onSelectNode?.(projectNode(node))
    draw()
  }

  function clearSelection() {
    activeNodeId = undefined
    options.onSelectNode?.(undefined)
    draw()
  }

  function projectNode(node: FallbackNode): SourceGraphPickResult {
    return { node, x: node.screenX || width / 2, y: node.screenY || height / 2 }
  }

  function nodeState(nodeId: string) {
    const highlighted = nodeId === activeNodeId || nodeId === hoveredNodeId || isNeighborOfFocus(nodeId)
    return {
      active: nodeId === activeNodeId,
      hovered: nodeId === hoveredNodeId,
      highlighted,
      dimmed: Boolean(activeNodeId || hoveredNodeId) && !highlighted,
    }
  }

  function isNeighborOfFocus(nodeId: string) {
    const focusId = activeNodeId ?? hoveredNodeId
    if (!focusId) return false
    return edges.some(
      (edge) =>
        (edge.source.id === focusId && edge.target.id === nodeId) ||
        (edge.target.id === focusId && edge.source.id === nodeId),
    )
  }
}

function buildFallbackGraphData(
  visualNodes: readonly SourceGraphVisualNode[],
  visualEdges: readonly SourceGraphVisualEdge[],
) {
  const nodes = visualNodes.map<FallbackNode>((node, index) => {
    const position = fallbackSpherePosition(node, index)
    return {
      ...node,
      ...position,
      radius: nodeRadius(node),
      screenX: 0,
      screenY: 0,
      screenRadius: 0,
      screenDepth: 0,
    }
  })

  const nodeById = new Map(nodes.map((node) => [node.id, node]))
  const edges = visualEdges.flatMap<FallbackEdge>((edge) => {
    const source = nodeById.get(edge.from)
    const target = nodeById.get(edge.to)
    return source && target ? [{ ...edge, source, target }] : []
  })

  return { nodes, edges }
}

function fallbackSpherePosition(node: SourceGraphVisualNode, index: number) {
  if (node.role === 'core') return { x: 0, y: 0, z: 0 }

  const goldenAngle = Math.PI * (3 - Math.sqrt(5))
  const clusterHash = hashString(node.clusterKey)
  const isLeaf = node.role === 'leaf'
  const leafHash = hashString(node.id)
  const y = clamp(
    ((clusterHash % 1000) / 1000) * 2 - 1 + (isLeaf ? ((leafHash % 1000) / 1000 - 0.5) * 0.12 : 0),
    -1,
    1,
  )
  const ringRadius = Math.sqrt(Math.max(0, 1 - y * y))
  const clusterTheta = (((clusterHash >> 10) % 1000) / 1000) * Math.PI * 2
  const spread = isLeaf ? ((leafHash >> 6) % 1000) / 1000 - 0.5 : 0
  const theta = clusterTheta + spread * 0.34 + index * goldenAngle * 0.015
  const shellRatio = isLeaf ? 0.8 : 0.98
  const radius = FALLBACK_GRAPH_RADIUS * shellRatio * (0.72 + node.strength * 0.28)

  return {
    x: Math.cos(theta) * ringRadius * radius,
    y: y * radius,
    z: Math.sin(theta) * ringRadius * radius,
  }
}

function nodeRadius(node: SourceGraphVisualNode) {
  if (node.role === 'core') return 22
  return clamp(4.6 + node.sizeWeight * 9, 4.6, 34)
}

async function loadWebglSceneFactory() {
  if (!hasWebglSupport()) return undefined

  try {
    const module = (await import('./connectGraphScene.webgl')) as WebglSceneModule
    return module.createConnectGraphScene
  } catch {
    return undefined
  }
}

function hasWebglSupport() {
  try {
    const canvas = document.createElement('canvas')
    return Boolean(canvas.getContext('webgl2') || canvas.getContext('webgl'))
  } catch {
    return false
  }
}

function withAlpha(hexColor: string, alpha: number) {
  const { red, green, blue } = hexToRgb(hexColor)
  return `rgba(${red}, ${green}, ${blue}, ${alpha})`
}

function roundRect(
  context: CanvasRenderingContext2D,
  x: number,
  y: number,
  width: number,
  height: number,
  radius: number,
) {
  context.beginPath()
  context.moveTo(x + radius, y)
  context.arcTo(x + width, y, x + width, y + height, radius)
  context.arcTo(x + width, y + height, x, y + height, radius)
  context.arcTo(x, y + height, x, y, radius)
  context.arcTo(x, y, x + width, y, radius)
  context.closePath()
}

function truncateLabel(label: string) {
  return label.length > 26 ? `${label.slice(0, 23)}...` : label
}

function toZoomPercent(zoom: number) {
  return Math.round(zoom * 100)
}

function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value))
}
