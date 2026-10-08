/**
 * The chart model of the chat: what a ```chart fence says, what a markdown
 * table can be read as, and which chart types a given dataset may be shown as.
 *
 * Pure and DOM-free, like `chat-export/`: no Solid, no `window`. Imports use
 * explicit `.ts` extensions so Node can load this module directly, the way the
 * export builders are loaded by `sample-exports.mts` (the Office exports of H3
 * and H4 reuse this model and `geometry.ts`).
 *
 * Two rules run through the whole file:
 *  - A bad spec NEVER throws. `parseChartFence` answers `{ ok: false, reason }`
 *    and the chat falls back to the plain code block with a one-line note.
 *  - Nothing is guessed silently. A table that cannot be charted honestly
 *    (a gap in a column, more than 24 rows, a percentage column beside a
 *    currency column) leaves the column out or returns `null`; it never
 *    invents a number or cuts data without saying so.
 */

import { stripInlineMarkdown } from '../chat-export/ooxml.ts'
import { detectExcelNumber } from '../chat-export/xlsx.ts'

export type ChartType = 'bar' | 'line' | 'area' | 'pie'

export type ChartSeries = {
  name: string
  /** One finite number per category, same order. */
  values: number[]
}

export type ChartSpec = {
  type: ChartType
  /** '' when the chart has no title. */
  title: string
  categories: string[]
  series: ChartSeries[]
  /** '' when unitless. Shown after values ("1 200 MNOK", "12 %"). */
  unit: string
  /** Bars and areas only; always false for a line or a pie. */
  stacked: boolean
}

/** Limits of the fence. The renderer lays out up to these and no more. */
export const CHART_MAX_CATEGORIES = 24
export const CHART_MAX_SERIES = 6
export const CHART_MAX_PIE_SLICES = 8
/** A chart fence is a few hundred bytes; a source this large is not one. */
export const CHART_MAX_SOURCE_CHARS = 20_000
const MAX_TITLE_CHARS = 140
const MAX_UNIT_CHARS = 16
const MAX_NAME_CHARS = 60

export type ChartParseFailure =
  | 'empty'
  | 'too-large'
  | 'not-json'
  | 'not-object'
  | 'bad-type'
  | 'no-categories'
  | 'too-many-categories'
  | 'no-series'
  | 'too-many-series'
  | 'too-many-slices'
  | 'bad-values'
  | 'length-mismatch'
  | 'pie-needs-one-series'
  | 'pie-values'

export type ChartParseResult = { ok: true; spec: ChartSpec } | { ok: false; reason: ChartParseFailure }

const CHART_TYPES: readonly ChartType[] = ['bar', 'line', 'area', 'pie']

function fail(reason: ChartParseFailure): ChartParseResult {
  return { ok: false, reason }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

/** An own property only: a key the model wrote, never one inherited from `Object.prototype`. */
function own(record: Record<string, unknown>, key: string): unknown {
  return Object.hasOwn(record, key) ? record[key] : undefined
}

function readType(value: unknown): ChartType | null {
  if (typeof value !== 'string') return null
  const normalized = value.trim().toLowerCase()
  // `column` is what most chart libraries call a vertical bar chart.
  if (normalized === 'column') return 'bar'
  return CHART_TYPES.find((type) => type === normalized) ?? null
}

function readText(value: unknown, max: number): string {
  if (typeof value === 'number' && Number.isFinite(value)) return String(value).slice(0, max)
  return typeof value === 'string' ? value.trim().slice(0, max) : ''
}

function readCategories(value: unknown): string[] | null {
  if (!Array.isArray(value) || value.length === 0) return null
  const categories: string[] = []
  for (const entry of value) {
    if (typeof entry === 'string') categories.push(entry.trim())
    else if (typeof entry === 'number' && Number.isFinite(entry)) categories.push(String(entry))
    else return null
  }
  return categories
}

function readValue(value: unknown): number | null {
  if (typeof value === 'number') return Number.isFinite(value) ? value : null
  // A model sometimes writes "1 234,5" for a number: the same reader the
  // spreadsheet export uses decides, so a chart and an exported sheet agree.
  if (typeof value === 'string') return detectExcelNumber(value)
  return null
}

/**
 * Reads the body of a ```chart fence: one JSON object
 * `{ type, title?, x | categories, series: [{ name, values }], unit?, stacked? }`.
 *
 * Never throws. A source that is cut off mid-stream is `not-json`, which is
 * why the markdown view only calls this once the fence has closed.
 */
export function parseChartFence(source: string): ChartParseResult {
  const text = source.trim()
  if (!text) return fail('empty')
  if (text.length > CHART_MAX_SOURCE_CHARS) return fail('too-large')

  let raw: unknown
  try {
    raw = JSON.parse(text)
  } catch {
    return fail('not-json')
  }
  if (!isRecord(raw)) return fail('not-object')

  const type = readType(own(raw, 'type'))
  if (!type) return fail('bad-type')

  const categories = readCategories(own(raw, 'x') ?? own(raw, 'categories'))
  if (!categories) return fail('no-categories')
  if (categories.length > CHART_MAX_CATEGORIES) return fail('too-many-categories')

  const rawSeries = own(raw, 'series')
  if (!Array.isArray(rawSeries) || rawSeries.length === 0) return fail('no-series')
  if (rawSeries.length > CHART_MAX_SERIES) return fail('too-many-series')

  const series: ChartSeries[] = []
  for (const [index, entry] of rawSeries.entries()) {
    if (!isRecord(entry)) return fail('bad-values')
    const rawValues = own(entry, 'values')
    if (!Array.isArray(rawValues)) return fail('bad-values')
    const values: number[] = []
    for (const rawValue of rawValues) {
      const value = readValue(rawValue)
      if (value === null) return fail('bad-values')
      values.push(value)
    }
    if (values.length !== categories.length) return fail('length-mismatch')
    series.push({ name: readText(own(entry, 'name'), MAX_NAME_CHARS) || `Serie ${index + 1}`, values })
  }

  if (type === 'pie') {
    if (series.length !== 1) return fail('pie-needs-one-series')
    if (categories.length > CHART_MAX_PIE_SLICES) return fail('too-many-slices')
    const values = series[0]?.values ?? []
    if (values.some((value) => value < 0) || values.reduce((sum, value) => sum + value, 0) <= 0) {
      return fail('pie-values')
    }
  }

  return {
    ok: true,
    spec: {
      type,
      title: readText(own(raw, 'title'), MAX_TITLE_CHARS),
      categories,
      series,
      unit: readText(own(raw, 'unit'), MAX_UNIT_CHARS),
      stacked: own(raw, 'stacked') === true && (type === 'bar' || type === 'area'),
    },
  }
}

// ── Numbers as people write them in a table ─────────────────────────────────

const CURRENCY_PREFIX = /^(NOK|SEK|DKK|EUR|USD|GBP|kr\.?|[$€£¥])\s*(?=[-+]?\d)/i
// A unit is one token ("%", "MNOK", "kr", "m²") that does not start with a
// separator, or an abbreviation with a second token ("mill. kr"; the second
// token is only accepted after an abbreviation, see `parseChartNumber`). Digits
// never belong to it: "5 av 8", "12/15" and "2 aktive kunder" are text.
const UNIT_SUFFIX = '[^\\d\\s.,][^\\d\\s]{0,11}(?:\\s[^\\d\\s]{1,6})?'
const NUMBER_WITH_UNIT = new RegExp(`^([-+]?\\d[\\d\\s.,]*?)\\s*(${UNIT_SUFFIX})?$`)
const EU_THOUSANDS = /^-?\d{1,3}(?:\.\d{3})+(?:,\d+)?$/
const US_THOUSANDS = /^-?\d{1,3}(?:,\d{3}){2,}(?:\.\d+)?$|^-?\d{1,3},\d{3}\.\d+$/
const TRAILING_CITATION = /\s*\[\d+(?:\s*,\s*\d+)*\]\s*$/

/** A cell as plain text: markdown syntax gone, a trailing `[1]` source marker gone, whitespace folded. */
export function cleanChartCell(raw: string): string {
  return stripInlineMarkdown(raw)
    .replace(TRAILING_CITATION, '')
    .replace(/[\u00a0\u202f]/g, ' ')
    .replace(/\s+/g, ' ')
    .trim()
}

function readNumberCore(core: string): number | null {
  const fromExcel = detectExcelNumber(core)
  if (fromExcel !== null) return fromExcel
  let normalized: string | null = null
  if (EU_THOUSANDS.test(core)) normalized = core.replace(/\./g, '').replace(',', '.')
  else if (US_THOUSANDS.test(core)) normalized = core.replace(/,/g, '')
  if (normalized === null) return null
  const value = Number(normalized)
  return Number.isFinite(value) ? value : null
}

/**
 * A table cell as a number plus its unit, or null when it is not a number:
 * `12 %`, `3,2 MNOK`, `kr 1 234`, `-5`, `1 234,5`, `1.234,5`. Dates, codes
 * (`FF-1042`, `Q1`), ranges and phrases stay text.
 */
export function parseChartNumber(raw: string): { value: number; unit: string } | null {
  let text = cleanChartCell(raw)
  if (!text) return null
  // A real minus sign, or a dash typed as one, directly before a digit.
  text = text.replace(/[\u2212\u2012\u2013](?=\d)/g, '-')

  let unit = ''
  const prefix = CURRENCY_PREFIX.exec(text)
  if (prefix) {
    unit = (prefix[1] ?? '').replace(/\.$/, '')
    text = text.slice(prefix[0].length)
  }

  const match = NUMBER_WITH_UNIT.exec(text)
  if (!match) return null
  const core = (match[1] ?? '').replace(/^\+/, '').trim()
  const suffix = match[2] ?? ''
  if (suffix && unit) return null
  // A second token belongs to the unit only after an abbreviation ("mill. kr"),
  // never in a phrase ("2 aktive kunder").
  if (/\s/.test(suffix) && !/\.\s/.test(suffix)) return null
  const value = readNumberCore(core)
  if (value === null) return null
  return { value, unit: unit || suffix }
}

/** The text used for any value in a tooltip, a label or the data table. */
export function formatChartValue(value: number, unit: string, locale = 'nb-NO'): string {
  let formatter = FORMATTERS.get(locale)
  if (!formatter) {
    try {
      formatter = new Intl.NumberFormat(locale, { maximumFractionDigits: 2 })
    } catch {
      formatter = new Intl.NumberFormat('nb-NO', { maximumFractionDigits: 2 })
    }
    FORMATTERS.set(locale, formatter)
  }
  const text = formatter.format(value)
  return unit ? `${text}\u00a0${unit}` : text
}

const FORMATTERS = new Map<string, Intl.NumberFormat>()

// ── Which type a dataset suggests ───────────────────────────────────────────

const MONTH_NAMES =
  'jan(?:uar|uary)?|feb(?:ruar|ruary)?|mar(?:s|ch)?|apr(?:il)?|mai|may|jun(?:i|e)?|jul(?:i|y)?|aug(?:ust)?|sep(?:t(?:ember)?)?|okt(?:ober)?|oct(?:ober)?|nov(?:ember)?|des(?:ember)?|dec(?:ember)?'
const WEEKDAY_NAMES =
  'man(?:dag)?|tir(?:sdag)?|ons(?:dag)?|tor(?:sdag)?|fre(?:dag)?|lør(?:dag)?|søn(?:dag)?|mon(?:day)?|tue(?:sday)?|wed(?:nesday)?|thu(?:rsday)?|fri(?:day)?|sat(?:urday)?|sun(?:day)?'

const DATE_LIKE: readonly RegExp[] = [
  /^(?:19|20|21)\d{2}$/,
  /^\d{4}[-/.]\d{1,2}(?:[-/.]\d{1,2})?$/,
  /^\d{1,2}[./-]\d{1,2}(?:[./-]\d{2,4})?$/,
  new RegExp(`^(?:\\d{1,2}\\.?\\s*)?(?:${MONTH_NAMES})\\.?(?:\\s+\\d{2,4})?$`),
  /^[qk][1-4](?:\s*[-/']?\s*\d{2,4})?$/,
  /^\d{4}\s*[-/]?\s*[qk][1-4]$/,
  /^[1-4]\.?\s*(?:kvartal|quarter)(?:\s+\d{4})?$/,
  /^(?:kvartal|quarter)\s*[1-4](?:\s+\d{4})?$/,
  /^(?:uke|week|w|u)\s*\d{1,2}(?:\s+\d{4})?$/,
  new RegExp(`^(?:${WEEKDAY_NAMES})\\.?$`),
]

function isDateLike(label: string): boolean {
  const text = label.trim().toLowerCase()
  return text !== '' && DATE_LIKE.some((pattern) => pattern.test(text))
}

/** A line for categories that are points in time, a bar chart for everything else. */
export function suggestChartType(categories: readonly string[]): 'line' | 'bar' {
  return categories.length >= 2 && categories.every(isDateLike) ? 'line' : 'bar'
}

function pieEligible(spec: ChartSpec): boolean {
  const only = spec.series.length === 1 ? spec.series[0] : undefined
  if (!only || spec.categories.length > CHART_MAX_PIE_SLICES) return false
  return only.values.every((value) => value >= 0) && only.values.reduce((sum, value) => sum + value, 0) > 0
}

/**
 * The types the chart's own switch offers for this data: bar always, line when
 * there is more than one point, pie only where a pie is honest (one series, up
 * to eight non-negative slices). `area` is offered only to a chart that is one.
 */
export function availableChartTypes(spec: ChartSpec): ChartType[] {
  const types: ChartType[] = ['bar']
  if (spec.categories.length >= 2) types.push('line')
  if (spec.type === 'area') types.push('area')
  if (pieEligible(spec)) types.push('pie')
  if (!types.includes(spec.type)) types.push(spec.type)
  return types
}

export function withChartType(spec: ChartSpec, type: ChartType): ChartSpec {
  return { ...spec, type, stacked: spec.stacked && (type === 'bar' || type === 'area') }
}

// ── A markdown table as a chart ─────────────────────────────────────────────

const TOTAL_LABEL = /^(?:sum|total|totalt|totalsum|i alt|samlet|grand total|overall)$/i
const BLANK_OR_MISSING = /^(?:[-\u2013\u2014]|n\/?a|nan|null)?$/i
const YEAR = /^(?:19|20|21)\d{2}$/
const HEADER_UNIT = /[([]([^()[\]]{1,12})[)\]]\s*$/

type ColumnKind = 'empty' | 'text' | 'year' | 'numeric'

type NumericColumn = {
  index: number
  name: string
  values: number[]
  unit: string
}

function headerWithoutUnit(header: string): string {
  return header.replace(/\s*[([][^()[\]]*[)\]]\s*$/, '').trim()
}

/**
 * Reads a markdown table as a chart: the first text column labels the
 * categories (a column of years does too) and every complete numeric column is
 * a series. Returns `null` when no honest chart can be made, so the toolbar
 * simply shows no chart button.
 *
 * What this will not do, on purpose:
 *  - A column with a gap or a text cell is left out (a missing number is never
 *    drawn as zero); a short row counts as a gap.
 *  - A trailing "Totalt" row and a "Totalt" column are left out when anything
 *    else can be charted, since a total would dwarf the parts it sums.
 *  - Two different units never share an axis: a percent column is not plotted
 *    beside a currency column. The larger group wins.
 *  - More than 24 rows or more than 6 series is `null`, not a quiet cut.
 */
export function chartFromTable(
  header: readonly string[],
  rows: readonly (readonly string[])[],
): ChartSpec | null {
  const columnCount = header.length
  if (columnCount < 2) return null

  let body = rows
    .map((row) => Array.from({ length: columnCount }, (_, index) => cleanChartCell(row[index] ?? '')))
    .filter((row) => row.some((cell) => cell !== ''))
  if (body.length < 2) return null

  const classify = (column: number): { kind: ColumnKind; complete: boolean } => {
    const present = body.filter((row) => !BLANK_OR_MISSING.test(row[column] ?? ''))
    if (present.length === 0) return { kind: 'empty', complete: false }
    const allNumeric = present.every((row) => parseChartNumber(row[column] ?? '') !== null)
    if (!allNumeric) return { kind: 'text', complete: present.length === body.length }
    const years = body.every((row) => YEAR.test(row[column] ?? ''))
    return { kind: years ? 'year' : 'numeric', complete: present.length === body.length }
  }

  let categoryColumn = -1
  for (let column = 0; column < columnCount; column += 1) {
    const { kind, complete } = classify(column)
    if ((kind === 'text' && complete) || kind === 'year') {
      categoryColumn = column
      break
    }
  }
  if (categoryColumn === -1) {
    // No labels anywhere: a table of plain numbers (x, y, z) uses its first
    // complete numeric column as the labels, as long as something is left to plot.
    const numeric = Array.from({ length: columnCount }, (_, column) => column).filter((column) => {
      const { kind, complete } = classify(column)
      return kind === 'numeric' && complete
    })
    if (numeric.length < 2) return null
    categoryColumn = numeric[0] ?? -1
  }
  if (categoryColumn === -1) return null

  const lastLabel = body.at(-1)?.[categoryColumn] ?? ''
  if (body.length >= 3 && TOTAL_LABEL.test(lastLabel)) body = body.slice(0, -1)
  if (body.length < 2 || body.length > CHART_MAX_CATEGORIES) return null

  const candidates: NumericColumn[] = []
  for (let column = categoryColumn + 1; column < columnCount; column += 1) {
    const { kind, complete } = classify(column)
    if (kind !== 'numeric' || !complete) continue
    const parsed = body.map((row) => parseChartNumber(row[column] ?? ''))
    const cellUnits = new Set(parsed.map((entry) => entry?.unit ?? '').filter((unit) => unit !== ''))
    if (cellUnits.size > 1) continue
    const name = cleanChartCell(header[column] ?? '') || `Serie ${candidates.length + 1}`
    const headerUnit = HEADER_UNIT.exec(name)?.[1]?.trim() ?? ''
    candidates.push({
      index: column,
      name,
      values: parsed.map((entry) => entry?.value ?? 0),
      unit: [...cellUnits][0] ?? headerUnit,
    })
  }
  if (candidates.length === 0) return null

  const measures = candidates.filter((column) => !TOTAL_LABEL.test(headerWithoutUnit(column.name)))
  const kept = measures.length > 0 ? measures : candidates

  // One scale: group the columns by unit. A unitless column joins the largest
  // currency-like group (it is a plain amount in the same table); percent
  // stays on its own.
  const groups = new Map<string, NumericColumn[]>()
  for (const column of kept) {
    const list = groups.get(column.unit) ?? []
    list.push(column)
    groups.set(column.unit, list)
  }
  const pickLargest = (entries: [string, NumericColumn[]][]) =>
    entries.reduce<[string, NumericColumn[]] | null>(
      (best, entry) => (best === null || entry[1].length > best[1].length ? entry : best),
      null,
    )
  const amountGroups = [...groups.entries()].filter(([unit]) => unit !== '' && unit !== '%')
  const unitless = groups.get('')
  if (unitless && amountGroups.length > 0) {
    const target = pickLargest(amountGroups)
    if (target) {
      groups.delete('')
      target[1].push(...unitless)
      target[1].sort((left, right) => left.index - right.index)
    }
  }
  const chosen = pickLargest([...groups.entries()])
  if (!chosen) return null
  const [unit, series] = chosen
  if (series.length === 0 || series.length > CHART_MAX_SERIES) return null

  const categories = body.map((row) => row[categoryColumn] ?? '')
  return {
    type: suggestChartType(categories),
    title: '',
    categories,
    series: series.map(({ name, values }) => ({ name, values })),
    unit,
    stacked: false,
  }
}
