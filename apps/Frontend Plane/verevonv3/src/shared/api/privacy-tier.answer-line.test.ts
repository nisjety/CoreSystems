import { describe, expect, it } from 'vitest'
import {
  answerProcessingNote,
  normalizeAnswerResidency,
  normalizeAnswerServedBy,
  type AnswerServedBy,
} from './privacy-tier'

// The line under an answer saying where it was processed. Honesty contract
// (same as the tier badge): a claim is rendered ONLY from what the gateway
// stamped, and a gateway that stamped nothing renders nothing. The function
// takes no model name, so a residency cannot be inferred from one.

const no = { tr: (norwegian: string) => norwegian }
const en = { tr: (_norwegian: string, english: string) => english }

describe('normalizeAnswerServedBy', () => {
  it('reads the three stamped fields off a usage payload', () => {
    expect(
      normalizeAnswerServedBy({
        provider_used: 'azure-openai',
        residency: 'eu',
        privacy_tier: 'zdr_contractual',
      }),
    ).toEqual({ providerUsed: 'azure-openai', residency: 'eu', privacyTier: 'zdr_contractual' })
  })

  it('returns undefined when an older gateway stamped nothing', () => {
    expect(normalizeAnswerServedBy({})).toBeUndefined()
    expect(normalizeAnswerServedBy({ input_tokens: 10, cost_usd: 0.01 } as never)).toBeUndefined()
  })

  it('drops unknown, empty and malformed values instead of guessing', () => {
    expect(
      normalizeAnswerServedBy({ provider_used: '', residency: 'atlantis', privacy_tier: 'platinum' }),
    ).toBeUndefined()
    expect(normalizeAnswerServedBy({ residency: 5, privacy_tier: {}, provider_used: null })).toBeUndefined()
    // `unspecified` means "no constraint"; a stamp never says it.
    expect(normalizeAnswerServedBy({ privacy_tier: 'unspecified' })).toBeUndefined()
    // A provider id keeps the shape of a registry id, nothing free-form.
    expect(normalizeAnswerServedBy({ provider_used: '<img src=x onerror=alert(1)>' })).toBeUndefined()
    expect(normalizeAnswerServedBy({ provider_used: 'a'.repeat(65) })).toBeUndefined()
  })

  it('keeps what is usable when only part of the stamp is', () => {
    expect(normalizeAnswerServedBy({ residency: ' EU ', privacy_tier: 'platinum' })).toEqual({ residency: 'eu' })
  })

  it('normalizes residency alone', () => {
    expect(normalizeAnswerResidency('Norway')).toBe('norway')
    expect(normalizeAnswerResidency('us')).toBeUndefined()
    expect(normalizeAnswerResidency(undefined)).toBeUndefined()
  })
})

describe('answerProcessingNote', () => {
  it('names EU, the provider and ZDR when the server asserted all three', () => {
    const note = answerProcessingNote(no, {
      providerUsed: 'azure-openai',
      residency: 'eu',
      privacyTier: 'zdr_contractual',
    })
    expect(note?.text).toBe('Behandlet: EU (Azure), ZDR')
  })

  it('says only EU when no retention claim was asserted', () => {
    const note = answerProcessingNote(no, {
      providerUsed: 'azure-anthropic',
      residency: 'eu',
      privacyTier: 'eu_resident',
    })
    expect(note?.text).toBe('Behandlet: EU (Azure)')
    expect(note?.text).not.toContain('ZDR')
  })

  it('reads global with no EU or ZDR guarantee for the subscription route', () => {
    const note = answerProcessingNote(no, {
      providerUsed: 'openai-codex-subscription',
      residency: 'global',
      privacyTier: 'global',
    })
    expect(note?.text).toBe('Behandlet: global, ingen EU- eller ZDR-garanti')
    // No EU or ZDR claim anywhere in the line or the hover text.
    expect(note?.text).not.toMatch(/\bEU \(|, ZDR$/)
    expect(note?.title).toContain('utenfor EU/EØS')
  })

  it('reads global from the residency alone when no tier was stamped', () => {
    expect(answerProcessingNote(no, { residency: 'global' })?.text).toBe(
      'Behandlet: global, ingen EU- eller ZDR-garanti',
    )
  })

  it('renders nothing when the gateway stamped nothing', () => {
    expect(answerProcessingNote(no, undefined)).toBeUndefined()
    expect(answerProcessingNote(no, null)).toBeUndefined()
    expect(answerProcessingNote(no, {})).toBeUndefined()
  })

  it('renders nothing for a provider with no residency or retention claim', () => {
    expect(answerProcessingNote(no, { providerUsed: 'azure-openai' })).toBeUndefined()
  })

  it('names no vendor for a provider it cannot name', () => {
    expect(
      answerProcessingNote(no, { providerUsed: 'mystery-cloud', residency: 'eu', privacyTier: 'eu_resident' })?.text,
    ).toBe('Behandlet: EU')
  })

  it('does not call a global answer EU when the tier and the residency disagree', () => {
    const conflicting: AnswerServedBy = { residency: 'global', privacyTier: 'eu_resident' }
    expect(answerProcessingNote(no, conflicting)?.text).toBe('Behandlet: global, ingen EU- eller ZDR-garanti')
  })

  it('keeps the EU guarantee out of a global answer that carries a ZDR tier', () => {
    expect(answerProcessingNote(no, { residency: 'global', privacyTier: 'zdr_contractual' })?.text).toBe(
      'Behandlet: global, ingen EU-garanti, ZDR',
    )
  })

  it('states Norway for a Norwegian deployment', () => {
    expect(
      answerProcessingNote(no, { residency: 'norway', privacyTier: 'sovereign' })?.text,
    ).toBe('Behandlet: Norge, ZDR')
  })

  it('has an English reading', () => {
    expect(
      answerProcessingNote(en, { providerUsed: 'azure-openai', residency: 'eu', privacyTier: 'zdr_contractual' })?.text,
    ).toBe('Processed: EU (Azure), ZDR')
    expect(answerProcessingNote(en, { residency: 'global', privacyTier: 'global' })?.text).toBe(
      'Processed: global, no EU or ZDR guarantee',
    )
  })

  it('never says that nothing is stored; the hover says the residency is the deployment’s declaration', () => {
    const note = answerProcessingNote(no, { residency: 'eu', privacyTier: 'zdr_contractual' })
    expect(note?.title).toContain('Ikke uavhengig verifisert')
    for (const text of [note?.text ?? '', note?.title ?? '']) {
      expect(text).not.toMatch(/ingenting lagres|nothing is stored/i)
    }
  })
})
