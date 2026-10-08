/**
 * Where everything of a chart goes, as plain numbers and SVG path strings.
 *
 * Pure and DOM-free: `ChatChart.tsx` draws what this returns, and the Office
 * exports of H3 (direct PDF) and H4 (charts inside Word, Excel and PowerPoint)
 * read the same layout instead of laying a chart out a second time. No Solid,
 * no `window`, no text measuring: text width is estimated by
 * `estimateTextWidth`, which is close enough to place an axis and decide that a
 * label does not fit.
 *
 * Mark rules (from the data-viz guidance this renders by): bars are at most
 * 24 px thick with a 4 px rounded data end and a square baseline end, a 2 px
 * surface gap separates touching bars and stacked segments, lines are drawn by
 * the caller at 2 px, and a bar chart always starts at zero.
 */

import type { ChartSpec } from './spec.ts'

export const BAR_MAX_THICKNESS = 24
export const BAR_GAP = 2
const BAR_END_RADIUS = 4
const LABEL_OFFSET = 6

export type PlotBox = { width: number; height: number }

export type NiceScale = {
  min: number
  max: number
  step: number
  ticks: number[]
  /** Fraction digits that show every tick exactly. */
  decimals: number
}

function round2(value: number): string {
  const rounded = Math.round(value * 100) / 100
  return String(Object.is(rounded, -0) ? 0 : rounded)
}

function clamp(value: number, low: number, high: number): number {
  return Math.min(high, Math.max(low, value))
}

/** Heckbert's "nice number": 1, 2, 5 or 10 times a power of ten. */
function niceStep(raw: number): number {
  const exponent = Math.floor(Math.log10(raw))
  const power = 10 ** exponent
  const fraction = raw / power
  const nice = fraction < 1.5 ? 1 : fraction < 3 ? 2 : fraction < 7 ? 5 : 10
  return nice * power
}

function decimalsFor(step: number): number {
  return clamp(Math.max(0, -Math.floor(Math.log10(step))), 0, 12)
}

function cleanNumber(value: number, decimals: number): number {
  return Number(value.toFixed(decimals)) + 0
}

/**
 * Round tick values spanning `min`..`max`, about `count` of them.
 *
 * `includeZero` (a bar or area chart always) widens the range to reach zero.
 * Without it a flat dataset gets a margin around its value instead. A single
 * value, all values equal, an all-zero dataset, a reversed range and
 * non-finite input all give a usable axis.
 */
export function niceTicks(min: number, max: number, count = 5, includeZero = true): NiceScale {
  if (!Number.isFinite(min) || !Number.isFinite(max)) return niceTicks(0, 1, count, true)
  let low = Math.min(min, max)
  let high = Math.max(min, max)
  if (includeZero) {
    low = Math.min(low, 0)
    high = Math.max(high, 0)
  }
  if (low === high) {
    if (low === 0) {
      high = 1
    } else {
      const pad = Math.abs(low) * 0.1
      low -= pad
      high += pad
    }
  }
  const step = niceStep((high - low) / Math.max(1, count - 1))
  const decimals = decimalsFor(step)
  const niceMin = cleanNumber(Math.floor(low / step) * step, decimals)
  const niceMax = cleanNumber(Math.ceil(high / step) * step, decimals)
  const intervals = Math.max(1, Math.round((niceMax - niceMin) / step))
  // Values at the edge of what a double holds overflow the arithmetic above (the top tick
  // becomes Infinity, the tick count is not a length) or underflow it (a step of zero). Such a
  // scale cannot be drawn, and building its ticks would throw inside the chart's render.
  if (!Number.isFinite(niceMin) || !Number.isFinite(niceMax) || !(step > 0) || !(intervals <= 100)) {
    return niceTicks(0, 1, count, true)
  }
  const ticks = Array.from({ length: intervals + 1 }, (_, index) => cleanNumber(niceMin + index * step, decimals))
  return { min: niceMin, max: niceMax, step, ticks, decimals }
}

function tickCountFor(height: number): number {
  return clamp(Math.round(height / 48) + 1, 3, 7)
}

export type Band = { x: number; width: number; center: number }

function makeBands(count: number, width: number): Band[] {
  const bandWidth = width / count
  return Array.from({ length: count }, (_, index) => ({
    x: index * bandWidth,
    width: bandWidth,
    center: (index + 0.5) * bandWidth,
  }))
}

function makeYOf(scale: NiceScale, height: number): (value: number) => number {
  const span = scale.max - scale.min || 1
  return (value) => height - ((value - scale.min) / span) * height
}

// ── Bars ────────────────────────────────────────────────────────────────────

export type BarMark = {
  series: number
  category: number
  value: number
  x: number
  y: number
  width: number
  height: number
  /** Closed path: square at the baseline, rounded at the data end. */
  path: string
  /** Where the value label sits, past the data end. */
  labelX: number
  labelY: number
}

export type BarLayout = {
  scale: NiceScale
  zeroY: number
  bands: Band[]
  bars: BarMark[]
  yOf: (value: number) => number
}

function barPath(x: number, y: number, width: number, height: number, up: boolean, rounded: boolean): string {
  const r = rounded ? Math.min(BAR_END_RADIUS, width / 2, height) : 0
  const [x0, y0, x1, y1] = [round2(x), round2(y), round2(x + width), round2(y + height)]
  if (r <= 0) return `M${x0} ${y0}L${x1} ${y0}L${x1} ${y1}L${x0} ${y1}Z`
  const left = round2(x + r)
  const right = round2(x + width - r)
  if (up) {
    const top = round2(y + r)
    return `M${x0} ${y1}L${x0} ${top}Q${x0} ${y0} ${left} ${y0}L${right} ${y0}Q${x1} ${y0} ${x1} ${top}L${x1} ${y1}Z`
  }
  const bottom = round2(y + height - r)
  return `M${x0} ${y0}L${x1} ${y0}L${x1} ${bottom}Q${x1} ${y1} ${right} ${y1}L${left} ${y1}Q${x0} ${y1} ${x0} ${bottom}Z`
}

/** Width of each bar of a group and the gap between them, for a band this wide. */
function groupMetrics(bandWidth: number, bars: number): { thickness: number; gap: number } {
  let inner = bandWidth * 0.7
  let gap = bars > 1 ? BAR_GAP : 0
  let thickness = (inner - gap * (bars - 1)) / bars
  if (thickness < 4) {
    inner = bandWidth * 0.92
    thickness = (inner - gap * (bars - 1)) / bars
  }
  if (thickness < 2 && gap > 1) {
    gap = 1
    thickness = (inner - gap * (bars - 1)) / bars
  }
  if (thickness < 1) {
    gap = 0
    thickness = inner / bars
  }
  return { thickness: Math.min(BAR_MAX_THICKNESS, Math.max(thickness, 0.5)), gap }
}

function dataExtent(values: readonly number[]): { min: number; max: number } {
  return { min: Math.min(...values), max: Math.max(...values) }
}

/** Grouped or stacked columns for a bar spec inside `plot`. */
export function layoutBars(spec: ChartSpec, plot: PlotBox): BarLayout {
  const categoryCount = spec.categories.length
  const seriesCount = spec.series.length
  const stacked = spec.stacked && seriesCount > 1

  const extremes: number[] = [0]
  if (stacked) {
    for (let category = 0; category < categoryCount; category += 1) {
      let positive = 0
      let negative = 0
      for (const series of spec.series) {
        const value = series.values[category] ?? 0
        if (value >= 0) positive += value
        else negative += value
      }
      extremes.push(positive, negative)
    }
  } else {
    for (const series of spec.series) extremes.push(...series.values)
  }
  const { min, max } = dataExtent(extremes)
  const scale = niceTicks(min, max, tickCountFor(plot.height), true)
  const yOf = makeYOf(scale, plot.height)
  const zeroY = yOf(0)
  const bands = makeBands(Math.max(1, categoryCount), plot.width)
  const bars: BarMark[] = []

  for (let category = 0; category < categoryCount; category += 1) {
    const band = bands[category]
    if (!band) continue

    if (stacked) {
      const { thickness } = groupMetrics(band.width, 1)
      const x = band.center - thickness / 2
      let positiveBase = 0
      let negativeBase = 0
      let lastPositive = -1
      let lastNegative = -1
      spec.series.forEach((series, index) => {
        const value = series.values[category] ?? 0
        if (value > 0) lastPositive = index
        else if (value < 0) lastNegative = index
      })
      spec.series.forEach((series, index) => {
        const value = series.values[category] ?? 0
        const up = value >= 0
        const from = up ? positiveBase : negativeBase
        const to = from + value
        if (up) positiveBase = to
        else negativeBase = to
        const yFrom = yOf(from)
        const yTo = yOf(to)
        let y = Math.min(yFrom, yTo)
        let height = Math.abs(yFrom - yTo)
        // The surface gap sits on the baseline side of every segment that does not touch the baseline.
        if (from !== 0 && height > 0) {
          const cut = Math.min(BAR_GAP, height)
          height -= cut
          if (!up) y += cut
        }
        if (value !== 0 && height < 1 && from === 0) {
          height = 1
          if (up) y = zeroY - 1
        }
        const rounded = value !== 0 && index === (up ? lastPositive : lastNegative)
        bars.push({
          series: index,
          category,
          value,
          x,
          y,
          width: thickness,
          height,
          path: barPath(x, y, thickness, height, up, rounded),
          labelX: x + thickness / 2,
          labelY: up ? y - LABEL_OFFSET : y + height + LABEL_OFFSET + 8,
        })
      })
      continue
    }

    const { thickness, gap } = groupMetrics(band.width, seriesCount)
    const groupWidth = thickness * seriesCount + gap * (seriesCount - 1)
    const left = band.x + (band.width - groupWidth) / 2
    spec.series.forEach((series, index) => {
      const value = series.values[category] ?? 0
      const up = value >= 0
      const x = left + index * (thickness + gap)
      const tip = yOf(value)
      let y = Math.min(tip, zeroY)
      let height = Math.abs(zeroY - tip)
      if (value !== 0 && height < 1) {
        height = 1
        if (up) y = zeroY - 1
      }
      bars.push({
        series: index,
        category,
        value,
        x,
        y,
        width: thickness,
        height,
        path: barPath(x, y, thickness, height, up, value !== 0),
        labelX: x + thickness / 2,
        labelY: up ? y - LABEL_OFFSET : y + height + LABEL_OFFSET + 8,
      })
    })
  }

  return { scale, zeroY, bands, bars, yOf }
}

// ── Lines and areas ─────────────────────────────────────────────────────────

export type LinePoint = { series: number; category: number; value: number; x: number; y: number }

export type LineSeriesLayout = {
  series: number
  points: LinePoint[]
  /** Open polyline through the points. */
  linePath: string
  /** Region between the series and the baseline (or the series below it when stacked); '' for a line chart. */
  areaPath: string
}

export type LineLayout = {
  scale: NiceScale
  zeroY: number
  bands: Band[]
  series: LineSeriesLayout[]
  yOf: (value: number) => number
}

function polyline(points: readonly { x: number; y: number }[]): string {
  return points.map((point, index) => `${index === 0 ? 'M' : 'L'}${round2(point.x)} ${round2(point.y)}`).join('')
}

/**
 * Lines (`kind: 'line'`) or areas (`kind: 'area'`, stacked when the spec says
 * so). An area always starts at zero; a line starts at zero unless the data
 * sits in a narrow band far above it, so a 98..102 wobble is not drawn as a
 * cliff and a 20..120 climb is not drawn from the middle of nowhere.
 */
export function layoutLines(spec: ChartSpec, plot: PlotBox, kind: 'line' | 'area'): LineLayout {
  const categoryCount = spec.categories.length
  const stacked = kind === 'area' && spec.stacked && spec.series.length > 1

  // Value each point is plotted at: its own, or the running total when stacked.
  const running: number[][] = []
  const lower: number[][] = []
  const totals = new Array<number>(categoryCount).fill(0)
  for (const series of spec.series) {
    lower.push(totals.map((total) => (stacked ? total : 0)))
    const plotted = series.values.map((value, category) => {
      if (stacked) totals[category] = (totals[category] ?? 0) + value
      return stacked ? (totals[category] ?? 0) : value
    })
    running.push(plotted)
  }

  const all = running.flat()
  const { min, max } = dataExtent(all.length > 0 ? all : [0])
  const spread = max - min
  const includeZero = kind === 'area' || min <= 0 || spread >= 0.25 * Math.abs(max)
  const scale = niceTicks(min, max, tickCountFor(plot.height), includeZero)
  const yOf = makeYOf(scale, plot.height)
  const baselineY = yOf(scale.min <= 0 && scale.max >= 0 ? 0 : scale.min)
  const bands = makeBands(Math.max(1, categoryCount), plot.width)

  const series = spec.series.map((entry, index): LineSeriesLayout => {
    const points = (running[index] ?? []).map(
      (plotted, category): LinePoint => ({
        series: index,
        category,
        value: entry.values[category] ?? 0,
        x: bands[category]?.center ?? 0,
        y: yOf(plotted),
      }),
    )
    let areaPath = ''
    if (kind === 'area' && points.length >= 2) {
      const base = (lower[index] ?? []).map((value, category) => ({
        x: bands[category]?.center ?? 0,
        y: stacked ? yOf(value) : baselineY,
      }))
      areaPath = `${polyline(points)}${[...base]
        .reverse()
        .map((point) => `L${round2(point.x)} ${round2(point.y)}`)
        .join('')}Z`
    }
    return { series: index, points, linePath: polyline(points), areaPath }
  })

  return { scale, zeroY: baselineY, bands, series, yOf }
}

// ── Pie ─────────────────────────────────────────────────────────────────────

export type PieSlice = {
  index: number
  value: number
  /** Share of the whole, 0 to 100. */
  percent: number
  startAngle: number
  endAngle: number
  /** '' for a slice with no area. */
  path: string
  /** Inside the slice, on its middle. */
  labelX: number
  labelY: number
}

export type PieLayout = { slices: PieSlice[] }

/** A pie that starts at twelve o clock and runs clockwise; slices keep their data order. */
export function layoutPie(
  values: readonly number[],
  circle: { cx: number; cy: number; radius: number },
): PieLayout {
  const { cx, cy, radius } = circle
  const total = values.reduce((sum, value) => sum + Math.max(0, value), 0)
  let angle = -Math.PI / 2
  const slices = values.map((raw, index): PieSlice => {
    const value = Math.max(0, raw)
    const fraction = total > 0 ? value / total : 0
    const startAngle = angle
    const endAngle = angle + fraction * Math.PI * 2
    angle = endAngle
    const middle = (startAngle + endAngle) / 2
    const labelX = cx + Math.cos(middle) * radius * 0.62
    const labelY = cy + Math.sin(middle) * radius * 0.62
    let path = ''
    if (fraction >= 0.99999) {
      path = `M${round2(cx - radius)} ${round2(cy)}A${round2(radius)} ${round2(radius)} 0 1 1 ${round2(cx + radius)} ${round2(cy)}A${round2(radius)} ${round2(radius)} 0 1 1 ${round2(cx - radius)} ${round2(cy)}Z`
    } else if (fraction > 0) {
      const x0 = cx + Math.cos(startAngle) * radius
      const y0 = cy + Math.sin(startAngle) * radius
      const x1 = cx + Math.cos(endAngle) * radius
      const y1 = cy + Math.sin(endAngle) * radius
      const large = fraction > 0.5 ? 1 : 0
      path = `M${round2(cx)} ${round2(cy)}L${round2(x0)} ${round2(y0)}A${round2(radius)} ${round2(radius)} 0 ${large} 1 ${round2(x1)} ${round2(y1)}Z`
    }
    return { index, value, percent: fraction * 100, startAngle, endAngle, path, labelX, labelY }
  })
  return { slices }
}

// ── Small helpers ───────────────────────────────────────────────────────────

/** A close estimate of a label's rendered width, for axis margins and "does it fit". */
export function estimateTextWidth(text: string, fontSize: number): number {
  return Array.from(text).length * fontSize * 0.56
}

export function truncateLabel(text: string, maxChars: number): string {
  const characters = Array.from(text)
  if (characters.length <= maxChars) return text
  return `${characters.slice(0, Math.max(1, maxChars - 1)).join('').trimEnd()}\u2026`
}

/**
 * Like `truncateLabel`, but keeps the end of the text: "Produktgruppe nummer 12"
 * becomes "Produktgrup\u2026mer 12", so labels that differ only at their end stay apart.
 */
export function truncateMiddle(text: string, maxChars: number): string {
  const characters = Array.from(text)
  if (characters.length <= maxChars) return text
  const room = Math.max(2, maxChars - 1)
  const head = Math.ceil(room * 0.6)
  const tail = room - head
  return `${characters.slice(0, head).join('').trimEnd()}\u2026${characters.slice(characters.length - tail).join('').trimStart()}`
}

/** True when two of these y positions are closer than `minGap`: labels there would overlap. */
export function labelsCollide(ys: readonly number[], minGap: number): boolean {
  const sorted = [...ys].sort((left, right) => left - right)
  return sorted.some((y, index) => index > 0 && y - (sorted[index - 1] ?? y) < minGap)
}

export type MarkerShape = 'circle' | 'square' | 'diamond' | 'triangle' | 'triangleDown' | 'hexagon'

const MARKER_SHAPES: readonly MarkerShape[] = ['circle', 'square', 'diamond', 'triangle', 'triangleDown', 'hexagon']

/** Each series of a line chart gets its own shape, so lines never rely on colour alone. */
export function markerShape(seriesIndex: number): MarkerShape {
  return MARKER_SHAPES[seriesIndex % MARKER_SHAPES.length] ?? 'circle'
}

/** The `points` of a polygon marker centred on `cx`,`cy` (not for a circle). */
export function markerPoints(shape: Exclude<MarkerShape, 'circle'>, cx: number, cy: number, r: number): string {
  const offsets: [number, number][] = (() => {
    switch (shape) {
      case 'square':
        return [[-0.86, -0.86], [0.86, -0.86], [0.86, 0.86], [-0.86, 0.86]]
      case 'diamond':
        return [[0, -1.15], [1.15, 0], [0, 1.15], [-1.15, 0]]
      case 'triangle':
        return [[0, -1.15], [1.05, 0.85], [-1.05, 0.85]]
      case 'triangleDown':
        return [[0, 1.15], [1.05, -0.85], [-1.05, -0.85]]
      case 'hexagon':
        return [[-1, 0], [-0.5, -0.87], [0.5, -0.87], [1, 0], [0.5, 0.87], [-0.5, 0.87]]
    }
  })()
  return offsets.map(([dx, dy]) => `${round2(cx + dx * r)},${round2(cy + dy * r)}`).join(' ')
}
