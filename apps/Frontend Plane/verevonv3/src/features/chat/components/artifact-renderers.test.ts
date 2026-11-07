// @vitest-environment jsdom

import { describe, expect, it, vi } from 'vitest'
import { deckManifestRenderer } from './ArtifactDeckView'
import {
  type ArtifactRenderer,
  createArtifactRendererRegistry,
  pickArtifactRenderer,
} from './artifact-renderers'
import type { ChatArtifact } from './chat-types'

function artifact(overrides: Partial<ChatArtifact> = {}): ChatArtifact {
  return { content: 'x', id: 'artifact-1', kind: 'grid', title: 'Tabell', version: 1, ...overrides }
}

function renderer(id: string, priority: number, match: (artifact: ChatArtifact) => boolean): ArtifactRenderer {
  return { id, match, priority }
}

describe('pickArtifactRenderer', () => {
  it('returns undefined when no renderer claims the artifact', () => {
    expect(pickArtifactRenderer([renderer('never', 0, () => false)], artifact())).toBeUndefined()
    expect(pickArtifactRenderer([], artifact())).toBeUndefined()
  })

  it('picks the highest priority among the renderers that match, whatever the order', () => {
    const low = renderer('low', 1, () => true)
    const high = renderer('high', 5, () => true)
    const skipped = renderer('skipped', 9, () => false)
    expect(pickArtifactRenderer([low, high, skipped], artifact())?.id).toBe('high')
    expect(pickArtifactRenderer([high, low], artifact())?.id).toBe('high')
  })

  it('keeps the earlier entry when priorities tie', () => {
    const first = renderer('first', 3, () => true)
    const second = renderer('second', 3, () => true)
    expect(pickArtifactRenderer([first, second], artifact())?.id).toBe('first')
  })

  it('hands the matcher the whole artifact', () => {
    const probe = artifact({ kind: 'probe' })
    const match = vi.fn(() => true)
    pickArtifactRenderer([renderer('p', 0, match)], probe)
    expect(match).toHaveBeenCalledWith(probe)
  })
})

describe('createArtifactRendererRegistry', () => {
  it('resolves nothing until a renderer is registered', () => {
    const registry = createArtifactRendererRegistry()
    expect(registry.resolve(artifact())).toBeUndefined()
  })

  it('resolves to a renderer registered after the registry was built', () => {
    const registry = createArtifactRendererRegistry<'grid'>()
    registry.register({ id: 'grid', match: (value) => value.kind === 'grid', priority: 1 })
    expect(registry.resolve(artifact())?.id).toBe('grid')
    expect(registry.resolve(artifact({ kind: 'other' }))).toBeUndefined()
  })

  it('removes only its own renderer when the returned function is called', () => {
    const registry = createArtifactRendererRegistry()
    const keep = registry.register(renderer('keep', 1, () => true))
    const drop = registry.register(renderer('drop', 2, () => true))
    expect(registry.resolve(artifact())?.id).toBe('drop')
    drop()
    expect(registry.resolve(artifact())?.id).toBe('keep')
    drop()
    expect(registry.resolve(artifact())?.id).toBe('keep')
    keep()
    expect(registry.resolve(artifact())).toBeUndefined()
  })

  it('carries the component of an extra viewer through to the resolved renderer', () => {
    const registry = createArtifactRendererRegistry()
    const component = () => null
    registry.register({ id: 'grid', match: () => true, priority: 0, component })
    expect(registry.resolve(artifact())?.component).toBe(component)
  })
})

describe('the deck manifest viewer (EP-5-S4)', () => {
  it('claims the deck kind and no other, so a document, an HTML page or code keeps its own viewer', () => {
    expect(deckManifestRenderer.match(artifact({ kind: 'deck', content: '{}' }))).toBe(true)
    for (const kind of ['document', 'html', 'code', 'text', 'pdf', 'spreadsheet']) {
      expect(deckManifestRenderer.match(artifact({ kind })), kind).toBe(false)
    }
  })

  it('resolves a deck to its own viewer when it is the only extra viewer registered', () => {
    const registry = createArtifactRendererRegistry()
    registry.register(deckManifestRenderer)
    expect(registry.resolve(artifact({ kind: 'deck', content: '{}' }))?.id).toBe('deck-manifest')
    expect(registry.resolve(artifact({ kind: 'document', content: '# Rapport' }))).toBeUndefined()
  })
})
