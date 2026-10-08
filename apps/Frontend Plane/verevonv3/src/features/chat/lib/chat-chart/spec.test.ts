import { describe, expect, it } from 'vitest'
import {
  CHART_MAX_CATEGORIES,
  CHART_MAX_PIE_SLICES,
  CHART_MAX_SERIES,
  availableChartTypes,
  chartFromTable,
  formatChartValue,
  parseChartFence,
  parseChartNumber,
  suggestChartType,
  withChartType,
  type ChartSpec,
} from './spec'

function fence(value: unknown): string {
  return JSON.stringify(value)
}

const QUARTERS = {
  type: 'bar',
  title: 'Omsetning per kvartal',
  x: ['K1', 'K2', 'K3', 'K4'],
  series: [
    { name: '2025', values: [10, 12, 9, 14] },
    { name: '2026', values: [11, 13, 12, 16] },
  ],
  unit: 'MNOK',
}

describe('parseChartFence', () => {
  it('parses a valid bar chart with `x` as the category list', () => {
    const result = parseChartFence(fence(QUARTERS))
    expect(result.ok).toBe(true)
    if (!result.ok) return
    expect(result.spec).toEqual({
      type: 'bar',
      title: 'Omsetning per kvartal',
      categories: ['K1', 'K2', 'K3', 'K4'],
      series: [
        { name: '2025', values: [10, 12, 9, 14] },
        { name: '2026', values: [11, 13, 12, 16] },
      ],
      unit: 'MNOK',
      stacked: false,
    })
  })

  it('accepts `categories` as an alias of `x`, a type in any case, and stacked on a bar', () => {
    const result = parseChartFence(
      fence({ type: ' Bar ', categories: ['a', 'b'], series: [{ name: 's', values: [1, 2] }], stacked: true }),
    )
    expect(result.ok && result.spec.categories).toEqual(['a', 'b'])
    expect(result.ok && result.spec.type).toBe('bar')
    expect(result.ok && result.spec.stacked).toBe(true)
  })

  it('reads a `column` type as a bar, and ignores `stacked` on a line or a pie', () => {
    const column = parseChartFence(fence({ type: 'column', x: ['a'], series: [{ name: 's', values: [1] }] }))
    expect(column.ok && column.spec.type).toBe('bar')
    const line = parseChartFence(fence({ type: 'line', x: ['a', 'b'], series: [{ name: 's', values: [1, 2] }], stacked: true }))
    expect(line.ok && line.spec.stacked).toBe(false)
  })

  it('passes numeric strings through the Norwegian number reader', () => {
    const result = parseChartFence(
      fence({ type: 'line', x: ['jan', 'feb', 'mar'], series: [{ name: 's', values: ['1 234,5', '12,5', 7] }] }),
    )
    expect(result.ok && result.spec.series[0]?.values).toEqual([1234.5, 12.5, 7])
  })

  it('gives an unnamed series and a missing title a harmless default', () => {
    const result = parseChartFence(fence({ type: 'bar', x: ['a'], series: [{ values: [3] }] }))
    expect(result.ok && result.spec.series[0]?.name).toBe('Serie 1')
    expect(result.ok && result.spec.title).toBe('')
    expect(result.ok && result.spec.unit).toBe('')
  })

  it.each([
    ['empty source', '', 'empty'],
    ['whitespace only', '   \n ', 'empty'],
    ['not JSON', 'type: bar', 'not-json'],
    ['a JSON array', '[1,2,3]', 'not-object'],
    ['a JSON string', '"bar"', 'not-object'],
    ['an unknown type', fence({ type: 'radar', x: ['a'], series: [{ name: 's', values: [1] }] }), 'bad-type'],
    ['a missing type', fence({ x: ['a'], series: [{ name: 's', values: [1] }] }), 'bad-type'],
    ['no categories', fence({ type: 'bar', series: [{ name: 's', values: [1] }] }), 'no-categories'],
    ['an empty category list', fence({ type: 'bar', x: [], series: [{ name: 's', values: [] }] }), 'no-categories'],
    ['categories that are not text or numbers', fence({ type: 'bar', x: [{}], series: [{ name: 's', values: [1] }] }), 'no-categories'],
    ['no series', fence({ type: 'bar', x: ['a'] }), 'no-series'],
    ['an empty series list', fence({ type: 'bar', x: ['a'], series: [] }), 'no-series'],
    ['a non-numeric value', fence({ type: 'bar', x: ['a', 'b'], series: [{ name: 's', values: [1, 'mye'] }] }), 'bad-values'],
    ['a null value', fence({ type: 'bar', x: ['a', 'b'], series: [{ name: 's', values: [1, null] }] }), 'bad-values'],
    ['a non-finite number', '{"type":"bar","x":["a"],"series":[{"name":"s","values":[1e999]}]}', 'bad-values'],
    ['a series shorter than the categories', fence({ type: 'bar', x: ['a', 'b'], series: [{ name: 's', values: [1] }] }), 'length-mismatch'],
    ['a pie with two series', fence({ type: 'pie', x: ['a', 'b'], series: [{ name: 's', values: [1, 2] }, { name: 't', values: [3, 4] }] }), 'pie-needs-one-series'],
    ['a pie with a negative slice', fence({ type: 'pie', x: ['a', 'b'], series: [{ name: 's', values: [1, -2] }] }), 'pie-values'],
    ['a pie that sums to zero', fence({ type: 'pie', x: ['a', 'b'], series: [{ name: 's', values: [0, 0] }] }), 'pie-values'],
  ])('rejects %s without throwing', (_label, source, reason) => {
    const result = parseChartFence(source)
    expect(result).toEqual({ ok: false, reason })
  })

  it('rejects an oversize chart: too many categories, series or slices, or a huge source', () => {
    const many = Array.from({ length: CHART_MAX_CATEGORIES + 1 }, (_, index) => `c${index}`)
    expect(
      parseChartFence(fence({ type: 'bar', x: many, series: [{ name: 's', values: many.map(() => 1) }] })),
    ).toEqual({ ok: false, reason: 'too-many-categories' })

    const atLimit = many.slice(0, CHART_MAX_CATEGORIES)
    expect(
      parseChartFence(fence({ type: 'bar', x: atLimit, series: [{ name: 's', values: atLimit.map(() => 1) }] })).ok,
    ).toBe(true)

    const series = Array.from({ length: CHART_MAX_SERIES + 1 }, (_, index) => ({ name: `s${index}`, values: [1, 2] }))
    expect(parseChartFence(fence({ type: 'bar', x: ['a', 'b'], series }))).toEqual({ ok: false, reason: 'too-many-series' })
    expect(parseChartFence(fence({ type: 'bar', x: ['a', 'b'], series: series.slice(0, CHART_MAX_SERIES) })).ok).toBe(true)

    const slices = Array.from({ length: CHART_MAX_PIE_SLICES + 1 }, (_, index) => `p${index}`)
    expect(
      parseChartFence(fence({ type: 'pie', x: slices, series: [{ name: 's', values: slices.map(() => 1) }] })),
    ).toEqual({ ok: false, reason: 'too-many-slices' })
    expect(
      parseChartFence(fence({ type: 'pie', x: slices.slice(0, CHART_MAX_PIE_SLICES), series: [{ name: 's', values: slices.slice(0, CHART_MAX_PIE_SLICES).map(() => 1) }] })).ok,
    ).toBe(true)

    expect(parseChartFence(`{"title":"${'x'.repeat(30_000)}"}`)).toEqual({ ok: false, reason: 'too-large' })
  })

  it('treats a still-streaming (cut-off) fence as not parseable instead of throwing', () => {
    const full = fence(QUARTERS)
    for (const cut of [1, 20, full.length - 1]) {
      expect(parseChartFence(full.slice(0, cut))).toEqual({ ok: false, reason: 'not-json' })
    }
  })

  it('never treats prototype keys or hostile text as anything but data', () => {
    const result = parseChartFence('{"type":"bar","x":["<img src=x onerror=alert(1)>"],"series":[{"name":"__proto__","values":[1]}],"__proto__":{"polluted":1}}')
    expect(result.ok).toBe(true)
    expect(({} as Record<string, unknown>).polluted).toBeUndefined()
    expect(result.ok && result.spec.categories[0]).toBe('<img src=x onerror=alert(1)>')
  })

  it('trims and caps the title and the unit', () => {
    const result = parseChartFence(
      fence({ type: 'bar', title: `  ${'T'.repeat(500)}  `, unit: 'U'.repeat(100), x: ['a'], series: [{ name: 's', values: [1] }] }),
    )
    expect(result.ok && result.spec.title.length).toBeLessThanOrEqual(140)
    expect(result.ok && result.spec.unit.length).toBeLessThanOrEqual(16)
  })
})

describe('parseChartNumber', () => {
  it.each([
    ['12', 12, ''],
    ['-12', -12, ''],
    ['\u221212', -12, ''],
    ['1 234,5', 1234.5, ''],
    ['1\u00a0234,5', 1234.5, ''],
    ['12,5', 12.5, ''],
    ['12.5', 12.5, ''],
    ['12 %', 12, '%'],
    ['12%', 12, '%'],
    ['-3,2 %', -3.2, '%'],
    ['+3,2 %', 3.2, '%'],
    ['3,2 MNOK', 3.2, 'MNOK'],
    ['1 234 kr', 1234, 'kr'],
    ['kr 1 234', 1234, 'kr'],
    ['NOK 1 234,50', 1234.5, 'NOK'],
    ['$12.5', 12.5, '$'],
    ['3,2 mill. kr', 3.2, 'mill. kr'],
    ['**1 234**', 1234, ''],
    ['1 234[1]', 1234, ''],
    ['1.234,5', 1234.5, ''],
    ['1,234.5', 1234.5, ''],
    ['1,234,567', 1234567, ''],
    ['2025', 2025, ''],
  ])('reads %j as %d with unit %j', (text, value, unit) => {
    expect(parseChartNumber(text)).toEqual({ value, unit })
  })

  it.each([
    [''],
    ['  '],
    ['-'],
    ['\u2013'],
    ['n/a'],
    ['mye'],
    ['Q1'],
    ['Uke 37'],
    ['FF-1042'],
    ['2025-01'],
    ['01.02.2025'],
    ['12/15'],
    ['5 av 8'],
    ['2 aktive kunder'],
    ['1e5'],
    ['Infinity'],
    ['NaN'],
  ])('does not read %j as a number', (text) => {
    expect(parseChartNumber(text)).toBeNull()
  })
})

describe('suggestChartType', () => {
  it.each([
    [['jan', 'feb', 'mar'], 'line'],
    [['Januar 2025', 'Februar 2025'], 'line'],
    [['2023', '2024', '2025'], 'line'],
    [['2025-01', '2025-02', '2025-03'], 'line'],
    [['01.02.2025', '02.02.2025'], 'line'],
    [['K1 2025', 'K2 2025', 'K3 2025'], 'line'],
    [['Q1', 'Q2', 'Q3', 'Q4'], 'line'],
    [['1. kvartal 2025', '2. kvartal 2025'], 'line'],
    [['Uke 36', 'Uke 37'], 'line'],
    [['mandag', 'tirsdag', 'onsdag'], 'line'],
    [['Oslo', 'Bergen', 'Trondheim'], 'bar'],
    [['Produkt A', 'Produkt B'], 'bar'],
    [['Mars', 'Venus'], 'bar'],
    [[], 'bar'],
  ] as const)('%j -> %s', (categories, expected) => {
    expect(suggestChartType(categories)).toBe(expected)
  })
})

describe('chartFromTable', () => {
  it('uses the first text column as categories and every numeric column as a series', () => {
    const spec = chartFromTable(
      ['Kvartal', 'Omsetning 2025', 'Omsetning 2026'],
      [
        ['K1', '1 200', '1 400'],
        ['K2', '1 350,5', '1 500'],
        ['K3', '1 100', '1 650'],
      ],
    )
    expect(spec).not.toBeNull()
    expect(spec?.categories).toEqual(['K1', 'K2', 'K3'])
    expect(spec?.series).toEqual([
      { name: 'Omsetning 2025', values: [1200, 1350.5, 1100] },
      { name: 'Omsetning 2026', values: [1400, 1500, 1650] },
    ])
    expect(spec?.type).toBe('line')
    expect(spec?.stacked).toBe(false)
  })

  it('picks a bar chart for plain categories', () => {
    const spec = chartFromTable(['Region', 'Salg'], [['Nord', '10'], ['Sør', '12']])
    expect(spec?.type).toBe('bar')
  })

  it('reads percentages, currency and unit suffixes, and carries the unit', () => {
    const percent = chartFromTable(['Segment', 'Andel'], [['A', '12 %'], ['B', '30,5 %'], ['C', '57,5 %']])
    expect(percent?.series[0]?.values).toEqual([12, 30.5, 57.5])
    expect(percent?.unit).toBe('%')

    const money = chartFromTable(['Kunde', 'Beløp'], [['A', '3,2 MNOK'], ['B', '1,1 MNOK']])
    expect(money?.series[0]?.values).toEqual([3.2, 1.1])
    expect(money?.unit).toBe('MNOK')

    const header = chartFromTable(['Kunde', 'Beløp (kr)'], [['A', '1 000'], ['B', '2 500']])
    expect(header?.unit).toBe('kr')
  })

  it('strips inline markdown from labels, headers and numbers', () => {
    const spec = chartFromTable(['**Produkt**', '*Salg*'], [['**A**', '**10**'], ['[B](https://example.com)', '20']])
    expect(spec?.categories).toEqual(['A', 'B'])
    expect(spec?.series[0]?.name).toBe('Salg')
    expect(spec?.series[0]?.values).toEqual([10, 20])
  })

  it('treats a year-like first column as categories, not a series', () => {
    const spec = chartFromTable(['År', 'Omsetning'], [['2023', '10'], ['2024', '12'], ['2025', '15']])
    expect(spec?.categories).toEqual(['2023', '2024', '2025'])
    expect(spec?.series.map((series) => series.name)).toEqual(['Omsetning'])
    expect(spec?.type).toBe('line')
  })

  it('drops an index column that sits before the category column', () => {
    const spec = chartFromTable(['Nr', 'Navn', 'Beløp'], [['1', 'A', '5'], ['2', 'B', '7']])
    expect(spec?.categories).toEqual(['A', 'B'])
    expect(spec?.series.map((series) => series.name)).toEqual(['Beløp'])
  })

  it('drops a trailing total row and a total column when other series exist', () => {
    const spec = chartFromTable(
      ['Produkt', 'Q1', 'Q2', 'Totalt'],
      [['A', '1', '2', '3'], ['B', '3', '4', '7'], ['Totalt', '4', '6', '10']],
    )
    expect(spec?.categories).toEqual(['A', 'B'])
    expect(spec?.series.map((series) => series.name)).toEqual(['Q1', 'Q2'])
  })

  it('keeps a lone total column, since it is the only measure', () => {
    const spec = chartFromTable(['Produkt', 'Sum'], [['A', '3'], ['B', '7']])
    expect(spec?.series.map((series) => series.name)).toEqual(['Sum'])
  })

  it('keeps one scale: a percent column is not plotted beside a currency column', () => {
    const spec = chartFromTable(
      ['Produkt', 'Omsetning (MNOK)', 'Budsjett', 'Margin'],
      [['A', '12', '10', '31 %'], ['B', '15', '14', '28 %']],
    )
    expect(spec?.series.map((series) => series.name)).toEqual(['Omsetning (MNOK)', 'Budsjett'])
    expect(spec?.unit).toBe('MNOK')
  })

  it('leaves out a column with a gap or a text cell, and keeps the complete ones', () => {
    const spec = chartFromTable(
      ['Produkt', 'Komplett', 'Hull', 'Blandet'],
      [['A', '1', '5', '1'], ['B', '2', '-', '2'], ['C', '3', '7', 'mye']],
    )
    expect(spec?.series.map((series) => series.name)).toEqual(['Komplett'])
  })

  it('handles ragged rows: short rows leave their column out, extra cells are ignored', () => {
    const spec = chartFromTable(
      ['Produkt', 'A', 'B'],
      [['x', '1', '2'], ['y', '3'], ['z', '5', '6', 'overskudd']],
    )
    expect(spec?.series.map((series) => series.name)).toEqual(['A'])
    expect(spec?.categories).toEqual(['x', 'y', 'z'])
  })

  it('uses the first numeric column as categories when the table has no text column', () => {
    const spec = chartFromTable(['x', 'y', 'z'], [['1', '10', '100'], ['2', '20', '200'], ['3', '30', '300']])
    expect(spec?.categories).toEqual(['1', '2', '3'])
    expect(spec?.series.map((series) => series.name)).toEqual(['y', 'z'])
  })

  it.each([
    ['no numeric column', ['Navn', 'Rolle'], [['A', 'Leder'], ['B', 'Utvikler']]],
    ['an empty body', ['Navn', 'Salg'], []],
    ['a single data row', ['Navn', 'Salg'], [['A', '1']]],
    ['a single column', ['Salg'], [['1'], ['2']]],
    ['only numeric columns that all have gaps', ['Navn', 'Salg'], [['A', '1'], ['B', '']]],
    ['only a date-like text column', ['Dato', 'Kommentar'], [['01.02.2025', 'ok'], ['02.02.2025', 'ok']]],
  ])('returns null for %s', (_label, header, rows) => {
    expect(chartFromTable(header, rows)).toBeNull()
  })

  it('returns null past the category and series limits instead of silently cutting data', () => {
    const rows = Array.from({ length: CHART_MAX_CATEGORIES + 1 }, (_, index) => [`r${index}`, String(index)])
    expect(chartFromTable(['Rad', 'Verdi'], rows)).toBeNull()
    expect(chartFromTable(['Rad', 'Verdi'], rows.slice(0, CHART_MAX_CATEGORIES))).not.toBeNull()

    const header = ['Rad', ...Array.from({ length: CHART_MAX_SERIES + 1 }, (_, index) => `S${index}`)]
    const wide = [['a', ...header.slice(1).map(() => '1')], ['b', ...header.slice(1).map(() => '2')]]
    expect(chartFromTable(header, wide)).toBeNull()
    expect(chartFromTable(header.slice(0, CHART_MAX_SERIES + 1), wide.map((row) => row.slice(0, CHART_MAX_SERIES + 1)))).not.toBeNull()
  })

  it('skips fully blank rows', () => {
    const spec = chartFromTable(['A', 'B'], [['x', '1'], ['', ''], ['y', '2']])
    expect(spec?.categories).toEqual(['x', 'y'])
  })
})

describe('availableChartTypes and withChartType', () => {
  const base: ChartSpec = {
    type: 'bar',
    title: '',
    categories: ['a', 'b', 'c'],
    series: [{ name: 's', values: [1, 2, 3] }],
    unit: '',
    stacked: false,
  }

  it('offers bar, line and pie for one non-negative series of up to eight slices', () => {
    expect(availableChartTypes(base)).toEqual(['bar', 'line', 'pie'])
  })

  it('offers no pie for several series, a negative value, or more than eight slices', () => {
    expect(availableChartTypes({ ...base, series: [...base.series, { name: 't', values: [1, 2, 3] }] })).toEqual(['bar', 'line'])
    expect(availableChartTypes({ ...base, series: [{ name: 's', values: [1, -2, 3] }] })).toEqual(['bar', 'line'])
    const nine = Array.from({ length: 9 }, (_, index) => `c${index}`)
    expect(availableChartTypes({ ...base, categories: nine, series: [{ name: 's', values: nine.map(() => 1) }] })).toEqual(['bar', 'line'])
  })

  it('offers no line for a single category, and keeps the spec own area type visible', () => {
    expect(availableChartTypes({ ...base, categories: ['a'], series: [{ name: 's', values: [1] }] })).toEqual(['bar', 'pie'])
    expect(availableChartTypes({ ...base, type: 'area' })).toEqual(['bar', 'line', 'area', 'pie'])
  })

  it('switches the type and drops stacking where it has no meaning', () => {
    expect(withChartType({ ...base, stacked: true }, 'line').stacked).toBe(false)
    expect(withChartType({ ...base, stacked: true }, 'bar').stacked).toBe(true)
    expect(withChartType(base, 'pie').type).toBe('pie')
  })
})

describe('formatChartValue', () => {
  it('formats with Norwegian grouping and keeps the unit', () => {
    expect(formatChartValue(1234.5, 'MNOK', 'nb-NO').replace(/\s/g, ' ')).toBe('1 234,5 MNOK')
    expect(formatChartValue(12, '%', 'nb-NO').replace(/\s/g, ' ')).toBe('12 %')
    expect(formatChartValue(7, '', 'nb-NO')).toBe('7')
  })
})
