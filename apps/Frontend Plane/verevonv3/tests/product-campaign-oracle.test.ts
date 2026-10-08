import { describe, expect, it } from 'vitest'
import { campaignCopy, lostPostNotes, postNotes } from './e2e/product-campaign-oracle'

const words = (value: string) => value.match(/[\p{L}\p{N}]+(?:[-’'][\p{L}\p{N}]+)*/gu)?.length ?? 0

const COPY = 'Når dere setter opp en ny arbeidsplass, hvilke detaljer vil dere ha på plass rundt skrivebordet? Lys bordlampe har tre lysnivåer, valgt med én knapp på foten, og en justerbar arm. Den finnes i sand og grafitt, slik at dere kan be om produktarket med de konkrete opplysningene før dere vurderer lampen. Snakk med oss om arbeidsplassen deres dersom dere vil diskutere innredningsbehov.'

describe('campaign copy extraction', () => {
  // Q16g take 1 (2026-09-29): the model put its metadata block, including the
  // "Produktinformasjon teksten bygger på" note, ABOVE the post. The oracle cut
  // the piece at that note and kept only "Kanal" and "Foreslått dato".
  it('keeps the copy when a metadata block with the source note leads the piece', () => {
    const section = [
      '**Status:** Utkast til gjennomgang',
      '**Kanal:** LinkedIn',
      '**Foreslått dato:** 21. september 2026',
      '**Vinkel:** Fleksibel plassering og valg av lysnivå',
      '**Produktinformasjon teksten bygger på:** Tre lysnivåer valgt med en knapp på foten; justerbar arm; fargene sand og grafitt.',
      '',
      COPY,
    ].join('\n')
    const copy = campaignCopy(section)
    expect(copy).toBe(COPY)
    expect(words(copy)).toBeGreaterThanOrEqual(60)
    expect(words(copy)).toBeLessThanOrEqual(90)
  })

  it('does not count Kanal or Foreslått dato as copy', () => {
    expect(campaignCopy(['**Kanal:** LinkedIn', '**Foreslått dato:** 21. september 2026', '', COPY].join('\n'))).toBe(COPY)
  })

  it('still ends the copy at a source note that sits BELOW it', () => {
    const section = [
      '**Status:** Utkast', '', COPY, '',
      '**Produktinformasjon teksten bygger på:** Tre lysnivåer; justerbar arm; fargene sand og grafitt.',
    ].join('\n')
    expect(campaignCopy(section)).toBe(COPY)
  })

  it('drops a leading note whose value is a list, and keeps the copy after it', () => {
    const section = [
      '**Status:** Utkast', '**Produktinformasjon teksten bygger på:**', '- Tre lysnivåer', '- Justerbar arm', '- Fargene sand og grafitt', '', COPY,
    ].join('\n')
    expect(campaignCopy(section)).toBe(COPY)
  })

  it('still cuts a final Kilder appendix and a note below the copy together', () => {
    const section = [COPY, '', '### Kilder', '', '- Kampanjebriefen: Målgruppe, formål', '- Produktarket: Lysnivåer og mål'].join('\n')
    expect(campaignCopy(section)).toBe(COPY)
  })

  it('accepts the label styles a model uses: bold outside the colon, bold around the label, plain', () => {
    for (const labels of [
      ['**Kanal**: LinkedIn', '**Foreslått dato**: 21. september 2026'],
      ['Kanal: LinkedIn', 'Foreslått dato: 21. september 2026'],
      ['**Kanal:** LinkedIn', '**Foreslått dato:** 21. september 2026'],
    ]) expect(campaignCopy([...labels, '', COPY].join('\n'))).toBe(COPY)
  })

  it('finds no copy in a piece that only holds metadata, so a missing body still fails the word minimum', () => {
    const copy = campaignCopy(['**Status:** Utkast', '**Kanal:** LinkedIn', '**Foreslått dato:** 21. september 2026', '**Vinkel:** Pris'].join('\n'))
    expect(copy).toBe('')
    expect(words(copy)).toBe(0)
  })

  it('keeps the copy of an unchanged layout with no metadata at all', () => {
    expect(campaignCopy(COPY)).toBe(COPY)
  })
})

// Q16h (section-only output): the revised post lost its `### Kilder` note in
// 3 of 3 takes that had one, and the copy checks passed every one of them.
describe('source notes under the posts', () => {
  const post = (day: number, body: string, note?: string) =>
    [`## LinkedIn — ${day}. september 2026`, '', '**Status:** Utkast til gjennomgang', '', body, ...(note ? ['', '### Kilder', '', note] : []), ''].join('\n')
  const document = (notes: [string?, string?, string?]) => [
    '# Fjordform — kampanjeutkast', '', '## Foreslått publiseringsoversikt', '', '| 21. september | LinkedIn |', '',
    post(21, COPY, notes[0]), post(23, COPY, notes[1]), post(25, COPY, notes[2]),
    '## E-post', '', COPY, '',
  ].join('\n')

  it('lists the sub-headings of each post by its day', () => {
    const notes = postNotes(document(['- Produktinformasjon brukt: arm.', undefined, '- Bygger på briefen.']))
    expect([...notes.keys()]).toEqual([21, 23, 25])
    expect(notes.get(21)).toEqual(['kilder'])
    expect(notes.get(23)).toEqual([])
    expect(notes.get(25)).toEqual(['kilder'])
  })

  it('finds the post whose note a revision deleted', () => {
    const note = '- Produktinformasjon brukt: arm.'
    expect(lostPostNotes(document([note, note, note]), document([note, undefined, note]))).toEqual([23])
    expect(lostPostNotes(document([note, note, note]), document([undefined, note, undefined]))).toEqual([21, 25])
  })

  it('accepts a note that was reworded, kept, or never there', () => {
    const note = '- Produktinformasjon brukt: arm.'
    expect(lostPostNotes(document([note, note, note]), document([note, '- Produktinformasjon brukt: arm; knapp.', note]))).toEqual([])
    expect(lostPostNotes(document([undefined, undefined, undefined]), document([undefined, undefined, undefined]))).toEqual([])
    expect(lostPostNotes(document([undefined, undefined, undefined]), document([note, note, note]))).toEqual([])
  })

  it('does not mistake the overview, the title or the e-mail for a post', () => {
    expect([...postNotes(document([undefined, undefined, undefined])).keys()]).toEqual([21, 23, 25])
  })

  it('counts a post that vanished from the revised document as having lost its note', () => {
    const note = '- Produktinformasjon brukt: arm.'
    expect(lostPostNotes(document([note, note, note]), document([note, note, note]).replace(/## LinkedIn — 23\. september 2026[\s\S]*?(?=## LinkedIn — 25)/, ''))).toEqual([23])
  })
})
