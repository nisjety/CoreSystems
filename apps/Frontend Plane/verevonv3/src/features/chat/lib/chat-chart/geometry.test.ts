import { describe, expect, it } from 'vitest'
import {
  BAR_MAX_THICKNESS,
  BAR_GAP,
  estimateTextWidth,
  labelsCollide,
  layoutBars,
  layoutLines,
  layoutPie,
  markerShape,
  niceTicks,
  truncateLabel,
  truncateMiddle,
} from './geometry'
import type { ChartSpec } from './spec'

function spec(overrides: Partial<ChartSpec> = {}): ChartSpec {
  return {
    type: 'bar',
    title: '',
    categories: ['K1', 'K2', 'K3', 'K4'],
    series: [
      { name: '2025', values: [10, 20, 30, 40] },
      { name: '2026', values: [15, 25, 35, 45] },
    ],
    unit: '',
    stacked: false,
    ...overrides,
  }
}

const PLOT = { width: 600, height: 240 }

function noNaN(text: string): boolean {
  return !/NaN|undefined|Infinity/.test(text)
}

describe('niceTicks', () => {
  it('covers 0..100 in steps of 20', () => {
    const scale = niceTicks(0, 100)
    expect(scale.ticks).toEqual([0, 20, 40, 60, 80, 100])
    expect(scale.min).toBe(0)
    expect(scale.max).toBe(100)
    expect(scale.step).toBe(20)
  })

  it('rounds the top up to a clean number', () => {
    const scale = niceTicks(0, 97)
    expect(scale.max).toBe(100)
    expect(scale.ticks.at(-1)).toBe(100)
  })

  it('handles all values equal by anchoring at zero', () => {
    const scale = niceTicks(5, 5)
    expect(scale.min).toBe(0)
    expect(scale.max).toBeGreaterThanOrEqual(5)
    expect(scale.ticks.length).toBeGreaterThanOrEqual(2)
    expect(scale.ticks[0]).toBe(0)
  })

  it('handles all zero', () => {
    const scale = niceTicks(0, 0)
    expect(scale.min).toBe(0)
    expect(scale.max).toBe(1)
    expect(scale.ticks[0]).toBe(0)
    expect(scale.ticks.at(-1)).toBe(1)
  })

  it('handles all values equal and negative', () => {
    const scale = niceTicks(-5, -5)
    expect(scale.max).toBe(0)
    expect(scale.min).toBeLessThanOrEqual(-5)
  })

  it('handles negatives and keeps zero as a tick', () => {
    const scale = niceTicks(-30, 70)
    expect(scale.ticks).toContain(0)
    expect(scale.ticks[0]).toBeLessThanOrEqual(-30)
    expect(scale.ticks.at(-1)).toBeGreaterThanOrEqual(70)
  })

  it('handles a single data point', () => {
    const scale = niceTicks(7, 7, 5, false)
    expect(scale.min).toBeLessThan(7)
    expect(scale.max).toBeGreaterThan(7)
    expect(scale.ticks.length).toBeGreaterThanOrEqual(2)
  })

  it('gives fractional scales without floating point noise', () => {
    const scale = niceTicks(0, 0.35)
    expect(scale.ticks).toEqual([0, 0.1, 0.2, 0.3, 0.4])
    expect(scale.decimals).toBe(1)
  })

  it('does not include zero when asked not to, and still brackets the data', () => {
    const scale = niceTicks(98, 102, 5, false)
    expect(scale.min).toBeGreaterThan(90)
    expect(scale.min).toBeLessThanOrEqual(98)
    expect(scale.max).toBeGreaterThanOrEqual(102)
    expect(scale.ticks).not.toContain(0)
  })

  it('falls back to 0..1 on non-finite input', () => {
    for (const [low, high] of [[Number.NaN, 5], [0, Number.POSITIVE_INFINITY], [Number.NEGATIVE_INFINITY, 1]] as const) {
      const scale = niceTicks(low, high)
      expect(scale.min).toBe(0)
      expect(scale.max).toBe(1)
    }
  })

  it('gives a finite axis, and never throws, for values at the edge of a double', () => {
    // 1.7e308 / 5 overflowed to an Infinity top tick and "Invalid array length" inside the chart's render.
    const extremes = [
      [0, 1.7e308],
      [0, Number.MAX_VALUE],
      [-1e308, 1e308],
      [0, Number.MIN_VALUE],
      [-Number.MIN_VALUE, Number.MIN_VALUE],
      [Number.MIN_VALUE, 1e-300],
    ] as const
    for (const [low, high] of extremes) {
      const scale = niceTicks(low, high, 5)
      expect(Number.isFinite(scale.min), `${low}..${high}`).toBe(true)
      expect(Number.isFinite(scale.max), `${low}..${high}`).toBe(true)
      expect(scale.step, `${low}..${high}`).toBeGreaterThan(0)
      expect(scale.ticks.length, `${low}..${high}`).toBeGreaterThanOrEqual(2)
      expect(scale.ticks.every(Number.isFinite), `${low}..${high}`).toBe(true)
    }
    const huge = spec({ series: [{ name: 'a', values: [1.7976931348623157e308, 1] }], categories: ['x', 'y'] })
    expect(() => layoutBars(huge, { width: 400, height: 200 })).not.toThrow()
    expect(() => layoutLines(huge, { width: 400, height: 200 }, 'line')).not.toThrow()
  })

  it('swaps a reversed range instead of producing an empty axis', () => {
    expect(niceTicks(100, 0).ticks).toEqual(niceTicks(0, 100).ticks)
  })

  it('stays evenly spaced for large values', () => {
    const scale = niceTicks(0, 1.2e9)
    expect(scale.ticks.length).toBeGreaterThanOrEqual(3)
    expect(scale.ticks.length).toBeLessThanOrEqual(9)
    scale.ticks.forEach((tick, index) => {
      expect(tick).toBeCloseTo(scale.min + index * scale.step, 3)
    })
    expect(scale.ticks.at(-1)).toBeGreaterThanOrEqual(1.2e9)
  })

  it('offers between three and eight ticks for any sane range', () => {
    for (const [low, high] of [[0, 1], [0, 7], [0, 13], [0, 99], [0, 1234], [-8, 8], [0, 0.0042]] as const) {
      const { ticks } = niceTicks(low, high, 5)
      expect(ticks.length, `${low}..${high}`).toBeGreaterThanOrEqual(3)
      expect(ticks.length, `${low}..${high}`).toBeLessThanOrEqual(8)
    }
  })
})

describe('layoutBars (grouped)', () => {
  const layout = layoutBars(spec(), PLOT)

  it('lays out one bar per series per category, in series order', () => {
    expect(layout.bars).toHaveLength(8)
    expect(layout.bars.map((bar) => [bar.category, bar.series])).toEqual([
      [0, 0], [0, 1], [1, 0], [1, 1], [2, 0], [2, 1], [3, 0], [3, 1],
    ])
    expect(layout.bands).toHaveLength(4)
  })

  it('caps the bar thickness and keeps a surface gap between neighbours', () => {
    for (const bar of layout.bars) expect(bar.width).toBeLessThanOrEqual(BAR_MAX_THICKNESS)
    const first = layout.bars[0]
    const second = layout.bars[1]
    expect(second && first ? second.x - (first.x + first.width) : 0).toBeGreaterThanOrEqual(BAR_GAP)
  })

  it('keeps every bar inside its own band, centred', () => {
    for (const bar of layout.bars) {
      const band = layout.bands[bar.category]
      expect(band).toBeDefined()
      if (!band) continue
      expect(bar.x).toBeGreaterThanOrEqual(band.x)
      expect(bar.x + bar.width).toBeLessThanOrEqual(band.x + band.width + 0.0001)
    }
    const group = layout.bars.filter((bar) => bar.category === 0)
    const left = Math.min(...group.map((bar) => bar.x))
    const right = Math.max(...group.map((bar) => bar.x + bar.width))
    const band = layout.bands[0]
    expect((left + right) / 2).toBeCloseTo((band?.x ?? 0) + (band?.width ?? 0) / 2, 3)
  })

  it('grows from the baseline in proportion to the value', () => {
    for (const bar of layout.bars) expect(bar.y + bar.height).toBeCloseTo(layout.zeroY, 5)
    const ten = layout.bars.find((bar) => bar.value === 10)
    const forty = layout.bars.find((bar) => bar.value === 40)
    expect((forty?.height ?? 0) / (ten?.height ?? 1)).toBeCloseTo(4, 3)
  })

  it('anchors the scale at zero and keeps the data inside the plot', () => {
    expect(layout.scale.min).toBe(0)
    for (const bar of layout.bars) {
      expect(bar.y).toBeGreaterThanOrEqual(0)
      expect(bar.y + bar.height).toBeLessThanOrEqual(PLOT.height + 0.0001)
    }
  })

  it('draws every bar as a closed path with no invalid numbers', () => {
    for (const bar of layout.bars) {
      expect(bar.path).toMatch(/^M.*Z$/)
      expect(noNaN(bar.path)).toBe(true)
    }
  })

  it('puts the value label past the bar end', () => {
    for (const bar of layout.bars) expect(bar.labelY).toBeLessThan(bar.y)
  })
})

describe('layoutBars (edge cases)', () => {
  it('lets a negative bar hang below the baseline', () => {
    const layout = layoutBars(spec({ categories: ['a', 'b'], series: [{ name: 's', values: [10, -20] }] }), PLOT)
    const [positive, negative] = layout.bars
    expect(positive && positive.y + positive.height).toBeCloseTo(layout.zeroY, 5)
    expect(negative?.y).toBeCloseTo(layout.zeroY, 5)
    expect(negative?.height).toBeGreaterThan(0)
    expect(layout.scale.min).toBeLessThan(0)
    expect(negative && negative.labelY).toBeGreaterThan(negative ? negative.y + negative.height : 0)
  })

  it('gives zero and constant data a flat but valid layout', () => {
    const zeros = layoutBars(spec({ categories: ['a', 'b'], series: [{ name: 's', values: [0, 0] }] }), PLOT)
    expect(zeros.scale.max).toBe(1)
    for (const bar of zeros.bars) {
      expect(bar.height).toBe(0)
      expect(noNaN(bar.path)).toBe(true)
    }
    const same = layoutBars(spec({ categories: ['a', 'b'], series: [{ name: 's', values: [5, 5] }] }), PLOT)
    expect(same.bars[0]?.height).toBeCloseTo(same.bars[1]?.height ?? -1, 5)
    expect(same.bars[0]?.height).toBeGreaterThan(0)
  })

  it('draws a tiny non-zero value as at least one visible pixel', () => {
    const layout = layoutBars(spec({ categories: ['a', 'b'], series: [{ name: 's', values: [1, 100000] }] }), PLOT)
    expect(layout.bars[0]?.height).toBeGreaterThanOrEqual(1)
  })

  it('handles one category as a single full-width band', () => {
    const layout = layoutBars(spec({ categories: ['a'], series: [{ name: 's', values: [3] }] }), PLOT)
    expect(layout.bands).toHaveLength(1)
    expect(layout.bands[0]?.width).toBe(PLOT.width)
    expect(layout.bars).toHaveLength(1)
    expect(layout.bars[0]?.width).toBeLessThanOrEqual(BAR_MAX_THICKNESS)
  })

  it('narrows bars rather than overflowing when the bands are tight', () => {
    const categories = Array.from({ length: 24 }, (_, index) => `c${index}`)
    const series = Array.from({ length: 6 }, (_, index) => ({ name: `s${index}`, values: categories.map((_c, row) => row + index + 1) }))
    const layout = layoutBars(spec({ categories, series }), { width: 320, height: 200 })
    expect(layout.bars).toHaveLength(144)
    for (const bar of layout.bars) {
      const band = layout.bands[bar.category]
      expect(bar.width).toBeGreaterThan(0)
      expect((band ? bar.x + bar.width <= band.x + band.width + 0.0001 : false)).toBe(true)
    }
  })
})

describe('layoutBars (stacked)', () => {
  const stacked = layoutBars(
    spec({
      stacked: true,
      categories: ['a', 'b'],
      series: [
        { name: 'x', values: [10, 20] },
        { name: 'y', values: [30, 10] },
      ],
    }),
    PLOT,
  )

  it('puts the series of a category on top of each other, one bar wide', () => {
    const [first, second] = stacked.bars.filter((bar) => bar.category === 0)
    expect(first?.x).toBe(second?.x)
    expect(first?.width).toBe(second?.width)
    expect(second && first ? second.y + second.height <= first.y : false).toBe(true)
  })

  it('separates stacked segments with the surface gap', () => {
    const [first, second] = stacked.bars.filter((bar) => bar.category === 0)
    expect(first && second ? first.y - (second.y + second.height) : 0).toBeGreaterThanOrEqual(BAR_GAP - 0.0001)
  })

  it('scales to the total of the stack', () => {
    expect(stacked.scale.max).toBeGreaterThanOrEqual(40)
  })

  it('stacks negative values downward from the baseline, separate from the positives', () => {
    const mixed = layoutBars(
      spec({
        stacked: true,
        categories: ['a'],
        series: [
          { name: 'x', values: [10] },
          { name: 'y', values: [-5] },
          { name: 'z', values: [4] },
        ],
      }),
      PLOT,
    )
    const [up, down, upper] = mixed.bars
    expect(up && up.y + up.height).toBeCloseTo(mixed.zeroY, 5)
    expect(down?.y).toBeCloseTo(mixed.zeroY, 5)
    expect(upper && up ? upper.y + upper.height <= up.y : false).toBe(true)
  })
})

describe('layoutLines', () => {
  const layout = layoutLines(spec({ type: 'line' }), PLOT, 'line')

  it('puts one point per category at the band centres, in series order', () => {
    expect(layout.series).toHaveLength(2)
    const first = layout.series[0]
    expect(first?.points).toHaveLength(4)
    expect(first?.points.map((point) => point.x)).toEqual([75, 225, 375, 525])
  })

  it('draws a higher value higher on the plot', () => {
    const ys = layout.series[0]?.points.map((point) => point.y) ?? []
    expect([...ys].sort((a, b) => b - a)).toEqual(ys)
  })

  it('builds an open path for a line and a path closed on the baseline for an area', () => {
    const line = layout.series[0]
    expect(line?.linePath).toMatch(/^M[^Z]*$/)
    expect(noNaN(line?.linePath ?? '')).toBe(true)
    const area = layoutLines(spec({ type: 'area' }), PLOT, 'area')
    expect(area.series[0]?.areaPath).toMatch(/Z$/)
    expect(area.series[0]?.areaPath).toContain(String(Math.round(area.zeroY * 100) / 100))
  })

  it('stacks areas on the running total', () => {
    const area = layoutLines(spec({ type: 'area', stacked: true }), PLOT, 'area')
    const lower = area.series[0]?.points[0]
    const upper = area.series[1]?.points[0]
    expect(upper && lower ? upper.y < lower.y : false).toBe(true)
    expect(area.scale.max).toBeGreaterThanOrEqual(25 + 15)
  })

  it('handles a single category as a lone point', () => {
    const one = layoutLines(spec({ categories: ['a'], series: [{ name: 's', values: [3] }] }), PLOT, 'line')
    expect(one.series[0]?.points).toHaveLength(1)
    expect(one.series[0]?.points[0]?.x).toBe(PLOT.width / 2)
    expect(noNaN(one.series[0]?.linePath ?? '')).toBe(true)
  })

  it('does not exaggerate a tiny wobble: a flat-ish line near a big value starts the axis at a non-zero tick only when the range is tight', () => {
    const tight = layoutLines(spec({ categories: ['a', 'b', 'c'], series: [{ name: 's', values: [98, 100, 102] }] }), PLOT, 'line')
    expect(tight.scale.min).toBeGreaterThan(0)
    const wide = layoutLines(spec({ categories: ['a', 'b', 'c'], series: [{ name: 's', values: [20, 60, 120] }] }), PLOT, 'line')
    expect(wide.scale.min).toBe(0)
  })

  it('always anchors an area at zero', () => {
    const area = layoutLines(spec({ categories: ['a', 'b', 'c'], series: [{ name: 's', values: [98, 100, 102] }] }), PLOT, 'area')
    expect(area.scale.min).toBe(0)
  })
})

describe('layoutPie', () => {
  it('starts at twelve o clock, runs clockwise and fills the circle', () => {
    const pie = layoutPie([1, 1, 2], { cx: 100, cy: 100, radius: 80 })
    expect(pie.slices).toHaveLength(3)
    expect(pie.slices[0]?.startAngle).toBeCloseTo(-Math.PI / 2, 6)
    expect(pie.slices.at(-1)?.endAngle).toBeCloseTo((3 * Math.PI) / 2, 6)
    expect(pie.slices.map((slice) => slice.percent)).toEqual([25, 25, 50])
    for (const slice of pie.slices) {
      expect(slice.path).toMatch(/^M.*Z$/)
      expect(noNaN(slice.path)).toBe(true)
    }
  })

  it('keeps a zero slice in the list but draws nothing for it', () => {
    const pie = layoutPie([3, 0, 1], { cx: 0, cy: 0, radius: 50 })
    expect(pie.slices).toHaveLength(3)
    expect(pie.slices[1]?.path).toBe('')
    expect(pie.slices[1]?.percent).toBe(0)
  })

  it('draws a lone slice as a full circle', () => {
    const pie = layoutPie([5], { cx: 50, cy: 50, radius: 40 })
    expect(pie.slices[0]?.percent).toBe(100)
    expect(noNaN(pie.slices[0]?.path ?? '')).toBe(true)
    expect(pie.slices[0]?.path.match(/A/g)?.length).toBeGreaterThanOrEqual(2)
  })

  it('puts each label anchor inside the circle on the middle of the slice', () => {
    const pie = layoutPie([1, 1], { cx: 100, cy: 100, radius: 80 })
    const [right, left] = pie.slices
    expect(right && right.labelX > 100).toBe(true)
    expect(left && left.labelX < 100).toBe(true)
  })

  it('returns no slices for data that sums to zero', () => {
    expect(layoutPie([0, 0], { cx: 0, cy: 0, radius: 10 }).slices.every((slice) => slice.path === '')).toBe(true)
  })
})

describe('helpers', () => {
  it('estimates text width in proportion to length and size', () => {
    expect(estimateTextWidth('abcdef', 11)).toBeGreaterThan(estimateTextWidth('abc', 11))
    expect(estimateTextWidth('abc', 14)).toBeGreaterThan(estimateTextWidth('abc', 11))
    expect(estimateTextWidth('', 11)).toBe(0)
  })

  it('truncates a long label with an ellipsis and leaves a short one alone', () => {
    expect(truncateLabel('Kort', 10)).toBe('Kort')
    const long = truncateLabel('Et veldig langt kategorinavn', 10)
    expect(long.length).toBeLessThanOrEqual(10)
    expect(long.endsWith('\u2026')).toBe(true)
  })

  it('keeps the end of a label when asked to truncate in the middle', () => {
    expect(truncateMiddle('Kort', 10)).toBe('Kort')
    const a = truncateMiddle('Produktgruppe nummer 1', 18)
    const b = truncateMiddle('Produktgruppe nummer 12', 18)
    expect(a.length).toBeLessThanOrEqual(18)
    expect(b.length).toBeLessThanOrEqual(18)
    expect(a).not.toBe(b)
    expect(b.endsWith('er 12')).toBe(true)
    expect(b).toContain('\u2026')
    // End truncation alone would have made these two identical.
    expect(truncateLabel('Produktgruppe nummer 1', 18)).toBe(truncateLabel('Produktgruppe nummer 12', 18))
  })

  it('detects labels that would collide vertically', () => {
    expect(labelsCollide([10, 40, 80], 14)).toBe(false)
    expect(labelsCollide([10, 18, 80], 14)).toBe(true)
    expect(labelsCollide([], 14)).toBe(false)
  })

  it('gives each of six series its own marker shape', () => {
    const shapes = Array.from({ length: 6 }, (_, index) => markerShape(index))
    expect(new Set(shapes).size).toBe(6)
    expect(markerShape(6)).toBe(markerShape(0))
  })
})
