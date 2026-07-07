import { describe, expect, it } from 'vitest'
import { humanizeToolName, summarizeToolArgs } from './chat-normalizers'

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
