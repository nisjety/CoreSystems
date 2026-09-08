import { describe, expect, it } from 'vitest'
import { parseSupportSurface, supportSurfaceHref } from './support-navigation'

describe('support navigation', () => {
  it('recognizes the canonical support surfaces', () => {
    expect(parseSupportSurface(null)).toBe('conversations')
    expect(parseSupportSurface('tickets')).toBe('tickets')
    expect(parseSupportSurface('outbound')).toBe('outbound')
    expect(parseSupportSurface('remote')).toBe('remote')
  })

  it('falls back to conversations for an unknown surface', () => {
    expect(parseSupportSurface('nonsense')).toBe('conversations')
  })

  it('maps the retired AI review surface into Ticketing', () => {
    expect(parseSupportSurface('review')).toBe('tickets')
  })

  it('builds stable support deep links', () => {
    expect(supportSurfaceHref('conversations')).toBe('/support')
    expect(supportSurfaceHref('tickets')).toBe('/support?surface=tickets')
    expect(supportSurfaceHref('outbound')).toBe('/support?surface=outbound')
    expect(supportSurfaceHref('remote')).toBe('/support?surface=remote')
  })
})
