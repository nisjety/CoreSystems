/**
 * A markdown table as text a spreadsheet takes: tab-separated for the
 * clipboard, CSV for a download.
 *
 * Pure and DOM-free (see `spec.ts`). Cells are written as the answer shows
 * them, with the markdown syntax and a trailing `[1]` source marker removed,
 * so a Norwegian "1 234,5" stays "1 234,5" and Excel in a Norwegian locale
 * reads it as a number. The CSV for Norwegian uses semicolons (the comma is
 * the decimal mark there) and a UTF-8 byte order mark, without which Excel
 * opens æøå as mojibake.
 *
 * Table cells are model output. A cell that starts with `=`, `@`, `+` or `-`
 * would run as a formula when the text is pasted or opened in a spreadsheet, so
 * it is written with a leading apostrophe, except a real signed number
 * ("-5", "-3,2 %").
 */

import { cleanChartCell, parseChartNumber } from './spec.ts'

export const CSV_BOM = '\ufeff'

type Locale = 'no' | 'en'

// A signed number as people write it: digits, then at most a unit ("%", "MNOK", "m²", "mill. kr").
// `parseChartNumber` alone is not enough, it also takes "-5+SUM(A)" as the number -5 with a unit.
const SIGNED_NUMBER = /^[-+][\d\s.,]*\d[\d\s.,]*(?:\s?[\p{L}%$€£¥²³°/.]+(?:\s[\p{L}.]+)?)?$/u

function guardCell(cell: string): string {
  if (cell === '') return cell
  const first = cell[0]
  if (first === '=' || first === '@') return `'${cell}`
  if ((first === '+' || first === '-') && !(parseChartNumber(cell) !== null && SIGNED_NUMBER.test(cell))) {
    return `'${cell}`
  }
  return cell
}

function gridOf(header: readonly string[], rows: readonly (readonly string[])[]): string[][] {
  const width = header.length
  const line = (cells: readonly string[]) =>
    Array.from({ length: width }, (_, index) => guardCell(cleanChartCell(cells[index] ?? '')))
  return [line(header), ...rows.map(line)]
}

/** Header and rows as tab-separated lines, no trailing newline: what Excel and Sheets paste as a grid. */
export function tableToTsv(header: readonly string[], rows: readonly (readonly string[])[]): string {
  return gridOf(header, rows)
    .map((row) => row.join('\t'))
    .join('\n')
}

function csvCell(cell: string, separator: string): string {
  return cell.includes(separator) || /["\r\n]/.test(cell) ? `"${cell.replace(/"/g, '""')}"` : cell
}

/** CSV text (no byte order mark, see `csvBytes`): semicolon-separated for `no`, comma for `en`, CRLF line ends. */
export function tableToCsv(
  header: readonly string[],
  rows: readonly (readonly string[])[],
  locale: Locale,
): string {
  const separator = locale === 'en' ? ',' : ';'
  return gridOf(header, rows)
    .map((row) => `${row.map((cell) => csvCell(cell, separator)).join(separator)}\r\n`)
    .join('')
}

const encoder = new TextEncoder()

/** The bytes to save: UTF-8 with a byte order mark (added once). */
export function csvBytes(text: string): Uint8Array {
  return encoder.encode(text.startsWith(CSV_BOM) ? text : `${CSV_BOM}${text}`)
}
