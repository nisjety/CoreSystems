import { afterEach, describe, expect, it, vi } from 'vitest'
import { browserOmniboxSearchEnabled, resolveOmniboxIntent } from './omnibox-resolver'

const on = { searchEnabled: true }

describe('resolveOmniboxIntent', () => {
  it('navigates to an explicit address or a bare host', () => {
    expect(resolveOmniboxIntent('https://example.com/a', on)).toEqual({ kind: 'navigate', url: 'https://example.com/a' })
    expect(resolveOmniboxIntent('example.com', on)).toEqual({ kind: 'navigate', url: 'https://example.com' })
    expect(resolveOmniboxIntent('localhost:5174', on)).toEqual({ kind: 'navigate', url: 'http://localhost:5174' })
  })

  it('searches free text through Verevon and never builds a third-party search url', () => {
    const intent = resolveOmniboxIntent('norwegian salmon exports', on)
    expect(intent).toEqual({ kind: 'search', query: 'norwegian salmon exports' })
    expect(JSON.stringify(resolveOmniboxIntent('vg', on))).not.toMatch(/google/i)
  })

  it('does nothing for free text while search is off, and says why', () => {
    expect(resolveOmniboxIntent('norwegian salmon', { searchEnabled: false })).toEqual({
      kind: 'noop',
      reason: 'search_disabled',
    })
  })

  it('does nothing for free text in a demo build, but still navigates to an address', () => {
    expect(resolveOmniboxIntent('salmon', { searchEnabled: true, demo: true })).toEqual({ kind: 'noop', reason: 'demo' })
    expect(resolveOmniboxIntent('example.com', { searchEnabled: true, demo: true })).toEqual({
      kind: 'navigate',
      url: 'https://example.com',
    })
  })

  it('navigates an empty field to the current page, and does nothing without one', () => {
    expect(resolveOmniboxIntent('   ', { searchEnabled: true, fallbackUrl: 'https://www.vg.no/' })).toEqual({
      kind: 'navigate',
      url: 'https://www.vg.no/',
    })
    expect(resolveOmniboxIntent('', on)).toEqual({ kind: 'noop', reason: 'empty' })
  })

  it('hands the text to chat for the ask row, and ignores an empty ask', () => {
    expect(resolveOmniboxIntent(' what is on this page ', on, { ask: true })).toEqual({
      kind: 'ask',
      text: 'what is on this page',
    })
    expect(resolveOmniboxIntent('  ', on, { ask: true })).toEqual({ kind: 'noop', reason: 'empty' })
  })
})

describe('browserOmniboxSearchEnabled', () => {
  afterEach(() => vi.unstubAllEnvs())

  it('is on only when the build flag is exactly 1', () => {
    vi.stubEnv('VITE_BROWSER_OMNIBOX_SEARCH', '1')
    expect(browserOmniboxSearchEnabled()).toBe(true)
    vi.stubEnv('VITE_BROWSER_OMNIBOX_SEARCH', '0')
    expect(browserOmniboxSearchEnabled()).toBe(false)
    vi.stubEnv('VITE_BROWSER_OMNIBOX_SEARCH', 'true')
    expect(browserOmniboxSearchEnabled()).toBe(false)
  })
})
