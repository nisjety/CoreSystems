import { describe, expect, it } from 'vitest'
import { CSV_BOM, csvBytes, tableToCsv, tableToTsv } from './table-text'

const HEADER = ['Kvartal', 'Omsetning', 'Margin']
const ROWS = [
  ['K1', '1 200,5', '12 %'],
  ['K2', '1 350', '14 %'],
]

describe('tableToTsv', () => {
  it('writes the header and rows separated by tabs and newlines, cells as typed', () => {
    expect(tableToTsv(HEADER, ROWS)).toBe('Kvartal\tOmsetning\tMargin\nK1\t1 200,5\t12 %\nK2\t1 350\t14 %')
  })

  it('strips inline markdown and a trailing source marker', () => {
    expect(tableToTsv(['**Navn**', 'Verdi'], [['[A](https://example.com)', '**5**[1]']])).toBe('Navn\tVerdi\nA\t5')
  })

  it('folds tabs and line breaks inside a cell so the grid stays a grid', () => {
    expect(tableToTsv(['a'], [['x\ty\nz']])).toBe('a\nx y z')
  })

  it('pads a short row to the header width and drops cells beyond it', () => {
    expect(tableToTsv(['a', 'b'], [['1'], ['2', '3', '4']])).toBe('a\tb\n1\t\n2\t3')
  })

  it('keeps a spreadsheet from running a cell as a formula', () => {
    const text = tableToTsv(['a'], [['=SUM(A1:A9)'], ['@cmd'], ['+1+1'], ['-abc']])
    expect(text.split('\n').slice(1)).toEqual(["'=SUM(A1:A9)", "'@cmd", "'+1+1", "'-abc"])
  })

  it('leaves real negative and signed numbers alone', () => {
    const text = tableToTsv(['a'], [['-5'], ['-3,2 %'], ['+4'], ['-1 234,5']])
    expect(text.split('\n').slice(1)).toEqual(['-5', '-3,2 %', '+4', '-1 234,5'])
  })

  it('leaves a signed number with a unit alone, but not one followed by an operator or a call', () => {
    const units = ['-5 MNOK', '-12 kr', '-5 m²', '-5 mill. kr']
    expect(tableToTsv(['a'], units.map((cell) => [cell])).split('\n').slice(1)).toEqual(units)
    // `parseChartNumber` reads these as a number with an odd unit; a spreadsheet would run them.
    const formulas = ['-5+SUM(A)', '-2|x', '-5(A)', '-5=A', '-5@A', '-1+cmd|x']
    expect(tableToTsv(['a'], formulas.map((cell) => [cell])).split('\n').slice(1)).toEqual(formulas.map((cell) => `'${cell}`))
  })
})

describe('tableToCsv', () => {
  it('uses semicolons for Norwegian, with CRLF line ends', () => {
    expect(tableToCsv(HEADER, ROWS, 'no')).toBe('Kvartal;Omsetning;Margin\r\nK1;1 200,5;12 %\r\nK2;1 350;14 %\r\n')
  })

  it('uses commas for English', () => {
    expect(tableToCsv(['a', 'b'], [['1', '2']], 'en')).toBe('a,b\r\n1,2\r\n')
  })

  it('quotes a cell that holds the separator or a quote, and folds a line break into a space', () => {
    expect(tableToCsv(['a', 'b'], [['x;y', 'say "hi"'], ['line\nbreak', 'ok']], 'no')).toBe(
      'a;b\r\n"x;y";"say ""hi"""\r\nline break;ok\r\n',
    )
    expect(tableToCsv(['a'], [['1,5']], 'en')).toBe('a\r\n"1,5"\r\n')
  })

  it('does not quote a comma-decimal in a semicolon file, so Excel reads the number', () => {
    expect(tableToCsv(['a'], [['1 234,5']], 'no')).toBe('a\r\n1 234,5\r\n')
  })

  it('neutralises formulas in a downloaded file', () => {
    const text = tableToCsv(['a', 'b'], [['=HYPERLINK("http://x","y")', '\tcmd'], ['@x', '\rcmd']], 'no')
    expect(text).not.toMatch(/(^|;|\n)=/)
    expect(text).toContain("\"'=HYPERLINK(\"\"http://x\"\",\"\"y\"\")\"")
  })

  it('keeps unicode text intact', () => {
    expect(tableToCsv(['Sted'], [['Tromsø – Ålesund']], 'no')).toBe('Sted\r\nTromsø – Ålesund\r\n')
  })
})

describe('csvBytes', () => {
  it('starts with a UTF-8 byte order mark so Excel opens æøå correctly', () => {
    const bytes = csvBytes('Sted;Navn\r\nTromsø;Å\r\n')
    expect([...bytes.slice(0, 3)]).toEqual([0xef, 0xbb, 0xbf])
    expect(new TextDecoder('utf-8', { ignoreBOM: true }).decode(bytes)).toBe(`${CSV_BOM}Sted;Navn\r\nTromsø;Å\r\n`)
    expect(new TextDecoder().decode(bytes)).toBe('Sted;Navn\r\nTromsø;Å\r\n')
  })

  it('does not add a second mark to text that already has one', () => {
    const bytes = csvBytes(`${CSV_BOM}a\r\n`)
    expect([...bytes.slice(0, 6)]).not.toEqual([0xef, 0xbb, 0xbf, 0xef, 0xbb, 0xbf])
  })
})
