import * as THREE from 'three'
import type { SourceGraphNodeRole } from './connectGraphScene'

type GraphNodeColor = { color: string }
type GraphNodeSize = { role: SourceGraphNodeRole; sizeWeight: number }

export function createGraphNodeSphere(node: GraphNodeColor) {
  return new THREE.Mesh(
    new THREE.SphereGeometry(1, 28, 20),
    new THREE.MeshStandardMaterial({
      color: node.color,
      emissive: node.color,
      emissiveIntensity: 0.18,
      metalness: 0.08,
      opacity: 0.96,
      roughness: 0.34,
      transparent: true,
    }),
  )
}

export function createGraphNodeHalo(node: GraphNodeColor) {
  return new THREE.Mesh(
    new THREE.SphereGeometry(1, 20, 14),
    new THREE.MeshBasicMaterial({
      blending: THREE.AdditiveBlending,
      color: node.color,
      depthWrite: false,
      opacity: 0.1,
      transparent: true,
    }),
  )
}

export function graphNodeRadius(node: GraphNodeSize) {
  if (node.role === 'core') return 20
  if (node.role === 'hub') return clamp(5 + node.sizeWeight * 4.6, 7, 16)
  if (node.role === 'standalone') return 4.2 + node.sizeWeight * 3.1
  return 2.7 + node.sizeWeight * 2.3
}

function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value))
}
