import ForceGraph3D, { type ForceGraph3DInstance } from '3d-force-graph'
import { forceCollide } from 'd3-force-3d'
import * as THREE from 'three'
import SpriteText from 'three-spritetext'
import { createGraphNodeHalo, createGraphNodeSphere, graphNodeRadius } from './connectGraphGeometry'

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
  onZoomChange?: (zoomPercent: number) => void
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
  addEventListener?: (type: 'change', listener: () => void) => void
  removeEventListener?: (type: 'change', listener: () => void) => void
}

type NodeVisualState = {
  active: boolean
  hovered: boolean
  highlighted: boolean
  dimmed: boolean
}

type NodeParts = {
  sphere: THREE.Mesh<THREE.SphereGeometry, THREE.MeshStandardMaterial>
  halo: THREE.Mesh<THREE.SphereGeometry, THREE.MeshBasicMaterial>
  label: SpriteText
}

const DEFAULT_CAMERA_DISTANCE = 480
const MIN_CAMERA_DISTANCE = 250
const MAX_CAMERA_DISTANCE = 760
const MAX_DEVICE_PIXEL_RATIO = 1.65
const GRAPH_RADIUS = 112
const POINTER_TAP_TOLERANCE = 6

const LINK_COLOR = 'rgba(247, 200, 168, 0.62)'
const LINK_HIGHLIGHT_COLOR = 'rgba(238, 122, 80, 0.92)'
const WHITE = new THREE.Color('#ffffff')

export function createConnectGraphScene(
  graphElement: HTMLElement,
  host: HTMLElement,
  options: GraphSceneOptions,
): SourceGraphSceneController | undefined {
  if (typeof window === 'undefined') return undefined
  if (window.navigator.userAgent.toLowerCase().includes('jsdom')) return undefined

  let graph: ForceGraph3DInstance<ForceGraphNode, ForceGraphLink> | undefined
  let nodes: ForceGraphNode[] = []
  let nodeById = new Map<string, ForceGraphNode>()
  let links: ForceGraphLink[] = []
  let activeNodeId: string | undefined
  let hoveredNodeId: string | undefined
  const nodeObjects = new Map<string, THREE.Object3D>()
  const raycaster = new THREE.Raycaster()
  const pointer = new THREE.Vector2()
  let cameraDistance = DEFAULT_CAMERA_DISTANCE
  let fitCameraDistance = DEFAULT_CAMERA_DISTANCE
  let lastReportedZoomPercent = 100
  let disposed = false
  let fittedAfterData = false
  let framingScheduled = false
  let pointerDownPoint: { x: number; y: number } | undefined
  const pendingTimeouts = new Set<number>()

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
    .nodeLabel(() => '')
    .nodeThreeObject((node) => {
      const object = createNodeObject(node, nodeVisualState(node.id))
      registerNodeObject(node.id, object)
      return object
    })
    .linkLabel((link) => link.label)
    .linkColor((link) => (isHighlightedLink(link) ? LINK_HIGHLIGHT_COLOR : LINK_COLOR))
    .linkOpacity(0.64)
    .linkWidth(0)
    .linkDirectionalParticles(0)
    .enableNodeDrag(true)
    .enablePointerInteraction(true)
    .showPointerCursor(() => false)
    .warmupTicks(options.reducedMotion ? 70 : 120)
    .cooldownTicks(options.reducedMotion ? 80 : 220)
    .cooldownTime(options.reducedMotion ? 1800 : 6500)
    .d3AlphaDecay(0.026)
    .d3VelocityDecay(0.34)
    .onNodeDragEnd((node) => {
      node.fx = node.x
      node.fy = node.y
      node.fz = node.z
    })
    // The layout keeps expanding well past the early fit, so the framing
    // shot has to wait for the simulation to actually stop.
    .onEngineStop(() => {
      if (disposed || fittedAfterData || framingScheduled) return
      framingScheduled = true
      cameraDistance = DEFAULT_CAMERA_DISTANCE
      fitCameraDistance = DEFAULT_CAMERA_DISTANCE
      graph?.cameraPosition({ x: 0, y: 0, z: cameraDistance }, { x: 0, y: 0, z: 0 }, 420)
      scheduleTimeout(() => {
        fittedAfterData = true
        framingScheduled = false
        syncCameraDistance(true)
      }, 460)
    })

  const renderer = graphInstance.renderer()
  renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, MAX_DEVICE_PIXEL_RATIO))

  const graphScene = graphInstance.scene()
  const ambientLight = new THREE.HemisphereLight(0xffefe4, 0x08130f, 1.5)
  const keyLight = new THREE.DirectionalLight(0xffc7a5, 2.25)
  keyLight.position.set(120, 180, 260)
  graphScene.add(ambientLight, keyLight)
  graphScene.fog = new THREE.FogExp2(0x080e0c, 0.0007)
  const orbitControls = graphInstance.controls() as OrbitControlsLike
  orbitControls.addEventListener?.('change', handleControlsChange)
  configureControls(orbitControls)
  configureForces()

  const handlePointerMove = (event: MouseEvent | PointerEvent) => {
    setHoveredNode(pickNodeFromPointer(event)?.id)
  }

  const handlePointerDown = (event: PointerEvent) => {
    pointerDownPoint = { x: event.clientX, y: event.clientY }
  }

  const handlePointerUp = (event: PointerEvent) => {
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
      clearPendingTimeouts()
      nodeObjects.clear()
      nodes = graphData.nodes
      nodeById = new Map(nodes.map((node) => [node.id, node]))
      links = graphData.links
      fittedAfterData = false
      framingScheduled = false
      graph?.graphData(graphData)
      configureForces()
    },
    setActiveNode(nodeId) {
      activeNodeId = nodeId
      applyVisualStates()
      syncAutoRotation()
      const node = nodeId ? nodeById.get(nodeId) : undefined
      options.onSelectNode?.(node ? projectNode(node) : undefined)
    },
    pick() {
      const node = activeNodeId ? nodeById.get(activeNodeId) : undefined
      return node ? projectNode(node) : undefined
    },
    zoom(direction) {
      cameraDistance = clamp(cameraDistance - direction * 40, MIN_CAMERA_DISTANCE, MAX_CAMERA_DISTANCE)
      graph?.cameraPosition({ x: 0, y: 0, z: cameraDistance }, { x: 0, y: 0, z: 0 }, 260)
      return reportZoomPercent()
    },
    reset() {
      cameraDistance = fitCameraDistance
      for (const node of nodes) {
        if (node.role === 'core') continue
        node.fx = undefined
        node.fy = undefined
        node.fz = undefined
      }
      configureControls(graph?.controls())
      graph?.cameraPosition({ x: 0, y: 0, z: cameraDistance }, { x: 0, y: 0, z: 0 }, 360)
      graph?.d3ReheatSimulation()
      return reportZoomPercent()
    },
    zoomPercent() {
      return toZoomPercent(cameraDistance, fitCameraDistance)
    },
    dispose() {
      disposed = true
      clearPendingTimeouts()
      resizeObserver?.disconnect()
      window.removeEventListener('resize', resize)
      graph?._destructor()
      graph = undefined
      orbitControls.removeEventListener?.('change', handleControlsChange)
      graphScene.remove(ambientLight, keyLight)
      nodeObjects.clear()
      nodeById.clear()
      graphElement.removeEventListener('pointermove', handlePointerMove)
      graphElement.removeEventListener('pointerdown', handlePointerDown, true)
      graphElement.removeEventListener('pointerup', handlePointerUp, true)
      graphElement.replaceChildren()
    },
  }

  function handleControlsChange() {
    if (!fittedAfterData) return
    syncCameraDistance()
  }

  function syncCameraDistance(setFitBaseline = false) {
    if (!graph || disposed) return
    const position = graph.cameraPosition()
    const distance = Math.hypot(position.x, position.y, position.z)
    if (Number.isFinite(distance) && distance > 1) {
      cameraDistance = clamp(distance, MIN_CAMERA_DISTANCE, MAX_CAMERA_DISTANCE)
      if (setFitBaseline) fitCameraDistance = cameraDistance
      reportZoomPercent()
    }
  }

  function reportZoomPercent() {
    const percent = toZoomPercent(cameraDistance, fitCameraDistance)
    if (percent !== lastReportedZoomPercent) {
      lastReportedZoomPercent = percent
      options.onZoomChange?.(percent)
    }
    return percent
  }

  function scheduleTimeout(callback: () => void, delay: number) {
    const timeoutId = window.setTimeout(() => {
      pendingTimeouts.delete(timeoutId)
      callback()
    }, delay)
    pendingTimeouts.add(timeoutId)
  }

  function clearPendingTimeouts() {
    pendingTimeouts.forEach((timeoutId) => window.clearTimeout(timeoutId))
    pendingTimeouts.clear()
  }

  function registerNodeObject(nodeId: string, object: THREE.Object3D) {
    object.userData.nodeId = nodeId
    object.traverse((child) => {
      child.userData.nodeId = nodeId
    })
    nodeObjects.set(nodeId, object)
  }

  function setHoveredNode(nodeId: string | undefined) {
    if (hoveredNodeId === nodeId) return
    hoveredNodeId = nodeId
    graphElement.classList.toggle('onboarding-source-graph__engine--hovering', Boolean(nodeId))
    options.onHoverNode?.(nodeId ? nodeById.get(nodeId) : undefined)
    applyVisualStates()
    syncAutoRotation()
  }

  function selectNode(node: ForceGraphNode) {
    activeNodeId = node.id
    options.onSelectNode?.(projectNode(node))
    applyVisualStates()
    syncAutoRotation()
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
    applyVisualStates()
    syncAutoRotation()
  }

  /** Mutates existing node materials/scales in place — never rebuilds the
   * scene graph, so hover/selection stays smooth and clicks are never lost
   * to a mid-frame teardown. */
  function applyVisualStates() {
    for (const node of nodes) {
      const object = nodeObjects.get(node.id)
      const parts = object?.userData.parts as NodeParts | undefined
      if (!parts) continue
      applyStateToParts(node, parts, nodeVisualState(node.id))
    }
    graph?.linkColor((link) => (isHighlightedLink(link) ? LINK_HIGHLIGHT_COLOR : LINK_COLOR))
  }

  function pickNodeFromPointer(event: MouseEvent | PointerEvent) {
    if (!graph) return undefined

    const bounds = graphElement.getBoundingClientRect()
    if (bounds.width <= 0 || bounds.height <= 0) return undefined

    pointer.x = ((event.clientX - bounds.left) / bounds.width) * 2 - 1
    pointer.y = -((event.clientY - bounds.top) / bounds.height) * 2 + 1
    raycaster.setFromCamera(pointer, graph.camera())

    const intersections = raycaster.intersectObjects([...nodeObjects.values()], true)
    for (const intersection of intersections) {
      const nodeId = findNodeId(intersection.object)
      if (!nodeId) continue
      const node = nodeById.get(nodeId)
      if (node) return node
    }

    return undefined
  }

  function configureControls(rawControls?: object) {
    const controls = rawControls as OrbitControlsLike | undefined
    if (!controls) return
    controls.autoRotate = !options.reducedMotion && !activeNodeId && !hoveredNodeId
    controls.autoRotateSpeed = 0.45
    controls.enableDamping = true
    controls.dampingFactor = 0.08
  }

  function syncAutoRotation() {
    configureControls(graph?.controls())
  }

  function configureForces() {
    if (!graph) return

    const charge = graph.d3Force('charge') as ForceWithStrength | undefined
    charge?.strength?.((node) => (node.role === 'leaf' ? -12 : node.role === 'hub' ? -82 : -48))

    const link = graph.d3Force('link') as LinkForce | undefined
    link?.distance?.((item) => {
      if (linkTouchesRole(item, 'leaf')) return 22
      if (linkTouchesKind(item, 'core')) return 88
      return 72
    })
    link?.strength?.((item) => (linkTouchesRole(item, 'leaf') ? 0.9 : 0.3))

    graph.d3Force('collide', forceCollide<ForceGraphNode>((node) => graphNodeRadius(node) * 1.35).strength(0.92))
    graph.d3Force('sphere', createSphereForce(GRAPH_RADIUS))
  }

  function nodeVisualState(nodeId: string): NodeVisualState {
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

function createNodeObject(node: ForceGraphNode, state: NodeVisualState) {
  const group = new THREE.Group()
  const radius = graphNodeRadius(node)

  const halo = createGraphNodeHalo(node)
  group.add(halo)

  const sphere = createGraphNodeSphere(node)
  sphere.userData.nodeId = node.id
  group.add(sphere)

  // Invisible, oversized raycast target so small leaves stay easy to
  // hover/click.
  const hitProxy = new THREE.Mesh(
    new THREE.SphereGeometry(1, 8, 6),
    new THREE.MeshBasicMaterial({ transparent: true, opacity: 0, depthWrite: false, colorWrite: false }),
  )
  hitProxy.scale.setScalar(Math.max(radius * 1.5, radius + 4))
  group.add(hitProxy)

  const label = new SpriteText(truncateLabel(node.label), labelTextHeight(node.role), '#f7f4ee')
  label.fontFace = 'Inter, ui-sans-serif, system-ui'
  label.fontWeight = node.role === 'core' ? '800' : '650'
  label.backgroundColor = false
  label.strokeWidth = 0.5
  label.strokeColor = 'rgba(8, 14, 12, 0.85)'
  label.material.transparent = true
  label.material.depthWrite = false
  label.center.set(0, 0.5)
  label.position.set(radius + 2.4, 0, 0)
  group.add(label)

  const parts: NodeParts = {
    sphere,
    halo,
    label,
  }
  group.userData.parts = parts
  applyStateToParts(node, parts, state)

  return group
}

function applyStateToParts(node: ForceGraphNode, parts: NodeParts, state: NodeVisualState) {
  const radius = graphNodeRadius(node)
  const focusScale = state.active ? 1.2 : state.hovered ? 1.1 : 1

  parts.sphere.material.color.set(node.color)
  if (state.active || state.hovered) parts.sphere.material.color.lerp(WHITE, 0.3)
  parts.sphere.material.opacity = state.dimmed ? 0.18 : node.connected ? 0.96 : 0.6
  parts.sphere.scale.setScalar(radius * focusScale)

  parts.halo.material.color.set(node.color)
  parts.halo.material.opacity = state.dimmed ? 0.02 : state.highlighted ? 0.24 : 0.09
  parts.halo.scale.setScalar(radius * (state.highlighted ? 1.7 : 1.35) * focusScale)

  parts.label.material.opacity = state.dimmed ? 0.08 : state.highlighted ? 1 : restLabelOpacity(node.role)
}

function restLabelOpacity(role: SourceGraphNodeRole) {
  if (role === 'core') return 0.95
  if (role === 'hub') return 0.94
  if (role === 'standalone') return 0.86
  return 0.68
}

function labelTextHeight(role: SourceGraphNodeRole) {
  if (role === 'core') return 8
  if (role === 'hub') return 5.2
  if (role === 'standalone') return 4.4
  return 3.2
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

function findNodeId(object: THREE.Object3D): string | undefined {
  let current: THREE.Object3D | null = object
  while (current) {
    const nodeId = current.userData.nodeId
    if (typeof nodeId === 'string') return nodeId
    current = current.parent
  }
  return undefined
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

function toZoomPercent(distance: number, baseline = DEFAULT_CAMERA_DISTANCE) {
  return Math.round((baseline / distance) * 100)
}

function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value))
}
