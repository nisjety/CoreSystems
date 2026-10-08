import { describe, expect, it } from 'vitest'
import { privacyTierBadgeTitle, sovereignCatalogNotice, type PrivacyTier } from './privacy-tier'

// The tier chips and their hover text are derived from what a provider DECLARES: a region label and an
// operator-written ZDR attestation. docs/compliance/evidence/claims-vs-code.md rows P1, P2 and P3 say what
// the code can and cannot back: a declared EU region label is not a jurisdiction, a ZDR attestation is our
// own record (its digest binds fields to each other, not to the endpoint), and no provider is classed Norway
// today. The hover text must say "declared" or "recorded" and must not promise what a label cannot carry.

const no = { tr: (norwegian: string) => norwegian }
const en = { tr: (_norwegian: string, english: string) => english }

const declared: PrivacyTier[] = ['sovereign', 'eu_resident', 'zdr_contractual']

describe('tier tooltips say what is declared, not what is guaranteed', () => {
  it.each(declared)('%s says it is declared or recorded and not independently verified', (tier) => {
    expect(privacyTierBadgeTitle(no, tier)).toMatch(/ikke uavhengig verifisert/)
    expect(privacyTierBadgeTitle(en, tier)).toMatch(/not independently verified/)
  })

  it.each(declared)('%s makes none of the claims the code contradicts', (tier) => {
    for (const text of [privacyTierBadgeTitle(no, tier), privacyTierBadgeTitle(en, tier)]) {
      expect(text).not.toMatch(/jurisdiksjon|jurisdiction/i)
      expect(text).not.toMatch(/ingenting lagres|nothing is stored/i)
      expect(text).not.toMatch(/aldri utenfor|never outside/i)
      expect(text).not.toMatch(/^Null datalagring|^Zero data retention –/)
    }
  })

  it('eu_resident names the region as set, not the data as kept', () => {
    expect(privacyTierBadgeTitle(no, 'eu_resident')).toContain('region er satt til EU/EØS')
    expect(privacyTierBadgeTitle(en, 'eu_resident')).toContain('region is set to EU/EEA')
  })

  it('zdr_contractual names a recorded attestation, our own record', () => {
    expect(privacyTierBadgeTitle(no, 'zdr_contractual')).toContain('registrert ZDR-godkjenning')
    expect(privacyTierBadgeTitle(en, 'zdr_contractual')).toContain('recorded zero-data-retention attestation')
  })

  it('sovereign says Norway only as a declaration, and a catalogue notice says the same', () => {
    expect(privacyTierBadgeTitle(no, 'sovereign')).toContain('Erklært')
    expect(privacyTierBadgeTitle(no, 'sovereign')).toContain('Norge')
    expect(sovereignCatalogNotice(no)).toContain('erklært behandling kun i Norge')
    expect(sovereignCatalogNotice(en)).toContain('declared processing stays in Norway')
  })

  it('global keeps its plain statement and unspecified shows nothing', () => {
    expect(privacyTierBadgeTitle(no, 'global')).toBe('Ingen regionsbegrensning')
    expect(privacyTierBadgeTitle(no, 'unspecified')).toBe('')
  })
})
