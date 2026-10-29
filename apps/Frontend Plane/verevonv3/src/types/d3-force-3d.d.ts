declare module 'd3-force-3d' {
  export type ForceCollide<NodeDatum> = {
    (alpha: number): void
    initialize?: (nodes: NodeDatum[]) => void
    radius: (value: number | ((node: NodeDatum) => number)) => ForceCollide<NodeDatum>
    strength: (value: number) => ForceCollide<NodeDatum>
    iterations: (value: number) => ForceCollide<NodeDatum>
  }

  export function forceCollide<NodeDatum>(
    radius?: number | ((node: NodeDatum) => number),
  ): ForceCollide<NodeDatum>

  export type SimulationNodeDatum = {
    index?: number
    x?: number
    y?: number
    z?: number
    vx?: number
    vy?: number
    vz?: number
    fx?: number | null
    fy?: number | null
    fz?: number | null
  }

  export type SimulationLinkDatum<NodeDatum> = {
    source: NodeDatum | string
    target: NodeDatum | string
    index?: number
  }

  export type ForceLink<NodeDatum, LinkDatum> = {
    (alpha: number): void
    initialize?: (nodes: NodeDatum[]) => void
    links: (value?: LinkDatum[]) => ForceLink<NodeDatum, LinkDatum> | LinkDatum[]
    id: (value: (node: NodeDatum) => string) => ForceLink<NodeDatum, LinkDatum>
    distance: (value: number | ((link: LinkDatum) => number)) => ForceLink<NodeDatum, LinkDatum>
    strength: (value: number | ((link: LinkDatum) => number)) => ForceLink<NodeDatum, LinkDatum>
    iterations: (value: number) => ForceLink<NodeDatum, LinkDatum>
  }

  export function forceLink<NodeDatum, LinkDatum = SimulationLinkDatum<NodeDatum>>(
    links?: LinkDatum[],
  ): ForceLink<NodeDatum, LinkDatum>

  export type ForceManyBody<NodeDatum> = {
    (alpha: number): void
    initialize?: (nodes: NodeDatum[]) => void
    strength: (value: number | ((node: NodeDatum) => number)) => ForceManyBody<NodeDatum>
    distanceMin: (value: number) => ForceManyBody<NodeDatum>
    distanceMax: (value: number) => ForceManyBody<NodeDatum>
  }

  export function forceManyBody<NodeDatum>(): ForceManyBody<NodeDatum>

  export type ForceCenter<NodeDatum> = {
    (): void
    initialize?: (nodes: NodeDatum[]) => void
    x: (value: number) => ForceCenter<NodeDatum>
    y: (value: number) => ForceCenter<NodeDatum>
    strength: (value: number) => ForceCenter<NodeDatum>
  }

  export function forceCenter<NodeDatum>(x?: number, y?: number): ForceCenter<NodeDatum>

  export type ForceSimulation<NodeDatum> = {
    nodes: (value?: NodeDatum[]) => ForceSimulation<NodeDatum> | NodeDatum[]
    force: (name: string, force?: unknown) => ForceSimulation<NodeDatum>
    alpha: {
      (): number
      (value: number): ForceSimulation<NodeDatum>
    }
    alphaMin: (value: number) => ForceSimulation<NodeDatum>
    alphaDecay: (value: number) => ForceSimulation<NodeDatum>
    alphaTarget: (value: number) => ForceSimulation<NodeDatum>
    velocityDecay: (value: number) => ForceSimulation<NodeDatum>
    restart: () => ForceSimulation<NodeDatum>
    stop: () => ForceSimulation<NodeDatum>
    tick: (iterations?: number) => ForceSimulation<NodeDatum>
    on: (typenames: string, listener?: (() => void) | null) => ForceSimulation<NodeDatum>
  }

  export function forceSimulation<NodeDatum>(
    nodes?: NodeDatum[],
    numDimensions?: number,
  ): ForceSimulation<NodeDatum>
}
