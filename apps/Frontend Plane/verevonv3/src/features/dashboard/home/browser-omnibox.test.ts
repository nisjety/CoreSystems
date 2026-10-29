import { describe, expect, it } from 'vitest'
import { browserOmniboxTarget } from './browser-omnibox'

describe('browser omnibox target', () => {
  it('keeps explicit http urls', () => {
    expect(browserOmniboxTarget('https://www.vg.no/')).toBe('https://www.vg.no/')
    expect(browserOmniboxTarget('http://localhost:5174')).toBe('http://localhost:5174')
  })

  it('normalizes host-like input to direct navigation urls', () => {
    expect(browserOmniboxTarget('coresystem.com')).toBe('https://coresystem.com')
    expect(browserOmniboxTarget('www.vg.no/nyheter')).toBe('https://www.vg.no/nyheter')
    expect(browserOmniboxTarget('localhost:5174')).toBe('http://localhost:5174')
  })

  it('falls back to Google search for plain queries', () => {
    expect(browserOmniboxTarget('latest norway news')).toBe('https://www.google.com/search?q=latest%20norway%20news')
    expect(browserOmniboxTarget('vg')).toBe('https://www.google.com/search?q=vg')
  })

  it('uses the current url when submitting an empty omnibox', () => {
    expect(browserOmniboxTarget('   ', 'https://www.vg.no/')).toBe('https://www.vg.no/')
    expect(browserOmniboxTarget('   ')).toBeNull()
  })
})
