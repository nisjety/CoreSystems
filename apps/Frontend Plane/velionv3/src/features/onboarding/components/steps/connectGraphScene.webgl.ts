import ForceGraph3D, { type ForceGraph3DInstance } from '3d-force-graph'
import { forceCollide } from 'd3-force-3d'
import * as THREE from 'three'
import SpriteText from 'three-spritetext'

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

type ForceGraphNode = SourceGraphVisualNode & {
  x?: number
  y?: number
  z?: number
  vx?: number
  vy?: number
  vz?: number
  fx?: number
  fy?: number
  fz?: number
}

type ForceGraphLink = {
  source: string | ForceGraphNode
  target: string | ForceGraphNode
  label: string
}

type ForceWithStrength = {
  strength?: (value: number | ((node: ForceGraphNode) => number)) => ForceWithStrength
}

type LinkForce = {
  distance?: (value: number | ((link: ForceGraphLink) => number)) => LinkForce
  strength?: (value: number | ((link: ForceGraphLink) => number)) => LinkForce
}

type OrbitControlsLike = {
  autoRotate?: boolean
  autoRotateSpeed?: number
  enableDamping?: boolean
  dampingFactor?: number
}

const DEFAULT_CAMERA_DISTANCE = 360
const MIN_CAMERA_DISTANCE = 230
const MAX_CAMERA_DISTANCE = 560
const MAX_DEVICE_PIXEL_RATIO = 1.65
const GRAPH_RADIUS = 130
const POINTER_TAP_TOLERANCE = 6

const HIGHLIGHT_COLOR = '#f4f1eb'
const LINK_COLOR = 'rgba(247, 200, 168, 0.32)'
const LINK_HIGHLIGHT_COLOR = 'rgba(238, 122, 80, 0.72)'

export function createConnectGraphScene(
  graphElement: HTMLElement,
  host: HTMLElement,
  options: GraphSceneOptions,
): SourceGraphSceneController | undefined {
  if (typeof window === 'undefined') return undefined
  if (window.navigator.userAgent.toLowerCase().includes('jsdom')) return undefined

  let graph: ForceGraph3DInstance<ForceGraphNode, ForceGraphLink> | undefined
  let nodes: ForceGraphNode[] = []
  let links: ForceGraphLink[] = []
  let activeNodeId: string | undefined
  let hoveredNodeId: string | undefined
  let cameraDistance = DEFAULT_CAMERA_DISTANCE
  let disposed = false
  let pointerDownPoint: { x: number; y: number } | undefined

  graphElement.replaceChildren()

  const graphInstance = new ForceGraph3D(graphElement, {
    controlType: 'orbit',
    rendererConfig: {
      alpha: true,
      antialias: true,
      powerPreference: 'high-performance',
    },
  }) as unknown as ForceGraph3DInstance<ForceGraphNode, ForceGraphLink>
  graph = graphInstance

  graphInstance
    .backgroundColor('rgba(0,0,0,0)')
    .showNavInfo(false)
    .nodeId('id')
    .nodeRelSize(7)
    .nodeVal((node) => node.sizeWeight)
    .nodeLabel((node) => `${node.label}: ${node.detail}`)
    .nodeThreeObject((node) => {
      return createNodeObject(node, nodeVisualState(node.id))
    })
    .linkLabel((link) => link.label)
    .linkColor((link) => (isHighlightedLink(link) ? LINK_HIGHLIGHT_COLOR : LINK_COLOR))
    .linkOpacity(0.42)
    .linkWidth((link) => (isHighlightedLink(link) ? 2.2 : 0.75))
    .linkDirectionalParticles(0)
    .enableNodeDrag(false)
    .enablePointerInteraction(false)
    .showPointerCursor(() => false)
    .warmupTicks(options.reducedMotion ? 70 : 120)
    .cooldownTicks(options.reducedMotion ? 80 : 220)
    .cooldownTime(options.reducedMotion ? 1800 : 6500)
    .d3AlphaDecay(0.026)
    .d3VelocityDecay(0.34)

  const renderer = graphInstance.renderer()
  renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, MAX_DEVICE_PIXEL_RATIO))

  graphInstance.scene().fog = new THREE.FogExp2(0x080e0c, 0.0018)
  configureControls(graphInstance.controls())
  configureForces()

  const handlePointerMove = (event: MouseEvent | PointerEvent) => {
    setHoveredNode(pickNodeFromPointer(event)?.id)
  }

  const handlePointerDown = (event: MouseEvent | PointerEvent) => {
    pointerDownPoint = { x: event.clientX, y: event.clientY }
  }

  const handlePointerUp = (event: MouseEvent | PointerEvent) => {
    if (!pointerDownPoint) return
    const movement = Math.hypot(event.clientX - pointerDownPoint.x, event.clientY - pointerDownPoint.y)
    pointerDownPoint = undefined
    if (movement > POINTER_TAP_TOLERANCE) return

    const node = pickNodeFromPointer(event)
    if (node) {
      selectNode(node)
    } else {
      clearSelection()
    }
  }

  graphElement.addEventListener('mousemove', handlePointerMove)
  graphElement.addEventListener('pointermove', handlePointerMove)
  graphElement.addEventListener('pointerdown', handlePointerDown, true)
  graphElement.addEventListener('pointerup', handlePointerUp, true)

  const resize = () => {
    if (!graph || disposed) return
    const bounds = host.getBoundingClientRect()
    graph.width(Math.max(320, Math.floor(bounds.width || host.clientWidth || 640)))
    graph.height(Math.max(360, Math.floor(bounds.height || host.clientHeight || 600)))
  }

  const resizeObserver = typeof ResizeObserver === 'undefined' ? undefined : new ResizeObserver(resize)
  resizeObserver?.observe(host)
  window.addEventListener('resize', resize)
  resize()
  graphInstance.cameraPosition({ x: 0, y: 0, z: DEFAULT_CAMERA_DISTANCE }, { x: 0, y: 0, z: 0 }, 0)

  return {
    setData(nextNodes, nextEdges) {
      const graphData = buildGraphData(nextNodes, nextEdges)
      nodes = graphData.nodes
      links = graphData.links
      graph?.graphData(graphData)
      configureForces()
      refreshGraphStyles()
      window.setTimeout(() => {
        if (!disposed) graph?.zoomToFit(420, 66)
      }, 120)
    },
    setActiveNode(nodeId) {
      activeNodeId = nodeId
      refreshGraphStyles()
      const node = nodeId ? nodes.find((item) => item.id === nodeId) : undefined
      options.onSelectNode?.(node ? projectNode(node) : undefined)
    },
    pick() {
      const node = activeNodeId ? nodes.find((item) => item.id === activeNodeId) : undefined
      return node ? projectNode(node) : undefined
    },
    zoom(direction) {
      cameraDistance = clamp(cameraDistance - direction * 36, MIN_CAMERA_DISTANCE, MAX_CAMERA_DISTANCE)
      graph?.cameraPosition({ x: 0, y: 0, z: cameraDistance }, { x: 0, y: 0, z: 0 }, 260)
      return toZoomPercent(cameraDistance)
    },
    reset() {
      cameraDistance = DEFAULT_CAMERA_DISTANCE
      configureControls(graph?.controls())
      graph?.cameraPosition({ x: 0, y: 0, z: cameraDistance }, { x: 0, y: 0, z: 0 }, 360)
      graph?.d3ReheatSimulation()
      window.setTimeout(() => {
        if (!disposed) graph?.zoomToFit(360, 66)
      }, 120)
      return toZoomPercent(cameraDistance)
    },
    zoomPercent() {
      return toZoomPercent(cameraDistance)
    },
    dispose() {
      disposed = true
      resizeObserver?.disconnect()
      window.removeEventListener('resize', resize)
      graph?._destructor()
      graph = undefined
      graphElement.removeEventListener('mousemove', handlePointerMove)
      graphElement.removeEventListener('pointermove', handlePointerMove)
      graphElement.removeEventListener('pointerdown', handlePointerDown, true)
      graphElement.removeEventListener('pointerup', handlePointerUp, true)
      graphElement.replaceChildren()
    },
  }

  function setHoveredNode(nodeId: string | undefined) {
    if (hoveredNodeId === nodeId) return
    hoveredNodeId = nodeId
    options.onHoverNode?.(nodeId ? nodes.find((node) => node.id === nodeId) : undefined)
    refreshGraphStyles()
  }

  function selectNode(node: ForceGraphNode) {
    activeNodeId = node.id
    options.onSelectNode?.(projectNode(node))
    refreshGraphStyles()
    if (node.x !== undefined && node.y !== undefined && node.z !== undefined) {
      graph?.cameraPosition(
        { x: node.x * 0.38, y: node.y * 0.38, z: cameraDistance },
        { x: node.x, y: node.y, z: node.z },
        420,
      )
    }
  }

  function clearSelection() {
    activeNodeId = undefined
    options.onSelectNode?.(undefined)
    refreshGraphStyles()
  }

  function pickNodeFromPointer(event: MouseEvent | PointerEvent) {
    if (!graph) return undefined

    const bounds = graphElement.getBoundingClientRect()
    if (bounds.width <= 0 || bounds.height <= 0) return undefined

    const pointerX = event.clientX - bounds.left
    const pointerY = event.clientY - bounds.top
    let bestPick: { node: ForceGraphNode; score: number } | undefined

    for (const node of nodes) {
      const hitArea = projectNodeHitArea(node)
      if (!hitArea) continue

      const distance = Math.hypot(hitArea.x - pointerX, hitArea.y - pointerY)
      if (distance > hitArea.radius) continue

      const score = distance / hitArea.radius
      if (!bestPick || score < bestPick.score) {
        bestPick = { node, score }
      }
    }

    return bestPick?.node
  }

  function configureControls(rawControls?: object) {
    const controls = rawControls as OrbitControlsLike | undefined
    if (!controls) return
    controls.autoRotate = !options.reducedMotion
    controls.autoRotateSpeed = 0.55
    controls.enableDamping = true
    controls.dampingFactor = 0.08
  }

  function configureForces() {
    if (!graph) return

    const charge = graph.d3Force('charge') as ForceWithStrength | undefined
    charge?.strength?.((node) => (node.role === 'leaf' ? -16 : -76))

    const link = graph.d3Force('link') as LinkForce | undefined
    link?.distance?.((item) => {
      if (linkTouchesRole(item, 'leaf')) return 24
      if (linkTouchesKind(item, 'core')) return 90
      return 118
    })
    link?.strength?.(0.4)

    graph.d3Force('collide', forceCollide<ForceGraphNode>((node) => nodeRadius(node) * 1.35).strength(0.86))
    graph.d3Force('sphere', createSphereForce(GRAPH_RADIUS))
  }

  function refreshGraphStyles() {
    if (!graph) return
    graph
      .nodeThreeObject((node) => {
        return createNodeObject(node, nodeVisualState(node.id))
      })
      .linkColor((link) => (isHighlightedLink(link) ? LINK_HIGHLIGHT_COLOR : LINK_COLOR))
      .linkWidth((link) => (isHighlightedLink(link) ? 2.2 : 0.75))
      .linkDirectionalParticles(0)
      .refresh()
  }

  function nodeVisualState(nodeId: string) {
    const highlighted = nodeId === activeNodeId || nodeId === hoveredNodeId || isNeighborOfActiveNode(nodeId)
    return {
      active: nodeId === activeNodeId,
      hovered: nodeId === hoveredNodeId,
      highlighted,
      dimmed: Boolean(activeNodeId || hoveredNodeId) && !highlighted,
    }
  }

  function isNeighborOfActiveNode(nodeId: string) {
    const focusId = activeNodeId ?? hoveredNodeId
    if (!focusId) return false
    return links.some((link) => {
      const source = linkEndpointId(link.source)
      const target = linkEndpointId(link.target)
      return (source === focusId && target === nodeId) || (target === focusId && source === nodeId)
    })
  }

  function isHighlightedLink(link: ForceGraphLink) {
    const focusId = activeNodeId ?? hoveredNodeId
    if (!focusId) return false
    return linkEndpointId(link.source) === focusId || linkEndpointId(link.target) === focusId
  }

  function projectNode(node: ForceGraphNode): SourceGraphPickResult {
    if (!graph || node.x === undefined || node.y === undefined || node.z === undefined) {
      return { node, x: host.clientWidth / 2, y: host.clientHeight / 2 }
    }

    const coords = graph.graph2ScreenCoords(node.x, node.y, node.z)
    if (!coords || typeof coords.x !== 'number' || typeof coords.y !== 'number') {
      return { node, x: host.clientWidth / 2, y: host.clientHeight / 2 }
    }

    return { node, x: coords.x, y: coords.y }
  }

  function projectNodeHitArea(node: ForceGraphNode) {
    if (!graph || node.x === undefined || node.y === undefined || node.z === undefined) return undefined

    const center = graph.graph2ScreenCoords(node.x, node.y, node.z)
    if (!center || typeof center.x !== 'number' || typeof center.y !== 'number') return undefined

    const radius = nodeRadius(node) * (node.id === activeNodeId ? 1.14 : 1)
    const projectedRadii = [
      graph.graph2ScreenCoords(node.x + radius, node.y, node.z),
      graph.graph2ScreenCoords(node.x, node.y + radius, node.z),
      graph.graph2ScreenCoords(node.x, node.y, node.z + radius),
    ]
      .filter((coords): coords is { x: number; y: number; z: number } => Boolean(coords))
      .map((coords) => Math.hypot(coords.x - center.x, coords.y - center.y))

    const projectedRadius = Math.max(...projectedRadii, 7) * 0.92
    return { x: center.x, y: center.y, radius: projectedRadius }
  }
}

function buildGraphData(
  visualNodes: readonly SourceGraphVisualNode[],
  visualEdges: readonly SourceGraphVisualEdge[],
) {
  const nodes = visualNodes.map<ForceGraphNode>((node, index) => {
    const position = initialSpherePosition(node, index)
    return {
      ...node,
      ...position,
      ...(node.role === 'core' ? { fx: 0, fy: 0, fz: 0 } : {}),
    }
  })

  return {
    nodes,
    links: visualEdges.map<ForceGraphLink>((edge) => ({
      source: edge.from,
      target: edge.to,
      label: edge.label,
    })),
  }
}

function createNodeObject(
  node: ForceGraphNode,
  state: { active: boolean; hovered: boolean; highlighted: boolean; dimmed: boolean },
) {
  const group = new THREE.Group()
  const baseColor = state.highlighted ? HIGHLIGHT_COLOR : node.color
  const radius = nodeRadius(node) * (state.active ? 1.18 : state.hovered ? 1.1 : 1)
  const opacity = state.dimmed ? 0.28 : node.connected ? 0.94 : 0.58

  const halo = new THREE.Mesh(
    new THREE.SphereGeometry(radius * (state.highlighted ? 2.28 : 1.78), 24, 16),
    new THREE.MeshBasicMaterial({
      color: baseColor,
      transparent: true,
      opacity: state.highlighted ? 0.22 : 0.1,
      depthWrite: false,
      blending: THREE.AdditiveBlending,
    }),
  )
  group.add(halo)

  const sphere = new THREE.Mesh(
    new THREE.SphereGeometry(radius, 28, 18),
    new THREE.MeshBasicMaterial({
      color: baseColor,
      transparent: true,
      opacity,
      depthWrite: false,
    }),
  )
  group.add(sphere)

  if (state.highlighted || node.role === 'core') {
    const ring = new THREE.Mesh(
      new THREE.TorusGeometry(radius * 1.75, Math.max(0.08, radius * 0.055), 8, 44),
      new THREE.MeshBasicMaterial({
        color: state.active ? '#ee7a50' : baseColor,
        transparent: true,
        opacity: state.dimmed ? 0.14 : state.active ? 0.48 : 0.3,
        depthWrite: false,
        blending: THREE.AdditiveBlending,
      }),
    )
    ring.rotation.x = Math.PI / 2
    group.add(ring)
  }

  if (state.highlighted || node.role === 'core') {
    const label = new SpriteText(truncateLabel(node.label), node.role === 'core' ? 4.8 : 3.2, '#f7f4ee')
    label.fontFace = 'Inter, ui-sans-serif, system-ui'
    label.fontWeight = state.highlighted || node.role === 'core' ? '800' : '650'
    label.backgroundColor = false
    label.padding = 0.4
    label.borderRadius = 2
    label.strokeWidth = state.highlighted ? 0.8 : 0.4
    label.strokeColor = 'rgba(8, 14, 12, 0.82)'
    label.position.set(radius + 5.2, radius * 0.18, 0)
    label.material.opacity = state.dimmed ? 0.22 : node.role === 'leaf' ? 0.62 : 0.88
    group.add(label)
  }

  return group
}

function createSphereForce(radius: number) {
  let forceNodes: ForceGraphNode[] = []

  const force = (alpha: number) => {
    for (const node of forceNodes) {
      if (node.role === 'core' || node.role === 'leaf') continue

      const x = node.x ?? 0
      const y = node.y ?? 0
      const z = node.z ?? 0
      const distance = Math.hypot(x, y, z) || 1
      const targetRadius = radius * (0.7 + node.strength * 0.3)
      const pull = (targetRadius - distance) * alpha * 0.018

      node.vx = (node.vx ?? 0) + (x / distance) * pull
      node.vy = (node.vy ?? 0) + (y / distance) * pull
      node.vz = (node.vz ?? 0) + (z / distance) * pull
    }
  }

  force.initialize = (nextNodes: ForceGraphNode[]) => {
    forceNodes = nextNodes
  }

  return force
}

function initialSpherePosition(node: SourceGraphVisualNode, index: number) {
  if (node.role === 'core') return { x: 0, y: 0, z: 0 }

  const goldenAngle = Math.PI * (3 - Math.sqrt(5))
  const clusterHash = hashString(node.clusterKey)
  const isLeaf = node.role === 'leaf'
  const leafHash = hashString(node.id)
  const y = clamp(
    ((clusterHash % 1000) / 1000) * 2 - 1 + (isLeaf ? ((leafHash % 1000) / 1000 - 0.5) * 0.14 : 0),
    -1,
    1,
  )
  const ringRadius = Math.sqrt(Math.max(0, 1 - y * y))
  const clusterTheta = (((clusterHash >> 10) % 1000) / 1000) * Math.PI * 2
  const spread = isLeaf ? ((leafHash >> 6) % 1000) / 1000 - 0.5 : 0
  const theta = clusterTheta + spread * 0.36 + index * goldenAngle * 0.015
  const shellRatio = isLeaf ? 0.62 : 1
  const radius = GRAPH_RADIUS * shellRatio * (0.7 + node.strength * 0.3)

  return {
    x: Math.cos(theta) * ringRadius * radius,
    y: y * radius,
    z: Math.sin(theta) * ringRadius * radius,
  }
}

function nodeRadius(node: { role: SourceGraphNodeRole; sizeWeight: number }) {
  if (node.role === 'core') return 24
  return clamp(4 + node.sizeWeight * 6.4, 4, 22)
}

function hashString(key: string) {
  let hash = 0
  for (let index = 0; index < key.length; index += 1) {
    hash = (hash * 31 + key.charCodeAt(index)) >>> 0
  }
  return hash
}

function linkEndpointId(endpoint: string | ForceGraphNode | undefined) {
  if (typeof endpoint === 'object' && endpoint) return endpoint.id
  return String(endpoint ?? '')
}

function linkTouchesKind(link: ForceGraphLink, kind: SourceGraphNodeKind) {
  const source = typeof link.source === 'object' ? link.source : undefined
  const target = typeof link.target === 'object' ? link.target : undefined
  return source?.kind === kind || target?.kind === kind || link.source === 'knowledge-base' || link.target === 'knowledge-base'
}

function linkTouchesRole(link: ForceGraphLink, role: SourceGraphNodeRole) {
  const source = typeof link.source === 'object' ? link.source : undefined
  const target = typeof link.target === 'object' ? link.target : undefined
  return source?.role === role || target?.role === role
}

function truncateLabel(label: string) {
  return label.length > 28 ? `${label.slice(0, 25)}...` : label
}

function toZoomPercent(distance: number) {
  return Math.round((DEFAULT_CAMERA_DISTANCE / distance) * 100)
}

function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value))
}
