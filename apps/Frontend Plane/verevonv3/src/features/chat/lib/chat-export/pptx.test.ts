import { describe, expect, it } from 'vitest'
import type { MarkdownBlock } from '../../components/chat-types'
import { buildPptx } from './pptx'
import { readZipText, unzip } from './unzip-for-tests'

function slideCount(bytes: Uint8Array): number {
  return unzip(bytes).filter((entry) => /^ppt\/slides\/slide\d+\.xml$/.test(entry.name)).length
}

describe('buildPptx', () => {
  it('writes every required OOXML part for a strict PowerPoint import', () => {
    const names = unzip(buildPptx([{ kind: 'paragraph', text: 'Hei' }], { title: 'Test' })).map((entry) => entry.name)
    for (const required of [
      '[Content_Types].xml',
      '_rels/.rels',
      'ppt/presentation.xml',
      'ppt/_rels/presentation.xml.rels',
      'ppt/presProps.xml',
      'ppt/viewProps.xml',
      'ppt/tableStyles.xml',
      'ppt/theme/theme1.xml',
      'ppt/slideMasters/slideMaster1.xml',
      'ppt/slideMasters/_rels/slideMaster1.xml.rels',
      'ppt/slideLayouts/slideLayout1.xml',
      'ppt/slideLayouts/_rels/slideLayout1.xml.rels',
      'ppt/slides/slide1.xml',
      'ppt/slides/_rels/slide1.xml.rels',
      'docProps/core.xml',
      'docProps/app.xml',
    ]) {
      expect(names).toContain(required)
    }
  })

  it('declares a 16:9 canvas', () => {
    const presentation = readZipText(buildPptx([{ kind: 'paragraph', text: 'Hei' }], { title: 'Test' }), 'ppt/presentation.xml')
    expect(presentation).toContain('cx="12192000" cy="6858000"')
  })

  it("theme1.xml's fmtScheme has at least 3 entries in every required list", () => {
    const theme = readZipText(buildPptx([{ kind: 'paragraph', text: 'Hei' }], { title: 'Test' }), 'ppt/theme/theme1.xml')
    const entryTagByList: Record<string, string> = {
      fillStyleLst: 'solidFill',
      lnStyleLst: 'ln',
      effectStyleLst: 'effectStyle',
      bgFillStyleLst: 'solidFill',
    }
    for (const [listTag, entryTag] of Object.entries(entryTagByList)) {
      const [, body] = new RegExp(`<a:${listTag}>([\\s\\S]*?)</a:${listTag}>`).exec(theme) ?? []
      expect(body, `${listTag} present`).toBeTruthy()
      const entryCount = (body?.match(new RegExp(`<a:${entryTag}\\b`, 'g')) ?? []).length
      expect(entryCount, `${listTag} has >= 3 entries`).toBeGreaterThanOrEqual(3)
    }
  })

  it('makes slide 1 the title + date, with no content slide for it', () => {
    const bytes = buildPptx([{ kind: 'heading', level: 1, text: 'Kapittel 1' }, { kind: 'paragraph', text: 'Innhold' }], {
      title: 'Kvartalsrapport',
      createdAt: '2026-09-28T00:00:00Z',
    })
    const slide1 = readZipText(bytes, 'ppt/slides/slide1.xml')
    expect(slide1).toContain('Kvartalsrapport')
    expect(slide1).toContain('2026')
    expect(slide1).not.toContain('Innhold')
  })

  it('starts a new slide at each level-1/2 heading', () => {
    const blocks: MarkdownBlock[] = [
      { kind: 'heading', level: 1, text: 'Første del' },
      { kind: 'paragraph', text: 'Tekst en' },
      { kind: 'heading', level: 2, text: 'Andre del' },
      { kind: 'paragraph', text: 'Tekst to' },
    ]
    const bytes = buildPptx(blocks, { title: 'Test' })
    expect(slideCount(bytes)).toBe(3) // title + 2 content slides
    const slide2 = readZipText(bytes, 'ppt/slides/slide2.xml')
    const slide3 = readZipText(bytes, 'ppt/slides/slide3.xml')
    expect(slide2).toContain('Første del')
    expect(slide2).toContain('Tekst en')
    expect(slide3).toContain('Andre del')
    expect(slide3).toContain('Tekst to')
  })

  it('renders a level-3 heading as a bold paragraph, not a new slide', () => {
    const blocks: MarkdownBlock[] = [
      { kind: 'heading', level: 1, text: 'Del 1' },
      { kind: 'heading', level: 3, text: 'Underoverskrift' },
      { kind: 'paragraph', text: 'Detalj' },
    ]
    const bytes = buildPptx(blocks, { title: 'Test' })
    expect(slideCount(bytes)).toBe(2) // title + one content slide
    const slide2 = readZipText(bytes, 'ppt/slides/slide2.xml')
    expect(slide2).toContain('Underoverskrift')
    expect(slide2).toContain('<a:rPr sz="2000" b="1" dirty="0"></a:rPr><a:t>Underoverskrift</a:t>')
  })

  it('renders a table as a real a:tbl inside a p:graphicFrame', () => {
    const blocks: MarkdownBlock[] = [
      { kind: 'heading', level: 1, text: 'Tall' },
      { kind: 'table', align: [null, null], header: ['Produkt', 'Pris'], rows: [['Kaffe', '45']] },
    ]
    const slide2 = readZipText(buildPptx(blocks, { title: 'Test' }), 'ppt/slides/slide2.xml')
    expect(slide2).toContain('<p:graphicFrame>')
    expect(slide2).toContain('<a:tbl>')
    expect(slide2).toContain('Produkt')
    expect(slide2).toContain('Kaffe')
  })

  it('strips inline markdown from table cells and keeps a wholly bold totals cell bold', () => {
    const blocks: MarkdownBlock[] = [
      { kind: 'heading', level: 1, text: 'Tall' },
      { kind: 'table', align: [null, null], header: ['Produkt', 'Omsetning'], rows: [['Kaffe', '`45` kr'], ['**Totalt**', '**360 000 kr**']] },
    ]
    const slide2 = readZipText(buildPptx(blocks, { title: 'Test' }), 'ppt/slides/slide2.xml')
    expect(slide2).not.toContain('**')
    expect(slide2).not.toContain('`')
    expect(slide2).toMatch(/<a:rPr sz="1400" b="1" dirty="0"><\/a:rPr><a:t>Totalt<\/a:t>/)
    expect(slide2).toMatch(/<a:rPr sz="1400" b="1" dirty="0"><\/a:rPr><a:t>360 000 kr<\/a:t>/)
    expect(slide2).toMatch(/<a:rPr sz="1400" dirty="0"><\/a:rPr><a:t>Kaffe<\/a:t>/)
  })

  it('splits long content onto a "(forts.)" continuation slide past the bullet budget', () => {
    const items = Array.from({ length: 12 }, (_, index) => ({ depth: 0, ordered: false, text: `Punkt ${index + 1}` }))
    const blocks: MarkdownBlock[] = [
      { kind: 'heading', level: 1, text: 'Lang liste' },
      { kind: 'list', ordered: false, items },
    ]
    const bytes = buildPptx(blocks, { title: 'Test' })
    expect(slideCount(bytes)).toBe(3) // title + original + continuation
    const slide3 = readZipText(bytes, 'ppt/slides/slide3.xml')
    expect(slide3).toContain('Lang liste (forts.)')
    expect(slide3).toContain('Punkt 12')
  })

  it('never leaves an empty content slide when the first paragraph under a heading alone exceeds the character budget', () => {
    const blocks: MarkdownBlock[] = [
      { kind: 'heading', level: 1, text: 'Del 1' },
      { kind: 'paragraph', text: 'x'.repeat(900) },
    ]
    const bytes = buildPptx(blocks, { title: 'Test' })
    // No continuation slide: the long paragraph is still the very first thing
    // on "Del 1" itself, so there is nothing to split off of yet.
    expect(slideCount(bytes)).toBe(2) // title + one content slide
    const slide2 = readZipText(bytes, 'ppt/slides/slide2.xml')
    expect(slide2).toContain('Del 1')
    expect(slide2).toMatch(/<a:t>x+<\/a:t>/)
  })

  it('escapes & < > and preserves Norwegian æøå in slide text', () => {
    const blocks: MarkdownBlock[] = [
      { kind: 'heading', level: 1, text: 'Del' },
      { kind: 'paragraph', text: 'Salg & <profitt> økte for Blåbær AS' },
    ]
    const slide2 = readZipText(buildPptx(blocks, { title: 'Test' }), 'ppt/slides/slide2.xml')
    expect(slide2).toContain('Salg &amp; &lt;profitt&gt; økte for Blåbær AS')
  })

  it('is well-formed XML for every slide', () => {
    const blocks: MarkdownBlock[] = [
      { kind: 'heading', level: 1, text: 'Del 1' },
      { kind: 'code', lang: 'ts', text: 'const a = 1', closed: true },
      { kind: 'table', align: [null], header: ['A'], rows: [['1']] },
    ]
    const bytes = buildPptx(blocks, { title: 'Test' })
    for (const entry of unzip(bytes).filter((e) => e.name.endsWith('.xml'))) {
      const text = new TextDecoder().decode(entry.data)
      const parsed = new DOMParser().parseFromString(text, 'application/xml')
      expect(parsed.getElementsByTagName('parsererror').length, entry.name).toBe(0)
    }
  })
})
