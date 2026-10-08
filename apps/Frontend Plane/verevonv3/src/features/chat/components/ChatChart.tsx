import { For, Match, Show, Switch, createEffect, createMemo, createSignal, createUniqueId, onCleanup, untrack } from 'solid-js'
import type { JSX } from '@solidjs/web'
import { Check, Download, Table2 } from '@/shared/icons'
import { useI18n } from '@/shared/i18n'
import { localeIntl } from '@/shared/i18n/locales'
import { downloadBytes } from '../lib/chat-export/download'
import {
  estimateTextWidth,
  layoutBars,
  layoutLines,
  layoutPie,
  markerPoints,
  markerShape,
  truncateLabel,
  truncateMiddle,
  type Band,
  type BarMark,
  type LineSeriesLayout,
  type MarkerShape,
  type PieSlice,
} from '../lib/chat-chart/geometry'
import {
  availableChartTypes,
  formatChartValue,
  parseChartFence,
  withChartType,
  type ChartParseFailure,
  type ChartSpec,
  type ChartType,
} from '../lib/chat-chart/spec'
import { chartFileName, serializeChartSvg } from '../lib/chat-chart/svg-export'

/**
 * One chart of the chat, drawn as a single SVG.
 *
 * Reads: a ```chart fence (`ChartFenceBlock`, mounted by `MarkdownCodeBlock`)
 * or a markdown table the reader chose to see as a chart (`ChatTableTools`).
 *
 * Accessibility, in the order a screen-reader or keyboard user meets it: the
 * SVG is `role="img"` named by its title and described by a summary; a data
 * table with every value follows it, visually hidden until "Vis tabell" is
 * pressed; the plot has one tab stop and arrow keys move between the
 * categories, each of which says all its values. Series are told apart by the
 * legend marker, the order and (lines) the marker shape, never colour alone.
 */

const FONT_SIZE = 11
const SERIES_COLOR_COUNT = 8
const DEFAULT_WIDTH = 640
const MIN_WIDTH = 240
const MAX_WIDTH = 760
const MAX_LABEL_CHARS = 18

const seriesColor = (index: number) => `var(--chart-s${(index % SERIES_COLOR_COUNT) + 1})`

// ── State that must survive a remount ───────────────────────────────────────
//
// `ChatMarkdown` reparses the whole message on every streamed token and hands
// every block a new object, so `<For>` remounts each block (see
// `TranscriptCodeBlock`). Anything the reader chose on a chart (its type, the
// open data table) and the last measured width are therefore kept here, keyed
// by the chart's own data, instead of in the component.

type ChartViewState = { type?: ChartType; showTable?: boolean }

const MAX_REMEMBERED = 200
const viewStates = new Map<string, ChartViewState>()
let lastMeasuredWidth = 0

function rememberView(key: string, patch: ChartViewState): void {
  const next = { ...viewStates.get(key), ...patch }
  viewStates.delete(key)
  viewStates.set(key, next)
  if (viewStates.size > MAX_REMEMBERED) {
    const oldest = viewStates.keys().next().value
    if (oldest !== undefined) viewStates.delete(oldest)
  }
}

function specKey(spec: ChartSpec): string {
  return JSON.stringify([spec.categories, spec.series, spec.unit, spec.title])
}

// ── Texts ───────────────────────────────────────────────────────────────────

type Translate = (no: string, en: string) => string

const FAILURE_TEXT: Record<ChartParseFailure, [string, string]> = {
  empty: ['diagrammet er tomt', 'the chart is empty'],
  'too-large': ['diagrammet er for stort', 'the chart is too large'],
  'not-json': ['kildeteksten er ikke gyldig JSON', 'the source is not valid JSON'],
  'not-object': ['kildeteksten er ikke et JSON-objekt', 'the source is not a JSON object'],
  'bad-type': ['ukjent diagramtype (bar, line, area eller pie)', 'unknown chart type (bar, line, area or pie)'],
  'no-categories': ['kategoriene mangler', 'the categories are missing'],
  'too-many-categories': ['over 24 kategorier', 'more than 24 categories'],
  'no-series': ['dataseriene mangler', 'the series are missing'],
  'too-many-series': ['over 6 dataserier', 'more than 6 series'],
  'too-many-slices': ['over 8 sektorer i kakediagrammet', 'more than 8 pie slices'],
  'bad-values': ['verdiene er ikke tall', 'the values are not numbers'],
  'length-mismatch': ['en serie har ikke like mange verdier som kategorier', 'a series does not have one value per category'],
  'pie-needs-one-series': ['et kakediagram kan bare ha én serie', 'a pie chart can only have one series'],
  'pie-values': ['et kakediagram trenger positive verdier', 'a pie chart needs positive values'],
}

export function chartFailureMessage(reason: ChartParseFailure, tr: Translate): string {
  const [no, en] = FAILURE_TEXT[reason]
  return tr(`Diagrammet kunne ikke tegnes: ${no}. Viser kildeteksten.`, `The chart could not be drawn: ${en}. Showing the source.`)
}

function typeName(type: ChartType, tr: Translate): string {
  switch (type) {
    case 'bar':
      return tr('Søyle', 'Bar')
    case 'line':
      return tr('Linje', 'Line')
    case 'area':
      return tr('Område', 'Area')
    case 'pie':
      return tr('Kake', 'Pie')
  }
}

function typeDescription(type: ChartType, tr: Translate): string {
  switch (type) {
    case 'bar':
      return tr('Søylediagram', 'Bar chart')
    case 'line':
      return tr('Linjediagram', 'Line chart')
    case 'area':
      return tr('Områdediagram', 'Area chart')
    case 'pie':
      return tr('Kakediagram', 'Pie chart')
  }
}

function describeChart(spec: ChartSpec, tr: Translate, locale: string): string {
  const values = spec.series.flatMap((series) => series.values)
  const low = Math.min(...values)
  const high = Math.max(...values)
  const categories = spec.categories.length
  const first = spec.categories[0] ?? ''
  const last = spec.categories[categories - 1] ?? ''
  const names = spec.series.map((series) => series.name).join(', ')
  const parts = [
    `${typeDescription(spec.type, tr)}.`,
    tr(
      `${categories} ${categories === 1 ? 'kategori' : 'kategorier'}${categories > 1 ? ` fra ${first} til ${last}` : `: ${first}`}.`,
      `${categories} ${categories === 1 ? 'category' : 'categories'}${categories > 1 ? ` from ${first} to ${last}` : `: ${first}`}.`,
    ),
    spec.type === 'pie'
      ? ''
      : tr(
          `${spec.series.length} ${spec.series.length === 1 ? 'serie' : 'serier'}: ${names}.`,
          `${spec.series.length} ${spec.series.length === 1 ? 'series' : 'series'}: ${names}.`,
        ),
    tr(
      `Verdier fra ${formatChartValue(low, '', locale)} til ${formatChartValue(high, spec.unit, locale)}.`,
      `Values from ${formatChartValue(low, '', locale)} to ${formatChartValue(high, spec.unit, locale)}.`,
    ),
    tr('Alle verdiene står i datatabellen under diagrammet.', 'Every value is in the data table below the chart.'),
  ]
  return parts.filter(Boolean).join(' ')
}

// ── Layout ──────────────────────────────────────────────────────────────────

type TickLabel = { y: number; label: string }

type CartesianView = {
  kind: 'cartesian'
  type: 'bar' | 'line' | 'area'
  width: number
  height: number
  left: number
  top: number
  plotWidth: number
  plotHeight: number
  zeroY: number
  ticks: TickLabel[]
  bands: Band[]
  xLabels: { x: number; text: string; full: string }[]
  rotate: boolean
  bars: BarMark[]
  lines: LineSeriesLayout[]
  valueLabels: string[]
}

type PieView = {
  kind: 'pie'
  width: number
  height: number
  cx: number
  cy: number
  radius: number
  slices: PieSlice[]
}

type ChartView = CartesianView | PieView

function clamp(value: number, low: number, high: number): number {
  return Math.min(high, Math.max(low, value))
}

function makeNumberFormat(locale: string, options: Intl.NumberFormatOptions): Intl.NumberFormat {
  try {
    return new Intl.NumberFormat(locale, options)
  } catch {
    return new Intl.NumberFormat('nb-NO', options)
  }
}

function buildView(spec: ChartSpec, width: number, locale: string): ChartView {
  if (spec.type === 'pie') {
    const height = Math.round(clamp(width * 0.5, 180, 250))
    const radius = Math.min(height / 2 - 10, width / 2 - 10)
    const slices = layoutPie(spec.series[0]?.values ?? [], { cx: width / 2, cy: height / 2, radius }).slices
    return { kind: 'pie', width, height, cx: width / 2, cy: height / 2, radius, slices }
  }

  const type = spec.type
  const plotHeight = Math.round(clamp(width * 0.42, 170, 250))
  const layoutFor = (plotWidth: number) =>
    type === 'bar'
      ? ({ kind: 'bar', layout: layoutBars(spec, { width: plotWidth, height: plotHeight }) } as const)
      : ({ kind: 'lines', layout: layoutLines(spec, { width: plotWidth, height: plotHeight }, type) } as const)

  // The scale depends on the data and the plot height only, so a first pass at
  // a rough width gives the tick labels the left margin is sized from.
  const provisional = layoutFor(Math.max(80, width - 60)).layout.scale
  const maxAbs = Math.max(Math.abs(provisional.min), Math.abs(provisional.max))
  const tickFormat =
    maxAbs >= 1e6
      ? makeNumberFormat(locale, { notation: 'compact', maximumFractionDigits: 1 })
      : makeNumberFormat(locale, { minimumFractionDigits: provisional.decimals, maximumFractionDigits: provisional.decimals })
  const tickTexts = provisional.ticks.map((tick) => tickFormat.format(tick))
  const widestTick = Math.max(0, ...tickTexts.map((text) => estimateTextWidth(text, FONT_SIZE)))
  const left = Math.ceil(widestTick) + 14
  const right = 14
  const plotWidth = Math.max(80, width - left - right)

  const built = layoutFor(plotWidth)
  const { bands, zeroY, yOf } = built.layout

  // Labels that differ only at their end ("Produktgruppe nummer 1", "... 12") would all read the same cut at the end.
  const endCut = spec.categories.map((category) => truncateLabel(category, MAX_LABEL_CHARS))
  const cutMakesDuplicates =
    new Set(spec.categories).size === spec.categories.length && new Set(endCut).size !== endCut.length
  const labelTexts = cutMakesDuplicates ? spec.categories.map((category) => truncateMiddle(category, MAX_LABEL_CHARS)) : endCut
  const widestLabel = Math.max(0, ...labelTexts.map((text) => estimateTextWidth(text, FONT_SIZE)))
  const bandWidth = plotWidth / Math.max(1, spec.categories.length)
  const rotate = widestLabel + 8 > bandWidth
  const step = rotate ? Math.max(1, Math.ceil(15 / bandWidth)) : Math.max(1, Math.ceil((widestLabel + 8) / bandWidth))
  const xLabels = spec.categories
    .map((category, index) => ({ x: bands[index]?.center ?? 0, text: labelTexts[index] ?? '', full: category, index }))
    .filter((label) => label.index % step === 0)
  const bottom = rotate ? Math.round(14 + Math.sin((40 * Math.PI) / 180) * widestLabel) + 8 : 32

  let valueLabels: string[] = []
  if (built.kind === 'bar' && !(spec.stacked && spec.series.length > 1)) {
    const texts = built.layout.bars.map((bar) => formatChartValue(bar.value, '', locale))
    const fits = built.layout.bars.every((bar, index) => {
      const slot = spec.series.length === 1 ? bandWidth - 6 : bar.width + 2
      return estimateTextWidth(texts[index] ?? '', FONT_SIZE) <= slot
    })
    if (fits) valueLabels = texts
  }

  const top = 24
  return {
    kind: 'cartesian',
    type,
    width,
    height: top + plotHeight + bottom,
    left,
    top,
    plotWidth,
    plotHeight,
    zeroY,
    ticks: provisional.ticks.map((tick, index) => ({ y: yOf(tick), label: tickTexts[index] ?? String(tick) })),
    bands,
    xLabels,
    rotate,
    bars: built.kind === 'bar' ? built.layout.bars : [],
    lines: built.kind === 'lines' ? built.layout.series : [],
    valueLabels,
  }
}

// ── Pieces ──────────────────────────────────────────────────────────────────

function MarkerGlyph(props: { shape: MarkerShape; cx: number; cy: number; r: number; index: number; ring: boolean }): JSX.Element {
  const style = () => ({ fill: seriesColor(props.index), stroke: props.ring ? 'var(--chart-surface)' : 'none' })
  return (
    <Show
      when={props.shape !== 'circle'}
      fallback={<circle cx={props.cx} cy={props.cy} r={props.r} stroke-width={props.ring ? 2 : 0} style={style()} />}
    >
      <polygon
        points={markerPoints(props.shape as Exclude<MarkerShape, 'circle'>, props.cx, props.cy, props.r)}
        stroke-width={props.ring ? 2 : 0}
        stroke-linejoin="round"
        style={style()}
      />
    </Show>
  )
}

function LegendMark(props: { kind: ChartType; index: number }): JSX.Element {
  return (
    <svg class="verevon-chat-chart__legend-mark" width="18" height="10" viewBox="0 0 18 10" aria-hidden="true">
      <Switch
        fallback={<rect x="2" y="1" width="14" height="8" rx="2" style={{ fill: seriesColor(props.index) }} />}
      >
        <Match when={props.kind === 'line'}>
          <line x1="0" y1="5" x2="18" y2="5" stroke-width="2" stroke-linecap="round" style={{ stroke: seriesColor(props.index) }} />
          <MarkerGlyph shape={markerShape(props.index)} cx={9} cy={5} r={3.4} index={props.index} ring={false} />
        </Match>
      </Switch>
    </svg>
  )
}

type TooltipRow = { name: string; value: string; kind: ChartType; index: number }
type Tooltip = { x: number; y: number; title: string; rows: TooltipRow[]; align: 'start' | 'center' | 'end' }

export type ChatChartProps = {
  spec: ChartSpec
  /** Where the reader's choices on this chart are remembered across remounts; defaults to the chart's own data. */
  stateKey?: string
}

export function ChatChart(props: ChatChartProps): JSX.Element {
  const i18n = useI18n()
  const tr: Translate = (no, en) => i18n.tr(no, en)
  const locale = () => localeIntl(i18n.locale())
  const key = () => props.stateKey ?? specKey(props.spec)
  // Initial values only: read once, outside any tracking scope.
  const remembered = untrack(() => viewStates.get(key()))

  const [type, setType] = createSignal<ChartType>(remembered?.type ?? untrack(() => props.spec.type))
  const [showTable, setShowTable] = createSignal(remembered?.showTable ?? false)
  const [width, setWidth] = createSignal(lastMeasuredWidth || DEFAULT_WIDTH)
  const [active, setActive] = createSignal<number | null>(null)
  const [focusIndex, setFocusIndex] = createSignal(0)
  const [saved, setSaved] = createSignal(false)

  const effectiveType = createMemo<ChartType>(() => {
    const offered = availableChartTypes(props.spec)
    return offered.includes(type()) ? type() : props.spec.type
  })
  const spec = createMemo(() => withChartType(props.spec, effectiveType()))
  const view = createMemo(() => buildView(spec(), width(), locale()))

  const titleId = createUniqueId()
  const descId = createUniqueId()
  const tableId = createUniqueId()
  const heading = () => props.spec.title || tr('Diagram', 'Chart')
  const description = createMemo(() => describeChart(spec(), tr, locale()))

  let figure: HTMLElement | undefined
  let svg: SVGSVGElement | undefined
  let plot: HTMLDivElement | undefined
  let observer: ResizeObserver | undefined
  let resizeFrame = 0
  let savedTimer: number | undefined

  // Solid v2 has no onMount: a two-phase effect with a constant compute runs once after mount.
  createEffect(
    () => undefined,
    () => {
      if (!plot || typeof ResizeObserver === 'undefined') return
      const apply = (measured: number) => {
        if (measured <= 0) return
        const next = Math.round(clamp(measured, MIN_WIDTH, MAX_WIDTH))
        lastMeasuredWidth = next
        setWidth(next)
      }
      apply(plot.getBoundingClientRect().width)
      // Applied on the next frame: the chart's height follows its width, and changing it from
      // inside the observer callback makes the browser report "ResizeObserver loop completed".
      observer = new ResizeObserver((entries) => {
        const entry = entries[0]
        if (!entry) return
        const measured = entry.contentRect.width
        window.cancelAnimationFrame(resizeFrame)
        resizeFrame = window.requestAnimationFrame(() => apply(measured))
      })
      observer.observe(plot)
    },
  )
  onCleanup(() => {
    observer?.disconnect()
    window.cancelAnimationFrame(resizeFrame)
    window.clearTimeout(savedTimer)
  })

  const choose = (next: ChartType) => {
    setType(next)
    setActive(null)
    rememberView(key(), { type: next })
  }
  const toggleTable = () => {
    const next = !showTable()
    setShowTable(next)
    rememberView(key(), { showTable: next })
  }

  const downloadSvg = () => {
    if (!svg || !figure) return
    const style = window.getComputedStyle(figure)
    const text = serializeChartSvg(svg, {
      background: style.getPropertyValue('--chart-surface').trim(),
      title: props.spec.title,
      titleColor: style.color,
    })
    downloadBytes(new TextEncoder().encode(text), chartFileName(props.spec.title, 'svg'), 'image/svg+xml')
    setSaved(true)
    window.clearTimeout(savedTimer)
    savedTimer = window.setTimeout(() => setSaved(false), 1800)
  }

  // Everything a category (or pie slice) says, for its aria-label and the tooltip.
  const rowsFor = (index: number): TooltipRow[] => {
    const current = spec()
    if (current.type === 'pie') {
      const slice = current.series[0]
      return [{ name: current.categories[index] ?? '', value: formatChartValue(slice?.values[index] ?? 0, current.unit, locale()), kind: 'pie', index }]
    }
    return current.series.map((series, seriesIndex) => ({
      name: series.name,
      value: formatChartValue(series.values[index] ?? 0, current.unit, locale()),
      kind: current.type,
      index: seriesIndex,
    }))
  }
  // A pie of percentages already says the share; any other unit gets it in brackets.
  const withShare = (valueText: string, percent: number) =>
    spec().unit === '%' ? valueText : `${valueText} (${formatPercent(percent, locale())})`
  const labelFor = (index: number): string => {
    const current = spec()
    const category = current.categories[index] ?? ''
    if (current.type === 'pie') {
      const percent = view().kind === 'pie' ? (view() as PieView).slices[index]?.percent ?? 0 : 0
      return `${category}: ${withShare(rowsFor(index)[0]?.value ?? '', percent)}`
    }
    if (current.series.length === 1) return `${category}: ${rowsFor(index)[0]?.value ?? ''}`
    return `${category}: ${rowsFor(index).map((row) => `${row.name} ${row.value}`).join(', ')}`
  }

  const tooltip = createMemo<Tooltip | null>(() => {
    const index = active()
    if (index === null) return null
    const current = view()
    if (current.kind === 'pie') {
      const slice = current.slices[index]
      if (!slice) return null
      const rows = rowsFor(index).map((row) => ({ ...row, value: withShare(row.value, slice.percent) }))
      return { x: slice.labelX, y: slice.labelY - 8, title: spec().categories[index] ?? '', rows, align: 'center' }
    }
    const band = current.bands[index]
    if (!band) return null
    const marks = current.type === 'bar'
      ? current.bars.filter((bar) => bar.category === index).map((bar) => bar.y)
      : current.lines.flatMap((line) => line.points.filter((point) => point.category === index).map((point) => point.y))
    const x = current.left + band.center
    const clearOfLabels = current.type === 'bar' && current.valueLabels.length > 0 ? 18 : 0
    const y = current.top + Math.min(...(marks.length > 0 ? marks : [current.plotHeight / 2])) - 8 - clearOfLabels
    const align = x < current.width * 0.2 ? 'start' : x > current.width * 0.8 ? 'end' : 'center'
    return { x, y, title: spec().categories[index] ?? '', rows: rowsFor(index), align }
  })

  // The categories that have a mark to focus: all of them, except a pie slice with no area (a
  // value of zero is drawn as nothing, so there is nothing there to tab to or to move onto).
  const hitIndexes = createMemo<number[]>(() => {
    const current = view()
    return current.kind === 'pie'
      ? current.slices.filter((slice) => slice.path !== '').map((slice) => slice.index)
      : spec().categories.map((_, index) => index)
  })
  // The one tab stop: the category last focused, or the first with a mark when that one has none now.
  const tabStop = () => {
    const present = hitIndexes()
    return present.includes(focusIndex()) ? focusIndex() : (present[0] ?? 0)
  }
  const focusPosition = (position: number) => {
    const present = hitIndexes()
    if (present.length === 0) return
    const next = present[((position % present.length) + present.length) % present.length] ?? 0
    setFocusIndex(next)
    setActive(next)
    svg?.querySelector<SVGElement>(`[data-chart-hit="${next}"]`)?.focus()
  }
  const moveFocus = (step: number) => focusPosition(Math.max(0, hitIndexes().indexOf(tabStop())) + step)
  const onKeyDown = (event: KeyboardEvent) => {
    switch (event.key) {
      case 'ArrowRight':
      case 'ArrowDown':
        event.preventDefault()
        moveFocus(1)
        break
      case 'ArrowLeft':
      case 'ArrowUp':
        event.preventDefault()
        moveFocus(-1)
        break
      case 'Home':
        event.preventDefault()
        focusPosition(0)
        break
      case 'End':
        event.preventDefault()
        focusPosition(hitIndexes().length - 1)
        break
      case 'Escape':
        setActive(null)
        break
    }
  }
  const hitProps = (index: number) => ({
    'data-chart-hit': String(index),
    role: 'graphics-symbol' as const,
    'aria-roledescription': tr('datapunkt', 'data point'),
    'aria-label': labelFor(index),
    tabindex: tabStop() === index ? 0 : -1,
    onPointerEnter: () => setActive(index),
    onFocus: () => {
      setFocusIndex(index)
      setActive(index)
    },
    onBlur: () => setActive((current) => (current === index ? null : current)),
    onKeyDown,
  })

  const legendEntries = createMemo(() => {
    const current = spec()
    if (current.type === 'pie') {
      const total = (current.series[0]?.values ?? []).reduce((sum, value) => sum + value, 0)
      return current.categories.map((category, index) => ({
        name: category,
        detail: withShare(
          formatChartValue(current.series[0]?.values[index] ?? 0, current.unit, locale()),
          total > 0 ? ((current.series[0]?.values[index] ?? 0) / total) * 100 : 0,
        ),
        kind: 'pie' as ChartType,
        index,
      }))
    }
    if (current.series.length < 2) return []
    return current.series.map((series, index) => ({ name: series.name, detail: '', kind: current.type, index }))
  })

  const typeOptions = createMemo(() => availableChartTypes(props.spec))

  return (
    <figure
      ref={figure}
      class="verevon-chat-chart"
      data-chart-type={effectiveType()}
      onPointerLeave={() => setActive(null)}
    >
      <div class="verevon-chat-chart__header">
        <Show when={props.spec.title}>
          <figcaption class="verevon-chat-chart__title">{props.spec.title}</figcaption>
        </Show>
        <div class="verevon-chat-chart__tools">
          <Show when={typeOptions().length > 1}>
            <div class="verevon-chat-chart__switch" role="group" aria-label={i18n.tr('Diagramtype', 'Chart type')}>
              <For each={typeOptions()}>
                {(option) => (
                  <button
                    type="button"
                    class="verevon-chat-chart__button"
                    aria-pressed={effectiveType() === option ? 'true' : 'false'}
                    onClick={() => choose(option)}
                  >
                    {typeName(option, tr)}
                  </button>
                )}
              </For>
            </div>
          </Show>
          <button
            type="button"
            class="verevon-chat-chart__button verevon-chat-chart__button--icon"
            aria-pressed={showTable() ? 'true' : 'false'}
            aria-controls={tableId}
            onClick={toggleTable}
            title={showTable() ? i18n.tr('Skjul tabell', 'Hide table') : i18n.tr('Vis tabell', 'Show table')}
          >
            <Table2 size={14} />
            <span>{showTable() ? tr('Skjul tabell', 'Hide table') : tr('Vis tabell', 'Show table')}</span>
          </button>
          <button
            type="button"
            class="verevon-chat-chart__button verevon-chat-chart__button--icon"
            onClick={downloadSvg}
            title={i18n.tr('Last ned SVG', 'Download SVG')}
          >
            <Show when={saved()} fallback={<Download size={14} />}>
              <Check size={14} />
            </Show>
            <span>{tr('Last ned SVG', 'Download SVG')}</span>
          </button>
        </div>
      </div>

      <div ref={plot} class="verevon-chat-chart__plot">
        <svg
          ref={svg}
          class="verevon-chat-chart__svg"
          role="img"
          aria-labelledby={titleId}
          aria-describedby={descId}
          viewBox={`0 0 ${view().width} ${view().height}`}
          preserveAspectRatio="xMidYMid meet"
        >
          <title id={titleId}>{heading()}</title>
          <desc id={descId}>{description()}</desc>
          <Switch>
            <Match when={view().kind === 'cartesian'}>
              <CartesianPlot
                view={view() as CartesianView}
                spec={spec()}
                active={active()}
                unit={props.spec.unit}
                hitProps={hitProps}
              />
            </Match>
            <Match when={view().kind === 'pie'}>
              <PiePlot view={view() as PieView} active={active()} hitProps={hitProps} />
            </Match>
          </Switch>
        </svg>
        <Show when={tooltip()}>
          {(tip) => (
            <div
              class="verevon-chat-chart__tooltip"
              data-align={tip().align}
              aria-hidden="true"
              style={{
                left: `${(tip().x / view().width) * 100}%`,
                top: `${(tip().y / view().height) * 100}%`,
              }}
            >
              <div class="verevon-chat-chart__tooltip-title">{tip().title}</div>
              <For each={tip().rows}>
                {(row) => (
                  <div class="verevon-chat-chart__tooltip-row">
                    <LegendMark kind={row.kind} index={row.index} />
                    <strong>{row.value}</strong>
                    <span>{row.name}</span>
                  </div>
                )}
              </For>
            </div>
          )}
        </Show>
      </div>

      <Show when={legendEntries().length > 0}>
        <ul class="verevon-chat-chart__legend">
          <For each={legendEntries()}>
            {(entry) => (
              <li>
                <LegendMark kind={entry.kind} index={entry.index} />
                <span class="verevon-chat-chart__legend-name">{entry.name}</span>
                <Show when={entry.detail}>
                  <span class="verevon-chat-chart__legend-detail">{entry.detail}</span>
                </Show>
              </li>
            )}
          </For>
        </ul>
      </Show>

      <ChartDataTable id={tableId} spec={spec()} visible={showTable()} locale={locale()} tr={tr} caption={heading()} />
    </figure>
  )
}

function formatPercent(percent: number, locale: string): string {
  return `${makeNumberFormat(locale, { maximumFractionDigits: percent < 10 ? 1 : 0 }).format(percent)}\u00a0%`
}

type HitProps = (index: number) => Record<string, unknown>

function CartesianPlot(props: {
  view: CartesianView
  spec: ChartSpec
  active: number | null
  unit: string
  hitProps: HitProps
}): JSX.Element {
  const v = () => props.view
  const showMarkers = () => props.spec.categories.length <= 12
  return (
    <g transform={`translate(${v().left} ${v().top})`}>
      <For each={v().ticks}>
        {(tick) => (
          <>
            <line class="verevon-chat-chart__grid" x1={0} x2={v().plotWidth} y1={tick.y} y2={tick.y} />
            <text class="verevon-chat-chart__tick" x={-8} y={tick.y} text-anchor="end" dominant-baseline="middle">{tick.label}</text>
          </>
        )}
      </For>
      <Show when={props.unit}>
        <text class="verevon-chat-chart__unit" x={-v().left + 4} y={-12} text-anchor="start">{props.unit}</text>
      </Show>

      <Show when={v().type === 'bar'}>
        <For each={v().bars}>
          {(bar) => <path d={bar.path} style={{ fill: seriesColor(bar.series) }} />}
        </For>
        <Show when={v().valueLabels.length > 0}>
          <For each={v().bars}>
            {(bar, index) => (
              <text class="verevon-chat-chart__value" x={bar.labelX} y={bar.labelY} text-anchor="middle">{v().valueLabels[index()]}</text>
            )}
          </For>
        </Show>
      </Show>

      <Show when={v().type !== 'bar'}>
        <Show when={props.active !== null}>
          <line
            class="verevon-chat-chart__crosshair"
            x1={v().bands[props.active ?? 0]?.center ?? 0}
            x2={v().bands[props.active ?? 0]?.center ?? 0}
            y1={0}
            y2={v().plotHeight}
          />
        </Show>
        <For each={v().lines}>
          {(line) => (
            <>
              <Show when={v().type === 'area'}>
                <path d={line.areaPath} stroke="none" style={{ fill: seriesColor(line.series), 'fill-opacity': props.spec.stacked ? 0.2 : 0.12 }} />
              </Show>
              <path
                d={line.linePath}
                fill="none"
                stroke-width={2}
                stroke-linecap="round"
                stroke-linejoin="round"
                style={{ stroke: seriesColor(line.series) }}
              />
              <For each={line.points}>
                {(point) => (
                  <Show when={showMarkers() || props.active === point.category}>
                    <MarkerGlyph
                      shape={markerShape(line.series)}
                      cx={point.x}
                      cy={point.y}
                      r={props.active === point.category ? 5.5 : 4}
                      index={line.series}
                      ring
                    />
                  </Show>
                )}
              </For>
            </>
          )}
        </For>
      </Show>

      <line class="verevon-chat-chart__baseline" x1={0} x2={v().plotWidth} y1={v().zeroY} y2={v().zeroY} />

      <For each={v().xLabels}>
        {(label) => (
          <text
            class="verevon-chat-chart__tick"
            x={label.x}
            y={v().plotHeight + 16}
            text-anchor={v().rotate ? 'end' : 'middle'}
            transform={v().rotate ? `rotate(-40 ${label.x} ${v().plotHeight + 16})` : undefined}
          >
            <title>{label.full}</title>
            {label.text}
          </text>
        )}
      </For>

      <For each={v().bands}>
        {(band, index) => (
          <rect
            class="verevon-chat-chart__hit"
            x={band.x}
            y={0}
            width={band.width}
            height={v().plotHeight}
            {...props.hitProps(index())}
          />
        )}
      </For>
    </g>
  )
}

function PiePlot(props: { view: PieView; active: number | null; hitProps: HitProps }): JSX.Element {
  return (
    <g>
      <For each={props.view.slices}>
        {(slice) => (
          <Show when={slice.path !== ''}>
            <path
              class="verevon-chat-chart__slice"
              d={slice.path}
              stroke-linejoin="round"
              style={{ fill: seriesColor(slice.index) }}
              data-active={props.active === slice.index ? 'true' : undefined}
              {...props.hitProps(slice.index)}
            />
            <Show when={slice.percent >= 7}>
              <text class="verevon-chat-chart__slice-label" x={slice.labelX} y={slice.labelY} text-anchor="middle" dominant-baseline="middle">
                {Math.round(slice.percent)}&nbsp;%
              </text>
            </Show>
          </Show>
        )}
      </For>
    </g>
  )
}

function ChartDataTable(props: {
  id: string
  spec: ChartSpec
  visible: boolean
  locale: string
  tr: Translate
  caption: string
}): JSX.Element {
  const isPie = () => props.spec.type === 'pie'
  return (
    <table
      id={props.id}
      class={{ 'verevon-chat-chart__table': true, 'verevon-chat-chart__table--visible': props.visible }}
    >
      <caption>{props.tr(`Data for diagrammet: ${props.caption}`, `Chart data: ${props.caption}`)}</caption>
      <thead>
        <tr>
          <th scope="col">{props.tr('Kategori', 'Category')}</th>
          <For each={props.spec.series}>{(series) => <th scope="col">{series.name}</th>}</For>
          <Show when={isPie()}>
            <th scope="col">{props.tr('Andel', 'Share')}</th>
          </Show>
        </tr>
      </thead>
      <tbody>
        <For each={props.spec.categories}>
          {(category, index) => (
            <tr>
              <th scope="row">{category}</th>
              <For each={props.spec.series}>
                {(series) => <td>{formatChartValue(series.values[index()] ?? 0, props.spec.unit, props.locale)}</td>}
              </For>
              <Show when={isPie()}>
                <td>
                  {formatPercent(
                    (() => {
                      const values = props.spec.series[0]?.values ?? []
                      const total = values.reduce((sum, value) => sum + value, 0)
                      return total > 0 ? ((values[index()] ?? 0) / total) * 100 : 0
                    })(),
                    props.locale,
                  )}
                </td>
              </Show>
            </tr>
          )}
        </For>
      </tbody>
    </table>
  )
}

/**
 * The body of a closed ```chart fence: the chart when the spec reads, the
 * ordinary code block plus a one-line note when it does not. Never throws.
 */
export function ChartFenceBlock(props: { text: string; fallback: JSX.Element }): JSX.Element {
  const i18n = useI18n()
  const parsed = createMemo(() => parseChartFence(props.text))
  const spec = createMemo(() => {
    const result = parsed()
    return result.ok ? result.spec : undefined
  })
  const note = createMemo(() => {
    const result = parsed()
    return result.ok ? '' : chartFailureMessage(result.reason, (no, en) => i18n.tr(no, en))
  })
  return (
    <Show
      when={spec()}
      fallback={
        <>
          {props.fallback}
          <p class="verevon-chat-chart__note" role="note">{note()}</p>
        </>
      }
    >
      {(ready) => <ChatChart spec={ready()} />}
    </Show>
  )
}
