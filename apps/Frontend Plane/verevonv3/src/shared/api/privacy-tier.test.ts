import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  isClaimedPrivacyTier,
  normalizePrivacyTier,
  privacyTierBadgeLabel,
  privacyTierBadgeTitle,
  sovereignCatalogNotice,
} from './privacy-tier'

// Honesty contract for the whole tier surface: an affirmative claim exists
// only when the backend actually attested one. Unknown/garbage wire values
// degrade to `undefined` (neutral presentation) — never a fabricated claim.

// Fewer-params impl is assignable where `tr(no, en)` is called; Norwegian-first.
const tr = { tr: vi.fn((no: string) => no) }

afterEach(() => {
  vi.clearAllMocks()
})

describe('normalizePrivacyTier degradation', () => {
  it('accepts every snake_case wire value', () => {
    expect(normalizePrivacyTier('unspecified')).toBe('unspecified')
    expect(normalizePrivacyTier('global')).toBe('global')
    expect(normalizePrivacyTier('eu_resident')).toBe('eu_resident')
    expect(normalizePrivacyTier('zdr_contractual')).toBe('zdr_contractual')
    expect(normalizePrivacyTier('sovereign')).toBe('sovereign')
  })

  it('is case/whitespace tolerant', () => {
    expect(normalizePrivacyTier(' SOVEREIGN ')).toBe('sovereign')
    expect(normalizePrivacyTier('EU_RESIDENT')).toBe('eu_resident')
  })

  it('accepts proto ordinals weakest→strongest and degrades out-of-range ones', () => {
    expect(normalizePrivacyTier(0)).toBe('unspecified')
    expect(normalizePrivacyTier(1)).toBe('global')
    expect(normalizePrivacyTier(2)).toBe('eu_resident')
    expect(normalizePrivacyTier(3)).toBe('zdr_contractual')
    expect(normalizePrivacyTier(4)).toBe('sovereign')
    expect(normalizePrivacyTier(5)).toBeUndefined()
    expect(normalizePrivacyTier(-1)).toBeUndefined()
  })

  it('degrades unknown values to undefined instead of guessing', () => {
    expect(normalizePrivacyTier('fort_knox')).toBeUndefined()
    expect(normalizePrivacyTier('')).toBeUndefined()
    expect(normalizePrivacyTier(undefined)).toBeUndefined()
    expect(normalizePrivacyTier(null)).toBeUndefined()
    expect(normalizePrivacyTier(42)).toBeUndefined()
    expect(normalizePrivacyTier({ tier: 'sovereign' })).toBeUndefined()
  })
})

describe('claimed-tier predicate (affirmative tint gate)', () => {
  it('claims only backend-attested tiers', () => {
    expect(isClaimedPrivacyTier('eu_resident')).toBe(true)
    expect(isClaimedPrivacyTier('zdr_contractual')).toBe(true)
    expect(isClaimedPrivacyTier('sovereign')).toBe(true)
  })

  it('never claims global, unspecified, or unknown', () => {
    expect(isClaimedPrivacyTier('global')).toBe(false)
    expect(isClaimedPrivacyTier('unspecified')).toBe(false)
    expect(isClaimedPrivacyTier(undefined)).toBe(false)
    expect(isClaimedPrivacyTier(null)).toBe(false)
  })
})

describe('badge copy', () => {
  it('labels every renderable tier Norwegian-first', () => {
    expect(privacyTierBadgeLabel(tr, 'sovereign')).toBe('Soveren')
    expect(privacyTierBadgeLabel(tr, 'eu_resident')).toBe('EU')
    expect(privacyTierBadgeLabel(tr, 'zdr_contractual')).toBe('ZDR')
    expect(privacyTierBadgeLabel(tr, 'global')).toBe('Global')
    // unspecified never renders — the label helper still returns '' for it.
    expect(privacyTierBadgeLabel(tr, 'unspecified')).toBe('')
  })

  it('gives each claimed tier an honest one-sentence tooltip', () => {
    const title = privacyTierBadgeTitle(tr, 'eu_resident')
    expect(title).toContain('EU')
    expect(tr.tr).toHaveBeenCalledWith(expect.stringContaining('EU'), expect.stringContaining('EU'))
    expect(privacyTierBadgeTitle(tr, 'sovereign')).toContain('Norge')
  })
})

describe('sovereign catalog notice', () => {
  it('states the smaller catalog honestly in the active locale', () => {
    expect(sovereignCatalogNotice(tr)).toContain('færre modeller')
    expect(tr.tr).toHaveBeenCalledWith(
      expect.stringContaining('færre modeller'),
      expect.stringContaining('fewer models'),
    )
  })
})
