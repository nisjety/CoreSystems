import { describe, expect, it } from 'vitest'
import {
  formatDerivedTrafficMetadata,
  formatTrafficObservation,
  type InformationObservation,
} from './information-client'

function observation(
  overrides: Partial<InformationObservation> = {},
): InformationObservation {
  return {
    confidence: null,
    fetchedAt: '2026-07-13T08:00:00Z',
    freshness: 'unavailable',
    observationType: 'unavailable',
    observedAt: null,
    provider: 'statens_vegvesen_atlas',
    quality: 'provider_metadata_only',
    source: 'https://trafikkdata-api.atlas.vegvesen.no',
    unavailableReason: 'provider_response_has_no_measurement',
    unit: 'km/h',
    value: null,
    ...overrides,
  }
}

describe('formatTrafficObservation', () => {
  it('renders unavailable data without inventing a number', () => {
    const rendered = formatTrafficObservation(observation(), 'en-US')
    expect(rendered).toContain('Unavailable')
    expect(rendered).toContain('Statens vegvesen')
    expect(rendered).not.toMatch(/\d+\s*km\/h/)
  })

  it('labels estimates and synthetic values explicitly', () => {
    expect(
      formatTrafficObservation(
        observation({ observationType: 'estimated', value: 71 }),
        'en-US',
      ),
    ).toContain('Estimated')
    expect(
      formatTrafficObservation(
        observation({ observationType: 'synthetic', value: 71 }),
        'en-US',
      ),
    ).toContain('Synthetic')
  })

  it('labels measured values and includes their units', () => {
    expect(
      formatTrafficObservation(
        observation({
          freshness: 'fresh',
          observationType: 'measured',
          value: 71,
        }),
        'en-US',
      ),
    ).toBe('71 km/h · Measured · Statens vegvesen')
  })
})

describe('formatDerivedTrafficMetadata', () => {
  it('labels heuristic road and county metadata as estimated', () => {
    expect(
      formatDerivedTrafficMetadata(
        {
          county: 'Oslo',
          roadReference: 'E6',
        },
        'en-US',
      ),
    ).toBe('E6 · Oslo · Estimated metadata')
  })

  it('defaults missing heuristic metadata to unavailable', () => {
    expect(
      formatDerivedTrafficMetadata(
        { county: '', roadReference: '' },
        'nb-NO',
      ),
    ).toBe('Metadata utilgjengelig')
  })
})
