import { describe, expect, it } from 'vitest'
import { parseSupportSurface, supportSurfaceHref } from './support-navigation'

describe('support navigation', () => {
  it('recognizes only the three canonical support surfaces', () => {
    expect(parseSupportSurface(null)).toBe('conversations')
    expect(parseSupportSurface('tickets')).toBe('tickets')
    expect(parseSupportSurface('outbound')).toBe('outbound')
  })

  it('maps the retired AI review surface into Ticketing', () => {
    expect(parseSupportSurface('review')).toBe('tickets')
  })

  it('builds stable support deep links', () => {
    expect(supportSurfaceHref('conversations')).toBe('/support')
    expect(supportSurfaceHref('tickets')).toBe('/support?surface=tickets')
    expect(supportSurfaceHref('outbound')).toBe('/support?surface=outbound')
  })
})
