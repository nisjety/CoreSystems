import * as THREE from 'three'
import { describe, expect, it } from 'vitest'
import { createGraphNodeHalo, createGraphNodeSphere, graphNodeRadius } from './connectGraphGeometry'

describe('connect graph geometry', () => {
  it('uses real Three.js sphere meshes with Velion-colored standard materials', () => {
    const sphere = createGraphNodeSphere({ color: '#ee7a50' })
    const halo = createGraphNodeHalo({ color: '#ee7a50' })

    expect(sphere).toBeInstanceOf(THREE.Mesh)
    expect(sphere.geometry).toBeInstanceOf(THREE.SphereGeometry)
    expect(sphere.material).toBeInstanceOf(THREE.MeshStandardMaterial)
    expect(sphere.material.color.getHexString()).toBe('ee7a50')
    expect(sphere.material.emissive.getHexString()).toBe('ee7a50')
    expect(halo.geometry).toBeInstanceOf(THREE.SphereGeometry)
    expect(halo.material).toBeInstanceOf(THREE.MeshBasicMaterial)
  })

  it('keeps the core and high-degree hubs larger than their leaf nodes', () => {
    const core = graphNodeRadius({ role: 'core', sizeWeight: 4 })
    const hub = graphNodeRadius({ role: 'hub', sizeWeight: 2.1 })
    const standalone = graphNodeRadius({ role: 'standalone', sizeWeight: 1 })
    const leaf = graphNodeRadius({ role: 'leaf', sizeWeight: 0.35 })

    expect(core).toBeGreaterThan(hub)
    expect(hub).toBeGreaterThan(standalone)
    expect(standalone).toBeGreaterThan(leaf)
  })
})
