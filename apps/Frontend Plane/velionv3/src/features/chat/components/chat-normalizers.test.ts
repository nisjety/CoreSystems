import { describe, expect, it } from 'vitest'
import { humanizeToolName, missingSearchResultStep, readBrowseWebPreference, summarizeToolArgs } from './chat-normalizers'

describe('summarizeToolArgs', () => {
  it('returns an empty string for null / undefined / empty inputs', () => {
    expect(summarizeToolArgs(null)).toBe('')
    expect(summarizeToolArgs(undefined)).toBe('')
    expect(summarizeToolArgs({})).toBe('')
    expect(summarizeToolArgs('   ')).toBe('')
  })

  it('surfaces provider and operation first for integration tools', () => {
    const summary = summarizeToolArgs({
      channel: '#ops',
      operation: 'send_message',
      provider: 'slack',
    })
    expect(summary).toBe('provider: slack · operation: send_message · channel: #ops')
  })

  it('surfaces the query for web tools', () => {
    expect(summarizeToolArgs({ query: 'norwegian aquaculture', provider: 'hybrid' })).toBe(
      'provider: hybrid · query: norwegian aquaculture',
    )
  })

  it('passes a plain string argument through', () => {
    expect(summarizeToolArgs('https://example.com/docs')).toBe('https://example.com/docs')
  })

  it('renders nested object values as compact JSON', () => {
    expect(summarizeToolArgs({ filters: { status: 'open' } })).toBe('filters: {"status":"open"}')
  })

  it('caps the number of rendered params', () => {
    const summary = summarizeToolArgs({ a: 1, b: 2, c: 3, d: 4, e: 5, f: 6, g: 7, h: 8 })
    expect(summary.split(' · ')).toHaveLength(6)
  })

  it('skips empty values instead of rendering bare keys', () => {
    expect(summarizeToolArgs({ provider: 'meta', note: '' })).toBe('provider: meta')
  })
})

describe('humanizeToolName', () => {
  it('maps known builtin tools to friendly labels', () => {
    expect(humanizeToolName('web_search')).toBe('Web search')
    expect(humanizeToolName('knowledge_search')).toBe('Knowledge search')
  })

  it('humanizes snake / dotted tool names', () => {
    expect(humanizeToolName('book_shipment')).toBe('Book shipment')
    expect(humanizeToolName('social.publish_post')).toBe('Social publish post')
  })
})

describe('readBrowseWebPreference', () => {
  it('defaults to ON when no preference is stored', () => {
    localStorage.removeItem('velion.chat.browseWeb.v1')
    sessionStorage.removeItem('velion.chat.browseWeb.v1')
    expect(readBrowseWebPreference()).toBe(true)
  })

  it('honors an explicit opt-out', () => {
    localStorage.setItem('velion.chat.browseWeb.v1', '0')
    expect(readBrowseWebPreference()).toBe(false)
    localStorage.removeItem('velion.chat.browseWeb.v1')
  })

  it('stays ON when explicitly enabled', () => {
    localStorage.setItem('velion.chat.browseWeb.v1', '1')
    expect(readBrowseWebPreference()).toBe(true)
    localStorage.removeItem('velion.chat.browseWeb.v1')
  })
})

describe('missingSearchResultStep', () => {
  const waitingSearchStep = {
    id: 'asst-1:tool-search',
    title: 'Search',
    detail: 'Web search is available; it runs only if the answer needs fresh data.',
    status: 'waiting' as const,
    createdAt: new Date().toISOString(),
  }

  it('resolves a still-waiting search step as a calm done, not a failure', () => {
    // Search availability no longer implies a search must run — the backend
    // searches only when the query needs fresh data, so "completed without
    // searching" is the normal outcome for timeless questions.
    const resolved = missingSearchResultStep(waitingSearchStep, 'done')
    expect(resolved).not.toBeNull()
    expect(resolved?.status).toBe('done')
    expect(resolved?.detail).toContain('No web search needed')
    expect(resolved?.expandedDetail).toContain('not web-verified')
  })

  it('leaves already-resolved search steps and other steps untouched', () => {
    expect(missingSearchResultStep({ ...waitingSearchStep, status: 'done' }, 'done')).toBeNull()
    expect(missingSearchResultStep({ ...waitingSearchStep, id: 'asst-1:answer' }, 'done')).toBeNull()
    expect(missingSearchResultStep(waitingSearchStep, 'stopped')).toBeNull()
  })
})
