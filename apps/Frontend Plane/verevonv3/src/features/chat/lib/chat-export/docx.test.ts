import { describe, expect, it } from 'vitest'
import type { MarkdownBlock } from '../../components/chat-types'
import { buildDocx } from './docx'
import { unzip, readZipText } from './unzip-for-tests'

function names(bytes: Uint8Array): string[] {
  return unzip(bytes)
    .map((entry) => entry.name)
    .sort()
}

describe('buildDocx', () => {
  it('writes every required OOXML part', () => {
    const bytes = buildDocx([{ kind: 'paragraph', text: 'Hei' }], { title: 'Test' })
    expect(names(bytes)).toEqual([
      '[Content_Types].xml',
      '_rels/.rels',
      'docProps/app.xml',
      'docProps/core.xml',
      'word/_rels/document.xml.rels',
      'word/document.xml',
      'word/numbering.xml',
      'word/styles.xml',
    ])
  })

  it('prints a leading heading that became the title once, not as Title and Heading both', () => {
    const blocks: MarkdownBlock[] = [
      { kind: 'heading', level: 2, text: 'Kvartalsrapport for Q3' },
      { kind: 'paragraph', text: 'Innhold' },
    ]
    const doc = readZipText(buildDocx(blocks, { title: 'Kvartalsrapport for Q3' }), 'word/document.xml')
    expect(doc.split('Kvartalsrapport for Q3').length - 1).toBe(1)
    expect(doc).toContain('w:val="Title"')
    // A different title keeps the heading: the two lines say different things.
    const other = readZipText(buildDocx(blocks, { title: 'Styremøte' }), 'word/document.xml')
    expect(other).toContain('Kvartalsrapport for Q3')
    expect(other).toContain('Styremøte')
  })

  it('escapes & < > and preserves Norwegian æøå in the document body', () => {
    const bytes = buildDocx([{ kind: 'paragraph', text: 'Salg & <profitt> økte for Blåbær AS' }], { title: 'Rapport' })
    const doc = readZipText(bytes, 'word/document.xml')
    expect(doc).toContain('Salg &amp; &lt;profitt&gt; økte for Blåbær AS')
    expect(doc).not.toContain('<profitt>')
  })

  it('renders a table as a real w:tbl with a bold, repeated header row and column alignment', () => {
    const blocks: MarkdownBlock[] = [
      {
        kind: 'table',
        align: ['left', 'right'],
        header: ['Produkt', 'Pris'],
        rows: [['Kaffe', '45']],
      },
    ]
    const doc = readZipText(buildDocx(blocks, { title: 'Test' }), 'word/document.xml')
    expect(doc).toContain('<w:tbl>')
    expect(doc).toContain('w:tblStyle w:val="VerevonTable"')
    expect(doc).toContain('<w:tblHeader/>')
    expect(doc).toContain('Produkt')
    expect(doc).toContain('w:jc w:val="right"')
  })

  it('gives an ordered list its own restarting w:num, distinct from a second ordered list', () => {
    const blocks: MarkdownBlock[] = [
      {
        kind: 'list',
        ordered: true,
        startNumber: 5,
        items: [
          { depth: 0, ordered: true, text: 'Fem' },
          { depth: 1, ordered: false, text: 'Nøstet punkt' },
          { depth: 0, ordered: true, text: 'Seks' },
        ],
      },
      {
        kind: 'list',
        ordered: true,
        startNumber: 1,
        items: [{ depth: 0, ordered: true, text: 'Første' }],
      },
    ]
    const doc = readZipText(buildDocx(blocks, { title: 'Test' }), 'word/document.xml')
    const numbering = readZipText(buildDocx(blocks, { title: 'Test' }), 'word/numbering.xml')

    // Two distinct restart instances, one per top-level ordered list.
    expect(numbering).toContain('<w:startOverride w:val="5"/>')
    expect(numbering).toContain('<w:startOverride w:val="1"/>')
    // The nested bullet item uses the shared bullet numbering (numId 1), not
    // the ordered list's own restarting instance.
    expect(doc).toContain('Nøstet punkt')
  })

  it('writes a checked task item with a ballot-box prefix', () => {
    const blocks: MarkdownBlock[] = [
      {
        kind: 'list',
        ordered: false,
        items: [
          { depth: 0, ordered: false, text: 'Gjort', checked: true },
          { depth: 0, ordered: false, text: 'Ugjort', checked: false },
        ],
      },
    ]
    const doc = readZipText(buildDocx(blocks, { title: 'Test' }), 'word/document.xml')
    expect(doc).toContain('☑ Gjort')
    expect(doc).toContain('☐ Ugjort')
  })

  it('renders a code block as one xml:space="preserve" paragraph per line, with tabs as w:tab', () => {
    const blocks: MarkdownBlock[] = [{ kind: 'code', lang: 'ts', text: 'const a = 1\n\tconst b = 2', closed: true }]
    const doc = readZipText(buildDocx(blocks, { title: 'Test' }), 'word/document.xml')
    expect(doc).toContain('w:pStyle w:val="Code"')
    expect(doc).toContain('xml:space="preserve"')
    expect(doc).toContain('<w:tab/>')
  })

  it('creates a real external hyperlink relationship for a markdown link', () => {
    const blocks: MarkdownBlock[] = [{ kind: 'paragraph', text: 'Se [Verevon](https://verevon.example/om)' }]
    const bytes = buildDocx(blocks, { title: 'Test' })
    const doc = readZipText(bytes, 'word/document.xml')
    const rels = readZipText(bytes, 'word/_rels/document.xml.rels')
    const relId = /<w:hyperlink r:id="(rId\d+)"/.exec(doc)?.[1]
    expect(relId).toBeTruthy()
    expect(rels).toContain(`Id="${relId}"`)
    expect(rels).toContain('TargetMode="External"')
    expect(rels).toContain('Target="https://verevon.example/om"')
  })

  it('never turns a javascript:/data:/file: markdown link into a real hyperlink', () => {
    const blocks: MarkdownBlock[] = [
      { kind: 'paragraph', text: 'Se [her](javascript:alert(1)) og [dette](data:text/html,x) og [fil](file:///etc/passwd)' },
    ]
    const bytes = buildDocx(blocks, { title: 'Test' })
    const doc = readZipText(bytes, 'word/document.xml')
    const rels = readZipText(bytes, 'word/_rels/document.xml.rels')
    expect(doc).not.toContain('<w:hyperlink')
    expect(doc).toContain('her')
    expect(doc).toContain('dette')
    expect(doc).toContain('fil')
    expect(rels).not.toContain('javascript:')
    expect(rels).not.toContain('data:')
    expect(rels).not.toContain('file:')
  })

  it('shows a source with an unsafe URL scheme as plain text, not a hyperlink relationship', () => {
    const doc = readZipText(
      buildDocx([{ kind: 'paragraph', text: 'Svar' }], {
        title: 'Test',
        sources: [{ title: 'Mistenkelig kilde', url: 'javascript:alert(1)' }],
      }),
      'word/document.xml',
    )
    expect(doc).not.toContain('<w:hyperlink')
    expect(doc).toContain('javascript:alert(1)')
  })

  it('keeps a citation marker as literal text, not a hyperlink', () => {
    const doc = readZipText(
      buildDocx([{ kind: 'paragraph', text: 'Dette er dokumentert [1].' }], { title: 'Test' }),
      'word/document.xml',
    )
    expect(doc).toContain('[1]')
    expect(doc).not.toContain('<w:hyperlink')
  })

  it('adds a "Kilder" section with numbered title + URL when sources are given', () => {
    const doc = readZipText(
      buildDocx([{ kind: 'paragraph', text: 'Svar' }], {
        title: 'Test',
        sources: [{ title: 'NRK-artikkel', url: 'https://nrk.no/x' }],
      }),
      'word/document.xml',
    )
    expect(doc).toContain('Kilder')
    expect(doc).toContain('1. NRK-artikkel')
    expect(doc).toContain('https://nrk.no/x')
  })

  it('omits the "Kilder" section when there are no sources', () => {
    const doc = readZipText(buildDocx([{ kind: 'paragraph', text: 'Svar' }], { title: 'Test' }), 'word/document.xml')
    expect(doc).not.toContain('Kilder')
  })

  it('uses "Sources" in English locale', () => {
    const doc = readZipText(
      buildDocx([{ kind: 'paragraph', text: 'Answer' }], {
        title: 'Test',
        locale: 'en',
        sources: [{ title: 'Example', url: 'https://example.com' }],
      }),
      'word/document.xml',
    )
    expect(doc).toContain('Sources')
  })

  it('adds a small exported-from footer line with the export date', () => {
    const doc = readZipText(
      buildDocx([{ kind: 'paragraph', text: 'Svar' }], { title: 'Test', createdAt: '2026-09-28T00:00:00Z' }),
      'word/document.xml',
    )
    expect(doc).toContain('Eksportert fra Verevon')
  })

  it('puts the title at the top of the document', () => {
    const doc = readZipText(buildDocx([{ kind: 'paragraph', text: 'Innhold' }], { title: 'Kvartalsrapport Q3' }), 'word/document.xml')
    expect(doc.indexOf('Kvartalsrapport Q3')).toBeLessThan(doc.indexOf('Innhold'))
    expect(doc).toContain('w:pStyle w:val="Title"')
  })

  it('is well-formed XML (balanced tags, no stray "<" or "&")', () => {
    const bytes = buildDocx(
      [
        { kind: 'heading', level: 1, text: 'Tittel & <ting>' },
        { kind: 'paragraph', text: 'Blåbær æøå' },
      ],
      { title: 'Test' },
    )
    const doc = readZipText(bytes, 'word/document.xml')
    expect(() => new DOMParser().parseFromString(doc, 'application/xml')).not.toThrow()
    const parsed = new DOMParser().parseFromString(doc, 'application/xml')
    expect(parsed.getElementsByTagName('parsererror').length).toBe(0)
  })
})
