import { describe, expect, it } from 'vitest'
import type { MarkdownBlock } from '../../components/chat-types'
import { buildXlsx, columnLetter, detectExcelNumber, sanitizeSheetName } from './xlsx'
import { readZipText, unzip } from './unzip-for-tests'

describe('detectExcelNumber', () => {
  it('converts a plain integer, including negative', () => {
    expect(detectExcelNumber('-12')).toBe(-12)
    expect(detectExcelNumber('12')).toBe(12)
  })

  it('converts a plain Norwegian decimal comma', () => {
    expect(detectExcelNumber('12,5')).toBe(12.5)
  })

  it('converts a dot-decimal with no other separators', () => {
    expect(detectExcelNumber('12.5')).toBe(12.5)
    expect(detectExcelNumber('12.50')).toBe(12.5)
  })

  it('converts Norwegian thousands grouping (space) with a decimal comma', () => {
    expect(detectExcelNumber('1 234 567,89')).toBe(1234567.89)
  })

  it('converts Norwegian thousands grouping with NBSP / narrow NBSP', () => {
    expect(detectExcelNumber('1 234,50')).toBe(1234.5)
    expect(detectExcelNumber('1 234,50')).toBe(1234.5)
  })

  it('leaves "1.234" as text — ambiguous with thousands grouping', () => {
    expect(detectExcelNumber('1.234')).toBeNull()
  })

  it('leaves a percentage as text', () => {
    expect(detectExcelNumber('12,5 %')).toBeNull()
  })

  it('leaves a currency amount as text', () => {
    expect(detectExcelNumber('kr 123')).toBeNull()
    expect(detectExcelNumber('$12.50')).toBeNull()
  })

  it('leaves a date-like string as text', () => {
    expect(detectExcelNumber('12.01.2026')).toBeNull()
  })

  it('leaves an empty or blank string as text', () => {
    expect(detectExcelNumber('')).toBeNull()
    expect(detectExcelNumber('   ')).toBeNull()
  })
})

describe('columnLetter', () => {
  it('maps 0-25 to A-Z and wraps into AA', () => {
    expect(columnLetter(0)).toBe('A')
    expect(columnLetter(25)).toBe('Z')
    expect(columnLetter(26)).toBe('AA')
    expect(columnLetter(27)).toBe('AB')
  })
})

describe('sanitizeSheetName', () => {
  it('strips illegal characters and truncates to 31 characters', () => {
    const used = new Set<string>()
    const name = sanitizeSheetName('Salg [Q3]: 2026/2027 * plan? \\total', used)
    expect(name.length).toBeLessThanOrEqual(31)
    expect(name).not.toMatch(/[[\]:*?/\\]/)
  })

  it('disambiguates a collision by appending a suffix', () => {
    const used = new Set<string>()
    const first = sanitizeSheetName('Tall', used)
    const second = sanitizeSheetName('Tall', used)
    expect(first).toBe('Tall')
    expect(second).toBe('Tall (2)')
  })
})

describe('buildXlsx', () => {
  it('writes one worksheet per table block, with unique sanitized sheet names', () => {
    const blocks: MarkdownBlock[] = [
      { kind: 'heading', level: 2, text: 'Salg [Q3]' },
      { kind: 'table', align: [null, null], header: ['Produkt', 'Antall'], rows: [['Kaffe', '10']] },
      { kind: 'heading', level: 2, text: 'Salg [Q3]' },
      { kind: 'table', align: [null, null], header: ['Produkt', 'Antall'], rows: [['Te', '5']] },
    ]
    const bytes = buildXlsx(blocks, { title: 'Rapport' })
    const workbook = readZipText(bytes, 'xl/workbook.xml')
    expect(workbook).toContain('name="Salg Q3"')
    expect(workbook).toContain('name="Salg Q3 (2)"')
    const sheetEntries = unzip(bytes).filter((entry) => entry.name.startsWith('xl/worksheets/'))
    expect(sheetEntries).toHaveLength(2)
  })

  it('writes cells as inline strings and needs no sharedStrings part', () => {
    const blocks: MarkdownBlock[] = [{ kind: 'table', align: [null], header: ['Navn'], rows: [['Ola']] }]
    const bytes = buildXlsx(blocks, { title: 'Test' })
    expect(unzip(bytes).some((entry) => entry.name === 'xl/sharedStrings.xml')).toBe(false)
    const sheet = readZipText(bytes, 'xl/worksheets/sheet1.xml')
    expect(sheet).toContain('t="inlineStr"')
  })

  it('converts an unambiguous number cell to <v>, leaves an ambiguous one as inline string', () => {
    const blocks: MarkdownBlock[] = [
      { kind: 'table', align: [null, null], header: ['Beløp', 'Andel'], rows: [['1 234,50', '12,5 %']] },
    ]
    const sheet = readZipText(buildXlsx(blocks, { title: 'Test' }), 'xl/worksheets/sheet1.xml')
    expect(sheet).toContain('<v>1234.5</v>')
    expect(sheet).toContain('t="inlineStr"><is><t xml:space="preserve">12,5 %</t>')
  })

  it('bolds the header row via a distinct style index and freezes it', () => {
    const blocks: MarkdownBlock[] = [{ kind: 'table', align: [null], header: ['Kolonne'], rows: [['verdi']] }]
    const sheet = readZipText(buildXlsx(blocks, { title: 'Test' }), 'xl/worksheets/sheet1.xml')
    expect(sheet).toContain('s="1"')
    expect(sheet).toContain('state="frozen"')
    const styles = readZipText(buildXlsx(blocks, { title: 'Test' }), 'xl/styles.xml')
    expect(styles).toContain('<b/>')
  })

  it('strips inline markdown from a cell before writing it', () => {
    const blocks: MarkdownBlock[] = [{ kind: 'table', align: [null], header: ['Note'], rows: [['**viktig** verdi']] }]
    const sheet = readZipText(buildXlsx(blocks, { title: 'Test' }), 'xl/worksheets/sheet1.xml')
    expect(sheet).toContain('viktig verdi')
    expect(sheet).not.toContain('**')
  })

  it('strips inline markdown from a heading before using it as a sheet name', () => {
    const blocks: MarkdownBlock[] = [
      { kind: 'heading', level: 2, text: '[Salgstall](https://example.com/salg)' },
      { kind: 'table', align: [null], header: ['Produkt'], rows: [['Kaffe']] },
    ]
    const bytes = buildXlsx(blocks, { title: 'Rapport' })
    const workbook = readZipText(bytes, 'xl/workbook.xml')
    expect(workbook).toContain('name="Salgstall"')
    expect(workbook).not.toContain('example.com')
    expect(workbook).not.toContain('https')
  })

  it('falls back to a single "Svar" sheet when the answer has no table', () => {
    const blocks: MarkdownBlock[] = [
      { kind: 'heading', level: 2, text: 'Oppsummering' },
      { kind: 'paragraph', text: 'Alt gikk bra.' },
      { kind: 'list', ordered: false, items: [{ depth: 0, ordered: false, text: 'Punkt en' }] },
    ]
    const bytes = buildXlsx(blocks, { title: 'Test' })
    const workbook = readZipText(bytes, 'xl/workbook.xml')
    expect(workbook).toContain('name="Svar"')
    const sheet = readZipText(bytes, 'xl/worksheets/sheet1.xml')
    expect(sheet).toContain('Oppsummering')
    expect(sheet).toContain('Alt gikk bra.')
    expect(sheet).toContain('Punkt en')
  })

  it('escapes & < > and preserves Norwegian æøå in a cell', () => {
    const blocks: MarkdownBlock[] = [{ kind: 'table', align: [null], header: ['Kolonne'], rows: [['Salg & <profitt> økte']] }]
    const sheet = readZipText(buildXlsx(blocks, { title: 'Test' }), 'xl/worksheets/sheet1.xml')
    expect(sheet).toContain('Salg &amp; &lt;profitt&gt; økte')
  })
})
