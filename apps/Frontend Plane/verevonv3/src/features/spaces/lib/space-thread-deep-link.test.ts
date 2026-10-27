import { afterEach, describe, expect, it, vi } from 'vitest'

import {
  readSpaceThreadFromSearch,
  readSpaceThreadParam,
  spaceThreadHref,
  spaceThreadPaneEnabled,
} from './space-thread-deep-link'

afterEach(() => {
  vi.unstubAllEnvs()
})

describe('readSpaceThreadParam (WS01-S7a)', () => {
  it('accepts a plain id and trims it', () => {
    expect(readSpaceThreadParam('thr_1')).toBe('thr_1')
    expect(readSpaceThreadParam('  thr_1  ')).toBe('thr_1')
  })

  it('rejects absent, blank and repeated values', () => {
    expect(readSpaceThreadParam(null)).toBeNull()
    expect(readSpaceThreadParam(undefined)).toBeNull()
    expect(readSpaceThreadParam('')).toBeNull()
    expect(readSpaceThreadParam('   ')).toBeNull()
    expect(readSpaceThreadParam(['a'])).toBeNull()
    expect(readSpaceThreadParam(['a', 'b'])).toBeNull()
  })

  it('rejects control characters and ids over 200 characters', () => {
    expect(readSpaceThreadParam('a\nb')).toBeNull()
    expect(readSpaceThreadParam('a\u0000b')).toBeNull()
    expect(readSpaceThreadParam('a\u007Fb')).toBeNull()
    expect(readSpaceThreadParam('x'.repeat(201))).toBeNull()
    expect(readSpaceThreadParam('x'.repeat(200))).toBe('x'.repeat(200))
  })

  it('reads the same value from a raw search string', () => {
    expect(readSpaceThreadFromSearch('?thread_id=thr_9')).toBe('thr_9')
    expect(readSpaceThreadFromSearch('?a=1&thread_id=thr_9')).toBe('thr_9')
    expect(readSpaceThreadFromSearch('?thread_id=')).toBeNull()
    expect(readSpaceThreadFromSearch('')).toBeNull()
    expect(readSpaceThreadFromSearch('?thread_id=a%0Ab')).toBeNull()
  })
})

describe('spaceThreadHref (WS01-S7a)', () => {
  it('builds the room link that opens the thread pane', () => {
    expect(spaceThreadHref('space-room', 'thr_1')).toBe('/spaces/space-room?thread_id=thr_1#chat')
  })

  it('round-trips through the reader', () => {
    const href = spaceThreadHref('space-room', 'thr_1')
    const url = new URL(href, 'https://example.test')
    expect(readSpaceThreadFromSearch(url.search)).toBe('thr_1')
    expect(url.hash).toBe('#chat')
  })

  it('encodes the space ref and the thread id', () => {
    const href = spaceThreadHref('a b/c', 'thr&1')
    expect(href).toBe('/spaces/a%20b%2Fc?thread_id=thr%261#chat')
    expect(readSpaceThreadFromSearch(new URL(href, 'https://example.test').search)).toBe('thr&1')
  })

  it('falls back to the plain room link for an id the reader would not hand back unchanged', () => {
    expect(spaceThreadHref('space-room', undefined)).toBe('/spaces/space-room#chat')
    expect(spaceThreadHref('space-room', null)).toBe('/spaces/space-room#chat')
    expect(spaceThreadHref('space-room', '')).toBe('/spaces/space-room#chat')
    expect(spaceThreadHref('space-room', '  thr_1 ')).toBe('/spaces/space-room#chat')
    expect(spaceThreadHref('space-room', 'a\nb')).toBe('/spaces/space-room#chat')
    expect(spaceThreadHref('space-room', 'x'.repeat(201))).toBe('/spaces/space-room#chat')
  })

  it('never builds a /chat link', () => {
    expect(spaceThreadHref('space-room', 'thr_1')).not.toContain('/chat?')
  })
})

describe('spaceThreadPaneEnabled', () => {
  it('follows the build flag, with the explicit values winning over dev', () => {
    vi.stubEnv('VITE_SPACE_THREAD_PANE_UI', 'true')
    expect(spaceThreadPaneEnabled()).toBe(true)
    vi.stubEnv('VITE_SPACE_THREAD_PANE_UI', 'false')
    expect(spaceThreadPaneEnabled()).toBe(false)
  })
})
