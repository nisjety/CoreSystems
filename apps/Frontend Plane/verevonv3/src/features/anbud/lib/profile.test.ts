import { describe, expect, it } from 'vitest'
import { emptyProfileDraft, parseProfile, type ProfileDraft } from './profile'

function draft(overrides: Partial<ProfileDraft>): ProfileDraft {
  return { ...emptyProfileDraft, ...overrides }
}

describe('tender profile form', () => {
  it('reads codes and keywords, and writes regions in capitals', () => {
    const parsed = parseProfile(
      draft({
        orgNumber: '974 760 673',
        cpv: '72212000, 72000000 45000000',
        nuts: 'no08 no081',
        keywords: ' fagsystem , drift ',
      }),
    )

    expect(parsed).toEqual({
      profile: {
        orgNumber: '974760673',
        cpv: ['72212000', '72000000', '45000000'],
        nuts: ['NO08', 'NO081'],
        keywords: ['fagsystem', 'drift'],
        valueRange: undefined,
      },
    })
  })

  it('keeps a value range in kroner when either end is given', () => {
    const parsed = parseProfile(draft({ cpv: '72212000', valueMin: '200000' }))

    expect(parsed).toEqual({
      profile: expect.objectContaining({ valueRange: { currency: 'NOK', min: 200000, max: undefined } }),
    })
  })

  it.each([
    ['a CPV code of seven digits', draft({ cpv: '7221200' }), 'cpv'],
    ['an organisation number of eight digits', draft({ cpv: '72212000', orgNumber: '97476067' }), 'orgNumber'],
    ['a region that is not a region code', draft({ cpv: '72212000', nuts: 'NORWAY' }), 'nuts'],
    ['a value that is not whole kroner', draft({ cpv: '72212000', valueMin: '2 000' }), 'value'],
    ['a range where from is above to', draft({ cpv: '72212000', valueMin: '9', valueMax: '3' }), 'value'],
    ['neither a CPV code nor a keyword', draft({ nuts: 'NO08' }), 'cpv'],
  ])('names the field that has %s', (_label, input, field) => {
    const parsed = parseProfile(input)

    expect(parsed).toEqual({ problems: [expect.objectContaining({ field })] })
  })

  it('gives every problem in both languages', () => {
    const parsed = parseProfile(draft({ cpv: '7221200' }))

    expect('problems' in parsed && parsed.problems[0]?.message.no).toBeTruthy()
    expect('problems' in parsed && parsed.problems[0]?.message.en).toBeTruthy()
  })
})
