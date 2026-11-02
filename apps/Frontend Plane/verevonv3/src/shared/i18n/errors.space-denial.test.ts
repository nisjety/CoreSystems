import { describe, expect, it } from 'vitest'
import { SPACE_DENIAL_TEXT } from '@/shared/api/space-denials'
import { translateApiError } from './errors'

// WS03-S7: translateApiError hands a typed Space denial to the presenter (so the reason, not the
// operation code, says why) and leaves every other code exactly as it was.

const no = (norwegian: string) => norwegian
const en = (_norwegian: string, english: string) => english

describe('translateApiError for typed Space denials', () => {
  it('delegates to the presenter, with the next step', () => {
    expect(translateApiError({ code: 'forbidden', details: { reason: 'space_role_below_floor' } }, en)).toBe(
      `${SPACE_DENIAL_TEXT.space_role_below_floor.en} Ask a room manager for help.`,
    )
    expect(translateApiError({ code: 'forbidden', details: { reason: 'space_role_below_floor' } }, no)).toBe(
      `${SPACE_DENIAL_TEXT.space_role_below_floor.no} Be en romleder om hjelp.`,
    )
  })

  it('lets the reason win over an operation code that has its own copy', () => {
    const text = translateApiError({ code: 'space_post_not_authorized', details: { reason: 'space_zdr_forbids_effect' } }, en)
    expect(text).toBe(SPACE_DENIAL_TEXT.space_zdr_forbids_effect.en)
  })

  it('accepts a pass-through denial code alone', () => {
    expect(translateApiError({ code: 'space_audience_not_current' }, en)).toBe(
      `${SPACE_DENIAL_TEXT.space_audience_not_current.en} Try again.`,
    )
  })

  it('says the generic line for an unknown reason and never the server text', () => {
    const text = translateApiError({ code: 'forbidden', message: 'server wording', details: { reason: 'space_future_reason' } }, en)
    expect(text).toBe('This is not allowed in this Space right now.')
    expect(text).not.toContain('server wording')
  })

  it('names a switched-off entitlement', () => {
    const text = translateApiError({ details: { reason: 'space_entitlement_off', entitlement: 'import_write' } }, en)
    expect(text).toContain('Import files and sources')
  })
})

describe('translateApiError leaves other codes unchanged', () => {
  it('uses the caller fallback for an unknown code without a reason', () => {
    expect(translateApiError({ code: 'something_new' }, en, { no: 'Reserve', en: 'Fallback copy' })).toBe('Fallback copy')
  })

  it('keeps the catalogue copy of a code that has one, when no reason is carried', () => {
    expect(translateApiError({ code: 'space_post_not_authorized' }, en)).toBe(
      'Your role in this room does not allow posting here. Ask an owner or manager for edit access.',
    )
  })

  it('keeps details that are not a Space denial from changing the result', () => {
    expect(
      translateApiError({ code: 'space_post_not_authorized', details: { other: 'x' } }, en),
    ).toBe('Your role in this room does not allow posting here. Ask an owner or manager for edit access.')
  })

  it('keeps the zero-retention copy ahead of a Space denial', () => {
    const text = translateApiError({ code: 'zdr_unavailable', details: { reason: 'space_role_below_floor' } }, en)
    expect(text).toContain('Zero data retention applies')
  })
})
