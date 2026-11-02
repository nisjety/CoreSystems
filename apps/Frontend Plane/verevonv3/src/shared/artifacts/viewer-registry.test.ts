import { afterEach, describe, expect, it } from 'vitest'

import {
  registerViewer,
  surfaceAllows,
  viewerActivationKey,
  viewerFor,
  type ViewerEntry,
} from './viewer-registry'

const removers: Array<() => void> = []
const register = (entry: ViewerEntry) => {
  const remove = registerViewer(entry)
  removers.push(remove)
  return remove
}

afterEach(() => {
  while (removers.length > 0) removers.pop()?.()
})

describe('viewerFor on the room surface', () => {
  it.each(['pdf', 'image', 'binary', 'text', 'spreadsheet', 'unknown-kind', ''])('refuses %j', (kind) => {
    expect(viewerFor(kind, 'room')).toBeUndefined()
    expect(surfaceAllows(kind, 'room')).toBe(false)
  })

  it.each(['document', 'code', 'html'])('allows %s', (kind) => {
    expect(viewerFor(kind, 'room')).toBeDefined()
    expect(surfaceAllows(kind, 'room')).toBe(true)
  })

  it('requires an explicit activation for html in the room only', () => {
    expect(viewerFor('html', 'room')?.requiresActivation).toBe(true)
    expect(viewerFor('html', 'room')?.trust).toBe('untrusted_markup')
    expect(viewerFor('html', 'chat')?.requiresActivation).toBeFalsy()
    expect(viewerFor('document', 'room')?.requiresActivation).toBeFalsy()
    expect(viewerFor('code', 'room')?.requiresActivation).toBeFalsy()
  })
})

describe('viewerFor on the chat surface', () => {
  it.each(['document', 'code', 'html', 'pdf', 'image', 'binary', 'text'])('keeps %s', (kind) => {
    expect(viewerFor(kind, 'chat')).toBeDefined()
  })

  it('refuses a kind nothing claims', () => {
    expect(viewerFor('hologram', 'chat')).toBeUndefined()
  })
})

describe('registerViewer', () => {
  it('adds a viewer for an unclaimed kind and removes it again', () => {
    const remove = register({ renderKinds: ['grid'], surfaces: ['chat'], trust: 'binary' })
    expect(viewerFor('grid', 'chat')).toBeDefined()
    expect(viewerFor('grid', 'room')).toBeUndefined()
    remove()
    expect(viewerFor('grid', 'chat')).toBeUndefined()
  })

  it('ignores an entry that tries to override a built-in kind', () => {
    for (const kind of ['pdf', 'image', 'binary', 'html', 'document', 'code', 'text']) {
      register({ renderKinds: [kind], surfaces: ['room', 'chat'], trust: 'text' })
    }
    expect(viewerFor('pdf', 'room')).toBeUndefined()
    expect(viewerFor('image', 'room')).toBeUndefined()
    expect(viewerFor('html', 'room')?.requiresActivation).toBe(true)
  })

  it('ignores empty entries', () => {
    register({ renderKinds: [], surfaces: ['chat'], trust: 'text' })
    register({ renderKinds: ['empty-surfaces'], surfaces: [], trust: 'text' })
    expect(viewerFor('empty-surfaces', 'chat')).toBeUndefined()
    expect(viewerFor('empty-surfaces', 'room')).toBeUndefined()
  })

  it('ignores untrusted markup for the room unless it requires activation', () => {
    register({ renderKinds: ['svg-live'], surfaces: ['room'], trust: 'untrusted_markup' })
    expect(viewerFor('svg-live', 'room')).toBeUndefined()

    register({ renderKinds: ['svg-gated'], surfaces: ['room'], trust: 'untrusted_markup', requiresActivation: true })
    expect(viewerFor('svg-gated', 'room')?.requiresActivation).toBe(true)
  })

  it('lets untrusted markup auto-preview on chat only, as before', () => {
    register({ renderKinds: ['widget'], surfaces: ['chat'], trust: 'untrusted_markup' })
    expect(viewerFor('widget', 'chat')).toBeDefined()
  })

  it('removing an ignored entry is a safe no-op', () => {
    const remove = registerViewer({ renderKinds: ['pdf'], surfaces: ['chat'], trust: 'binary' })
    expect(() => remove()).not.toThrow()
  })
})

describe('viewerActivationKey', () => {
  const base = { id: 'report', version: 1, content: '<p>one</p>' }

  it('is stable for the same artifact', () => {
    expect(viewerActivationKey(base)).toBe(viewerActivationKey({ ...base }))
  })

  it('differs by id, version and content', () => {
    const key = viewerActivationKey(base)
    expect(viewerActivationKey({ ...base, id: 'other' })).not.toBe(key)
    expect(viewerActivationKey({ ...base, version: 2 })).not.toBe(key)
    expect(viewerActivationKey({ ...base, content: '<p>two</p>' })).not.toBe(key)
  })

  it('treats a missing version as zero and distinguishes same-length content', () => {
    expect(viewerActivationKey({ id: 'a', content: 'xy' })).toBe(viewerActivationKey({ id: 'a', version: 0, content: 'xy' }))
    expect(viewerActivationKey({ id: 'a', content: 'xy' })).not.toBe(viewerActivationKey({ id: 'a', content: 'xz' }))
  })
})
