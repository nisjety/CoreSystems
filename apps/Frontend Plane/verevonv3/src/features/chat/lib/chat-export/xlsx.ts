/**
 * `buildXlsx` — a finished answer as a valid SpreadsheetML (.xlsx) workbook:
 * one worksheet per table in the answer, or (when the answer has no table at
 * all) a single "Svar"/"Answer" sheet listing its headings, paragraphs and
 * list items as rows.
 *
 * Every cell is written as an inline string (`t="inlineStr"`) rather than a
 * shared-string reference, which is what lets this module skip the
 * `xl/sharedStrings.xml` part entirely — there is exactly one workbook-wide
 * string table to maintain otherwise, and a chat answer's numbers vastly
 * outnumber its repeated text, so the size trade-off shared strings exist
 * for barely applies here.
 *
 * Pure — see `docx.ts`'s header comment for why, and why the one
 * `chat-types.ts` import below is `import type` only.
 */

import type { MarkdownBlock } from '../../components/chat-types'
// Explicit `.ts` extensions: see `ooxml.ts`'s header comment — Node's ESM
// resolver needs them for `sample-exports.mts` to run this module directly.
import { buildAppPropsXml, buildCorePropsXml, buildPackageRelsXml, encodeUtf8, escapeXmlAttr, escapeXmlText, stripInlineMarkdown, XML_DECLARATION } from './ooxml.ts'
import { buildZip, type ZipEntry } from './zip.ts'
import type { ExportLocale, ExportMeta } from './types.ts'

const SS_NS = 'http://schemas.openxmlformats.org/spreadsheetml/2006/main'

/**
 * Converts a cell's raw text to a NUMBER only when the reading is
 * unambiguous, in this priority order:
 *
 *  1. Norwegian thousands grouping — digits grouped in 3s by a space,
 *     NBSP (` `) or narrow NBSP (` `), with an optional
 *     comma-decimal tail: `1 234 567,89`.
 *  2. Plain Norwegian decimal comma, no grouping: `12,5`.
 *  3. Plain integer: `-12`.
 *  4. Dot-decimal with no other separators, EXCEPT exactly three digits
 *     after the dot — that reading is indistinguishable from a
 *     thousands-grouped integer in the many locales that use `.` as the
 *     grouping character, so `1.234` stays text while `12.5` and `12.50`
 *     (and anything with 4+ fractional digits) convert.
 *
 * Everything else — a percentage, a currency amount, a date, `1.234` — is
 * left as text: guessing wrong here would silently change what the answer
 * said, and Excel already renders a text cell that happens to look numeric
 * just fine.
 */
export function detectExcelNumber(raw: string): number | null {
  const trimmed = raw.trim()
  if (!trimmed) return null

  const grouped = /^(-?)(\d{1,3}(?:[   ]\d{3})+)(?:,(\d+))?$/.exec(trimmed)
  if (grouped) {
    const sign = grouped[1] ?? ''
    const integerPart = (grouped[2] ?? '').replace(/[   ]/g, '')
    const fraction = grouped[3]
    const value = Number(`${sign}${integerPart}${fraction ? `.${fraction}` : ''}`)
    return Number.isFinite(value) ? value : null
  }

  if (/^-?\d+,\d+$/.test(trimmed)) {
    const value = Number(trimmed.replace(',', '.'))
    return Number.isFinite(value) ? value : null
  }

  if (/^-?\d+$/.test(trimmed)) {
    const value = Number(trimmed)
    return Number.isFinite(value) ? value : null
  }

  const dotDecimal = /^-?\d+\.(\d+)$/.exec(trimmed)
  if (dotDecimal && (dotDecimal[1] ?? '').length !== 3) {
    const value = Number(trimmed)
    return Number.isFinite(value) ? value : null
  }

  return null
}

/** Column reference letters: 0 -> A, 25 -> Z, 26 -> AA, ... */
export function columnLetter(index: number): string {
  let value = index
  let letters = ''
  do {
    letters = String.fromCharCode(65 + (value % 26)) + letters
    value = Math.floor(value / 26) - 1
  } while (value >= 0)
  return letters
}

const ILLEGAL_SHEET_NAME_CHARS = /[[\]:*?/\\]/g

/**
 * Excel sheet-name rules: at most 31 characters, none of `[ ] : * ? / \`, and
 * unique within the workbook. `used` accumulates lowercase names already
 * assigned so a second table under the same heading (or two headings with
 * the same text) gets " (2)", " (3)", ... instead of a silent collision.
 */
export function sanitizeSheetName(rawName: string, used: Set<string>): string {
  const stripped = rawName.replace(ILLEGAL_SHEET_NAME_CHARS, '').trim()
  const base = (stripped || 'Sheet').slice(0, 31)
  let candidate = base
  let suffix = 2
  while (used.has(candidate.toLowerCase())) {
    const marker = ` (${suffix})`
    candidate = `${base.slice(0, Math.max(1, 31 - marker.length))}${marker}`
    suffix += 1
  }
  used.add(candidate.toLowerCase())
  return candidate
}

type SheetPlan = { name: string; header: string[] | null; rows: string[][] }

/** The heading text immediately above each table, if any — a much better sheet name than "Tabell 1" when the answer actually has one. */
function collectTableSheets(blocks: readonly MarkdownBlock[], locale: ExportLocale): SheetPlan[] {
  const used = new Set<string>()
  const sheets: SheetPlan[] = []
  let pendingHeading: string | null = null
  let tableCount = 0

  const visit = (list: readonly MarkdownBlock[]) => {
    for (const block of list) {
      if (block.kind === 'heading') {
        // Plain text only: the raw markdown heading can carry a link
        // ("[Salg](url)") or emphasis ("**Salg**") that `sanitizeSheetName`
        // does not understand — it only strips the characters Excel bans
        // from a sheet name, so a literal `[`/`]` around a link left the URL
        // fragment stuck onto the name instead of dropping with it.
        pendingHeading = stripInlineMarkdown(block.text)
        continue
      }
      if (block.kind === 'table') {
        tableCount += 1
        const fallback = locale === 'en' ? `Table ${tableCount}` : `Tabell ${tableCount}`
        sheets.push({
          name: sanitizeSheetName(pendingHeading || fallback, used),
          header: block.header,
          rows: block.rows,
        })
        pendingHeading = null
        continue
      }
      if (block.kind === 'details') {
        visit(block.blocks)
        continue
      }
      // Any other block breaks the "heading immediately above a table"
      // adjacency — a paragraph between a heading and its table means the
      // heading was talking about something else by the time the table
      // arrives.
      pendingHeading = null
    }
  }

  visit(blocks)
  return sheets
}

/** Flattened plain-text lines for the answer-has-no-table fallback sheet: every heading, paragraph and list item, recursing into `<details>` bodies. */
function collectFallbackLines(blocks: readonly MarkdownBlock[]): string[] {
  const lines: string[] = []
  const visit = (list: readonly MarkdownBlock[]) => {
    for (const block of list) {
      if (block.kind === 'heading' && block.text) lines.push(stripInlineMarkdown(block.text))
      else if (block.kind === 'paragraph' && block.text) lines.push(stripInlineMarkdown(block.text))
      else if (block.kind === 'list') {
        for (const item of block.items) lines.push(stripInlineMarkdown(item.text))
      } else if (block.kind === 'details') {
        if (block.summary) lines.push(stripInlineMarkdown(block.summary))
        visit(block.blocks)
      }
    }
  }
  visit(blocks)
  return lines
}

function cellXml(ref: string, rawText: string, bold: boolean): string {
  const plain = stripInlineMarkdown(rawText)
  const numeric = detectExcelNumber(plain)
  const style = bold ? ' s="1"' : ''
  if (numeric !== null) {
    return `<c r="${ref}"${style}><v>${numeric}</v></c>`
  }
  return `<c r="${ref}" t="inlineStr"${style}><is><t xml:space="preserve">${escapeXmlText(plain)}</t></is></c>`
}

function estimateColumnWidth(values: readonly string[]): number {
  const longest = values.reduce((max, value) => Math.max(max, stripInlineMarkdown(value).length), 0)
  return Math.min(60, Math.max(8, longest + 2))
}

function buildSheetXml(plan: SheetPlan): string {
  const columnCount = plan.header ? plan.header.length : 1
  const colWidths = Array.from({ length: columnCount }, (_, col) => {
    const values = plan.header ? [plan.header[col] ?? '', ...plan.rows.map((row) => row[col] ?? '')] : plan.rows.map((row) => row[0] ?? '')
    return estimateColumnWidth(values)
  })
  const cols = colWidths
    .map((width, index) => `<col min="${index + 1}" max="${index + 1}" width="${width}" customWidth="1"/>`)
    .join('')

  const rowsXml: string[] = []
  let rowNumber = 1
  if (plan.header) {
    const cells = plan.header
      .map((text, col) => cellXml(`${columnLetter(col)}${rowNumber}`, text, true))
      .join('')
    rowsXml.push(`<row r="${rowNumber}">${cells}</row>`)
    rowNumber += 1
  }
  for (const row of plan.rows) {
    const cells = row.map((text, col) => cellXml(`${columnLetter(col)}${rowNumber}`, text, false)).join('')
    rowsXml.push(`<row r="${rowNumber}">${cells}</row>`)
    rowNumber += 1
  }

  const freezeRow = plan.header ? 1 : 0
  const pane = freezeRow
    ? `<pane ySplit="${freezeRow}" topLeftCell="A${freezeRow + 1}" activePane="bottomLeft" state="frozen"/>`
    : ''

  return (
    XML_DECLARATION +
    `<worksheet xmlns="${SS_NS}">` +
    `<sheetViews><sheetView workbookViewId="0">${pane}</sheetView></sheetViews>` +
    `<cols>${cols}</cols>` +
    `<sheetData>${rowsXml.join('')}</sheetData>` +
    '</worksheet>'
  )
}

function buildFallbackSheet(blocks: readonly MarkdownBlock[], meta: ExportMeta): SheetPlan {
  const locale = meta.locale ?? 'no'
  const lines = collectFallbackLines(blocks)
  return {
    name: locale === 'en' ? 'Answer' : 'Svar',
    header: [locale === 'en' ? 'Answer' : 'Svar'],
    rows: lines.map((line) => [line]),
  }
}

function buildStylesXml(): string {
  return (
    XML_DECLARATION +
    `<styleSheet xmlns="${SS_NS}">` +
    '<fonts count="2"><font><sz val="11"/><name val="Calibri"/></font><font><b/><sz val="11"/><name val="Calibri"/></font></fonts>' +
    '<fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills>' +
    '<borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders>' +
    '<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>' +
    '<cellXfs count="2">' +
    '<xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/>' +
    '<xf numFmtId="0" fontId="1" fillId="0" borderId="0" xfId="0" applyFont="1"/>' +
    '</cellXfs>' +
    '<cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles>' +
    '</styleSheet>'
  )
}

function buildWorkbookXml(sheets: readonly SheetPlan[]): string {
  const entries = sheets
    .map((sheet, index) => `<sheet name="${escapeXmlAttr(sheet.name)}" sheetId="${index + 1}" r:id="rId${index + 1}"/>`)
    .join('')
  return (
    XML_DECLARATION +
    `<workbook xmlns="${SS_NS}" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">` +
    `<sheets>${entries}</sheets>` +
    '</workbook>'
  )
}

function buildWorkbookRelsXml(sheetCount: number): string {
  const sheetRels = Array.from(
    { length: sheetCount },
    (_, index) =>
      `<Relationship Id="rId${index + 1}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet${index + 1}.xml"/>`,
  ).join('')
  const stylesRel = `<Relationship Id="rId${sheetCount + 1}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>`
  return (
    XML_DECLARATION +
    '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">' +
    sheetRels +
    stylesRel +
    '</Relationships>'
  )
}

function buildContentTypesXml(sheetCount: number): string {
  const sheetOverrides = Array.from(
    { length: sheetCount },
    (_, index) =>
      `<Override PartName="/xl/worksheets/sheet${index + 1}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>`,
  ).join('')
  return (
    XML_DECLARATION +
    '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">' +
    '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>' +
    '<Default Extension="xml" ContentType="application/xml"/>' +
    '<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>' +
    '<Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>' +
    sheetOverrides +
    '<Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/>' +
    '<Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/>' +
    '</Types>'
  )
}

/** Builds a complete, valid SpreadsheetML (.xlsx) workbook for one finished chat answer. */
export function buildXlsx(blocks: readonly MarkdownBlock[], meta: ExportMeta): Uint8Array {
  const locale = meta.locale ?? 'no'
  const tableSheets = collectTableSheets(blocks, locale)
  const sheets = tableSheets.length > 0 ? tableSheets : [buildFallbackSheet(blocks, meta)]

  const entries: ZipEntry[] = [
    { name: '[Content_Types].xml', data: encodeUtf8(buildContentTypesXml(sheets.length)) },
    { name: '_rels/.rels', data: encodeUtf8(buildPackageRelsXml('xl/workbook.xml')) },
    { name: 'xl/workbook.xml', data: encodeUtf8(buildWorkbookXml(sheets)) },
    { name: 'xl/_rels/workbook.xml.rels', data: encodeUtf8(buildWorkbookRelsXml(sheets.length)) },
    { name: 'xl/styles.xml', data: encodeUtf8(buildStylesXml()) },
    { name: 'docProps/core.xml', data: encodeUtf8(buildCorePropsXml(meta.title, meta.createdAt)) },
    { name: 'docProps/app.xml', data: encodeUtf8(buildAppPropsXml()) },
  ]
  sheets.forEach((sheet, index) => {
    entries.push({ name: `xl/worksheets/sheet${index + 1}.xml`, data: encodeUtf8(buildSheetXml(sheet)) })
  })
  return buildZip(entries)
}
