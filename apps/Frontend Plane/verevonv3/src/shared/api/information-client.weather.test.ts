import { describe, expect, it } from 'vitest'
import { weatherConditionLabel, weatherGlyph } from './information-client'

describe('weatherGlyph', () => {
  it('keeps the glyphs of the English conditions the backend sends', () => {
    expect(weatherGlyph('Clear')).toBe('☀')
    expect(weatherGlyph('Partly cloudy')).toBe('⛅')
    expect(weatherGlyph('Cloudy')).toBe('☁')
    expect(weatherGlyph('Rain')).toBe('☔')
    expect(weatherGlyph('Snow')).toBe('❄')
    expect(weatherGlyph('Fog')).toBe('〰')
    expect(weatherGlyph('Thunderstorm')).toBe('⚡')
  })

  it('reads the Norwegian condition names', () => {
    expect(weatherGlyph('Klart')).toBe('☀')
    expect(weatherGlyph('Lettskyet')).toBe('⛅')
    expect(weatherGlyph('Delvis skyet')).toBe('⛅')
    expect(weatherGlyph('Skyet')).toBe('☁')
    expect(weatherGlyph('Overskyet')).toBe('☁')
    expect(weatherGlyph('Regn')).toBe('☔')
    expect(weatherGlyph('Snø')).toBe('❄')
    expect(weatherGlyph('Tåke')).toBe('〰')
    expect(weatherGlyph('Torden')).toBe('⚡')
  })

  it('does not take «lettskyet» for «skyet»', () => {
    expect(weatherGlyph('Lettskyet')).not.toBe('☁')
    expect(weatherGlyph('lettskyet')).toBe('⛅')
  })

  it('shows a neutral dot for a condition it does not know', () => {
    expect(weatherGlyph('Sandstorm')).toBe('○')
    expect(weatherGlyph('')).toBe('○')
  })
})

describe('weatherConditionLabel', () => {
  it('names a known condition in Norwegian for Norwegian readers', () => {
    expect(weatherConditionLabel('Partly cloudy', 'nb-NO')).toBe('Lettskyet')
    expect(weatherConditionLabel('Cloudy', 'no')).toBe('Skyet')
    expect(weatherConditionLabel('Clear', 'nb')).toBe('Klart')
    expect(weatherConditionLabel('Thunderstorm', 'nb-NO')).toBe('Torden')
  })

  it('keeps the backend’s own text for English readers and for unknown conditions', () => {
    expect(weatherConditionLabel('Partly cloudy', 'en-US')).toBe('Partly cloudy')
    expect(weatherConditionLabel('  Sandstorm ', 'no')).toBe('Sandstorm')
  })
})
