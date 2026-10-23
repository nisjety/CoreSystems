import { describe, expect, it } from 'vitest'
import { browserOmniboxTarget, classifyOmniboxInput } from './browser-omnibox'

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

  it('never turns free text into a search url; free text is a query', () => {
    expect(browserOmniboxTarget('latest norway news')).toBeNull()
    expect(browserOmniboxTarget('vg')).toBeNull()
  })

  it('classifies single words, schemes and addresses', () => {
    expect(classifyOmniboxInput('vg')).toEqual({ kind: 'query', query: 'vg' })
    expect(classifyOmniboxInput('latest norway news')).toEqual({ kind: 'query', query: 'latest norway news' })
    expect(classifyOmniboxInput('localhost:5174')).toEqual({ kind: 'url', url: 'http://localhost:5174' })
    expect(classifyOmniboxInput('127.0.0.1:8080/x')).toEqual({ kind: 'url', url: 'http://127.0.0.1:8080/x' })
    expect(classifyOmniboxInput('about:blank')).toEqual({ kind: 'query', query: 'about:blank' })
    expect(classifyOmniboxInput('javascript:alert(1)')).toEqual({ kind: 'query', query: 'javascript:alert(1)' })
    expect(classifyOmniboxInput('   ')).toEqual({ kind: 'empty' })
  })

  it('uses the current url when submitting an empty omnibox', () => {
    expect(browserOmniboxTarget('   ', 'https://www.vg.no/')).toBe('https://www.vg.no/')
    expect(browserOmniboxTarget('   ')).toBeNull()
  })
})
