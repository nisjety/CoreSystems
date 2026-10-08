// @vitest-environment jsdom

/**
 * The native chart of the chat: what it exposes to a screen reader and a
 * keyboard (role=img with a title and a description, a data table with every
 * value, one tab stop with arrow-key movement), what its type switch does, and
 * that the SVG can be saved as a self-contained file. Geometry and parsing have
 * their own tests next to `lib/chat-chart/`.
 */

import { fireEvent, render, screen } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ChartFenceBlock, ChatChart, chartFailureMessage } from './ChatChart'
import type { ChartSpec } from '../lib/chat-chart/spec'

const QUARTERS: ChartSpec = {
  type: 'bar',
  title: 'Omsetning per kvartal',
  categories: ['K1', 'K2', 'K3', 'K4'],
  series: [
    { name: '2025', values: [10, 12, 9, 14] },
    { name: '2026', values: [11, 13, 12, 16] },
  ],
  unit: 'MNOK',
  stacked: false,
}

const SHARES: ChartSpec = {
  type: 'bar',
  title: 'Andeler',
  categories: ['Nord', 'Sør', 'Vest'],
  series: [{ name: 'Andel', values: [50, 30, 20] }],
  unit: '%',
  stacked: false,
}

// A chart's remembered view state is module-level and keyed by its data; each
// test gets data of its own by renaming the title.
let counter = 0
function unique(spec: ChartSpec): ChartSpec {
  counter += 1
  return { ...spec, title: `${spec.title} ${counter}` }
}

function plain(text: string | null | undefined): string {
  return (text ?? '').replace(/[\u00a0\u202f]/g, ' ')
}

function hits(container: HTMLElement): SVGElement[] {
  return Array.from(container.querySelectorAll<SVGElement>('[data-chart-hit]'))
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('ChatChart', () => {
  it('is an image with a title and a description', () => {
    const { container } = render(() => <ChatChart spec={unique(QUARTERS)} />)
    const svg = container.querySelector('svg[role="img"]')
    expect(svg).toBeTruthy()
    const titleId = svg?.getAttribute('aria-labelledby') ?? ''
    const descId = svg?.getAttribute('aria-describedby') ?? ''
    expect(container.querySelector(`[id="${titleId}"]`)?.textContent).toMatch(/^Omsetning per kvartal/)
    const description = plain(container.querySelector(`[id="${descId}"]`)?.textContent)
    expect(description).toContain('Søylediagram')
    expect(description).toContain('4 kategorier fra K1 til K4')
    expect(description).toContain('2 serier: 2025, 2026')
    expect(description).toContain('datatabellen')
  })

  it('draws one bar per series per category', () => {
    const { container } = render(() => <ChatChart spec={unique(QUARTERS)} />)
    expect(container.querySelectorAll('svg.verevon-chat-chart__svg g > path')).toHaveLength(8)
    expect(container.querySelector('figure')?.getAttribute('data-chart-type')).toBe('bar')
  })

  it('always carries a data table with every value, hidden until asked for', () => {
    const { container } = render(() => <ChatChart spec={unique(QUARTERS)} />)
    const table = container.querySelector('table.verevon-chat-chart__table')
    expect(table).toBeTruthy()
    expect(table?.classList.contains('verevon-chat-chart__table--visible')).toBe(false)
    expect(Array.from(table?.querySelectorAll('thead th') ?? []).map((cell) => cell.textContent)).toEqual(['Kategori', '2025', '2026'])
    const rows = Array.from(table?.querySelectorAll('tbody tr') ?? [])
    expect(rows).toHaveLength(4)
    const cells = (row: Element | undefined) => Array.from(row?.querySelectorAll('th, td') ?? []).map((cell) => plain(cell.textContent))
    expect(cells(rows[0])).toEqual(['K1', '10 MNOK', '11 MNOK'])
    expect(cells(rows[3])).toEqual(['K4', '14 MNOK', '16 MNOK'])
    expect(table?.querySelector('caption')?.textContent).toContain('Omsetning per kvartal')
  })

  it('shows and hides the data table from the toolbar', async () => {
    const { container } = render(() => <ChatChart spec={unique(QUARTERS)} />)
    const toggle = screen.getByRole('button', { name: /vis tabell/i })
    expect(toggle.getAttribute('aria-pressed')).toBe('false')
    fireEvent.click(toggle)
    await vi.waitFor(() => {
      expect(container.querySelector('table.verevon-chat-chart__table--visible')).toBeTruthy()
    })
    const hide = screen.getByRole('button', { name: /skjul tabell/i })
    expect(hide.getAttribute('aria-pressed')).toBe('true')
    fireEvent.click(hide)
    await vi.waitFor(() => {
      expect(container.querySelector('table.verevon-chat-chart__table--visible')).toBeNull()
    })
  })

  it('names every series in a legend, with a marker per entry, when there are two or more', () => {
    const { container } = render(() => <ChatChart spec={unique(QUARTERS)} />)
    const entries = Array.from(container.querySelectorAll('.verevon-chat-chart__legend li'))
    expect(entries.map((entry) => entry.textContent)).toEqual(['2025', '2026'])
    for (const entry of entries) expect(entry.querySelector('svg')).toBeTruthy()
  })

  it('has no legend box for a single series', () => {
    const { container } = render(() => <ChatChart spec={unique({ ...QUARTERS, series: [{ name: '2025', values: [10, 12, 9, 14] }] })} />)
    expect(container.querySelector('.verevon-chat-chart__legend')).toBeNull()
  })

  it('switches between bar and line, drawing lines with a marker shape of their own per series', async () => {
    const { container } = render(() => <ChatChart spec={unique(QUARTERS)} />)
    expect(screen.queryByRole('button', { name: 'Kake' })).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Linje' }))
    await vi.waitFor(() => {
      expect(container.querySelector('figure')?.getAttribute('data-chart-type')).toBe('line')
    })
    expect(screen.getByRole('button', { name: 'Linje' }).getAttribute('aria-pressed')).toBe('true')
    expect(screen.getByRole('button', { name: 'Søyle' }).getAttribute('aria-pressed')).toBe('false')
    // Two series: two polylines, and the second series is not drawn with the first one's circle.
    const polylines = container.querySelectorAll('svg.verevon-chat-chart__svg g > path[fill="none"]')
    expect(polylines).toHaveLength(2)
    expect(container.querySelectorAll('svg.verevon-chat-chart__svg g circle').length).toBe(4)
    expect(container.querySelectorAll('svg.verevon-chat-chart__svg g polygon').length).toBe(4)
    expect(hits(container)).toHaveLength(4)
  })

  it('offers a pie for one series and shows each slice with its share, in the legend and the table', async () => {
    const { container } = render(() => <ChatChart spec={unique(SHARES)} />)
    fireEvent.click(screen.getByRole('button', { name: 'Kake' }))
    await vi.waitFor(() => {
      expect(container.querySelector('figure')?.getAttribute('data-chart-type')).toBe('pie')
    })
    expect(container.querySelectorAll('path.verevon-chat-chart__slice')).toHaveLength(3)
    const legend = Array.from(container.querySelectorAll('.verevon-chat-chart__legend li')).map((entry) => plain(entry.textContent))
    // A pie of percentages already says the share, so it is not repeated in brackets.
    expect(legend).toEqual(['Nord50 %', 'Sør30 %', 'Vest20 %'])
    expect(Array.from(container.querySelectorAll('table.verevon-chat-chart__table thead th')).map((cell) => cell.textContent)).toEqual(['Kategori', 'Andel', 'Andel'])
    expect(hits(container)).toHaveLength(3)
    expect(plain(hits(container)[0]?.getAttribute('aria-label'))).toBe('Nord: 50 %')
  })

  it('adds the share to a pie of amounts, in the legend, the spoken label and the tooltip', async () => {
    const amounts = unique({ ...SHARES, unit: 'MNOK', series: [{ name: 'Salg', values: [60, 30, 10] }] })
    const { container } = render(() => <ChatChart spec={{ ...amounts, type: 'pie' }} />)
    const legend = Array.from(container.querySelectorAll('.verevon-chat-chart__legend li')).map((entry) => plain(entry.textContent))
    expect(legend).toEqual(['Nord60 MNOK (60 %)', 'Sør30 MNOK (30 %)', 'Vest10 MNOK (10 %)'])
    expect(plain(hits(container)[1]?.getAttribute('aria-label'))).toBe('Sør: 30 MNOK (30 %)')
    fireEvent.pointerEnter(hits(container)[0] as SVGElement)
    await vi.waitFor(() => {
      expect(plain(container.querySelector('.verevon-chat-chart__tooltip')?.textContent)).toContain('60 MNOK (60 %)')
    })
  })

  it('gives each category a spoken label with every series value', () => {
    const { container } = render(() => <ChatChart spec={unique(QUARTERS)} />)
    const labels = hits(container).map((hit) => plain(hit.getAttribute('aria-label')))
    expect(labels).toEqual([
      'K1: 2025 10 MNOK, 2026 11 MNOK',
      'K2: 2025 12 MNOK, 2026 13 MNOK',
      'K3: 2025 9 MNOK, 2026 12 MNOK',
      'K4: 2025 14 MNOK, 2026 16 MNOK',
    ])
  })

  it('has one tab stop on the plot and moves between categories with the arrow keys', async () => {
    const { container } = render(() => <ChatChart spec={unique(QUARTERS)} />)
    const initial = hits(container)
    expect(initial.map((hit) => hit.getAttribute('tabindex'))).toEqual(['0', '-1', '-1', '-1'])

    initial[0]?.focus()
    fireEvent.keyDown(initial[0] as SVGElement, { key: 'ArrowRight' })
    await vi.waitFor(() => {
      expect(hits(container).map((hit) => hit.getAttribute('tabindex'))).toEqual(['-1', '0', '-1', '-1'])
    })
    expect(document.activeElement).toBe(hits(container)[1])

    fireEvent.keyDown(hits(container)[1] as SVGElement, { key: 'End' })
    await vi.waitFor(() => {
      expect(document.activeElement).toBe(hits(container)[3])
    })
    fireEvent.keyDown(hits(container)[3] as SVGElement, { key: 'ArrowRight' })
    await vi.waitFor(() => {
      expect(document.activeElement).toBe(hits(container)[0])
    })
  })

  it('keeps one tab stop and working arrow keys on a pie whose first slice has no area', async () => {
    // A slice of zero has no shape, so there is no mark to focus for it: the tab stop is the first slice that has one.
    const withZero = unique({ ...SHARES, type: 'pie', series: [{ name: 'Andel', values: [0, 30, 70] }] })
    const { container } = render(() => <ChatChart spec={withZero} />)
    expect(hits(container).map((hit) => hit.getAttribute('tabindex'))).toEqual(['0', '-1'])

    hits(container)[0]?.focus()
    fireEvent.keyDown(hits(container)[0] as SVGElement, { key: 'ArrowRight' })
    await vi.waitFor(() => {
      expect(document.activeElement).toBe(hits(container)[1])
    })
    expect(plain(hits(container)[1]?.getAttribute('aria-label'))).toBe('Vest: 70 %')
    expect(hits(container).map((hit) => hit.getAttribute('tabindex'))).toEqual(['-1', '0'])

    fireEvent.keyDown(hits(container)[1] as SVGElement, { key: 'ArrowRight' })
    await vi.waitFor(() => {
      expect(document.activeElement).toBe(hits(container)[0])
    })
    expect(plain(hits(container)[0]?.getAttribute('aria-label'))).toBe('Sør: 30 %')

    fireEvent.keyDown(hits(container)[0] as SVGElement, { key: 'End' })
    await vi.waitFor(() => {
      expect(document.activeElement).toBe(hits(container)[1])
    })
  })

  it('shows every series value of the category under the pointer or focus in a tooltip', async () => {
    const { container } = render(() => <ChatChart spec={unique(QUARTERS)} />)
    expect(container.querySelector('.verevon-chat-chart__tooltip')).toBeNull()
    fireEvent.pointerEnter(hits(container)[2] as SVGElement)
    await vi.waitFor(() => {
      expect(container.querySelector('.verevon-chat-chart__tooltip')).toBeTruthy()
    })
    const tooltip = container.querySelector('.verevon-chat-chart__tooltip')
    expect(tooltip?.getAttribute('aria-hidden')).toBe('true')
    expect(plain(tooltip?.textContent)).toBe('K39 MNOK202512 MNOK2026')
    fireEvent.pointerLeave(container.querySelector('figure') as HTMLElement)
    await vi.waitFor(() => {
      expect(container.querySelector('.verevon-chat-chart__tooltip')).toBeNull()
    })
    hits(container)[1]?.focus()
    await vi.waitFor(() => {
      expect(plain(container.querySelector('.verevon-chat-chart__tooltip')?.textContent)).toContain('K2')
    })
  })

  it('writes value labels on the bars of a single series when they fit', () => {
    const { container } = render(() => <ChatChart spec={unique({ ...QUARTERS, series: [{ name: '2025', values: [10, 12, 9, 14] }] })} />)
    expect(Array.from(container.querySelectorAll('text.verevon-chat-chart__value')).map((text) => text.textContent)).toEqual(['10', '12', '9', '14'])
  })

  it('draws one tick label per axis line and the unit at the top of the axis', () => {
    const { container } = render(() => <ChatChart spec={unique(QUARTERS)} />)
    expect(container.querySelectorAll('line.verevon-chat-chart__grid').length).toBeGreaterThanOrEqual(3)
    expect(container.querySelector('text.verevon-chat-chart__unit')?.textContent).toBe('MNOK')
    const ticks = Array.from(container.querySelectorAll('text.verevon-chat-chart__tick')).map((text) => text.textContent)
    expect(ticks).toContain('0')
    // Each category label also carries its full name as a hover title.
    expect(ticks.some((text) => text?.endsWith('K1'))).toBe(true)
  })

  it('treats category names as text, never as markup', () => {
    const hostile = unique({ ...QUARTERS, categories: ['<img src=x onerror=alert(1)>', 'b', 'c', 'd'] })
    const { container } = render(() => <ChatChart spec={hostile} />)
    expect(container.querySelector('img')).toBeNull()
    expect(container.querySelector('table.verevon-chat-chart__table tbody th')?.textContent).toBe('<img src=x onerror=alert(1)>')
  })

  it('keeps the reader-chosen type when the chart is mounted again (a streaming message remounts its blocks)', async () => {
    const spec = unique(QUARTERS)
    const first = render(() => <ChatChart spec={spec} />)
    fireEvent.click(screen.getByRole('button', { name: 'Linje' }))
    await vi.waitFor(() => {
      expect(first.container.querySelector('figure')?.getAttribute('data-chart-type')).toBe('line')
    })
    first.unmount()
    const second = render(() => <ChatChart spec={{ ...spec }} />)
    expect(second.container.querySelector('figure')?.getAttribute('data-chart-type')).toBe('line')
  })

  describe('SVG download', () => {
    let createObjectURL: ReturnType<typeof vi.fn>
    let clicked: { download: string }[]

    beforeEach(() => {
      createObjectURL = vi.fn(() => 'blob:chart')
      clicked = []
      vi.stubGlobal('URL', Object.assign(URL, { createObjectURL, revokeObjectURL: vi.fn() }))
      vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(function (this: HTMLAnchorElement) {
        clicked.push({ download: this.download })
      })
    })

    afterEach(() => {
      vi.restoreAllMocks()
    })

    it('saves a standalone SVG, without the invisible hover targets', async () => {
      render(() => <ChatChart spec={unique(QUARTERS)} />)
      fireEvent.click(screen.getByRole('button', { name: /last ned svg/i }))
      flush()
      await vi.waitFor(() => expect(createObjectURL).toHaveBeenCalledTimes(1))
      const blob = createObjectURL.mock.calls[0]?.[0] as Blob
      expect(blob.type).toBe('image/svg+xml')
      const text = await blob.text()
      expect(text.startsWith('<?xml')).toBe(true)
      expect(text).toContain('xmlns="http://www.w3.org/2000/svg"')
      expect(text).toContain('<title')
      expect(text).toContain('Omsetning per kvartal')
      // The visible title is drawn into the file, above the plot (it is HTML on screen).
      expect(text).toMatch(/<text[^>]*font-weight="650"[^>]*>Omsetning per kvartal \d+<\/text>/)
      expect(text).toMatch(/viewBox="0 0 \d+ \d+"/)
      expect(text).not.toContain('data-chart-hit')
      expect(text).not.toContain('tabindex')
      expect(clicked[0]?.download).toMatch(/^omsetning-per-kvartal-\d+\.svg$/)
    })
  })
})

describe('ChartFenceBlock', () => {
  const fallback = <pre data-testid="fallback">raw source</pre>

  it('draws the chart when the JSON reads', () => {
    const source = JSON.stringify({ type: 'bar', title: 'Fence chart', x: ['a', 'b'], series: [{ name: 's', values: [1, 2] }] })
    const { container } = render(() => <ChartFenceBlock text={source} fallback={fallback} />)
    expect(container.querySelector('svg[role="img"]')).toBeTruthy()
    expect(screen.queryByTestId('fallback')).toBeNull()
    expect(container.querySelector('.verevon-chat-chart__note')).toBeNull()
  })

  it('keeps the source and says why in one line when it does not', () => {
    const { container } = render(() => <ChartFenceBlock text={'{"type":"bar","x":["a"]'} fallback={fallback} />)
    expect(container.querySelector('svg')).toBeNull()
    expect(screen.getByTestId('fallback').textContent).toBe('raw source')
    const note = container.querySelector('.verevon-chat-chart__note')
    expect(note?.textContent).toBe('Diagrammet kunne ikke tegnes: kildeteksten er ikke gyldig JSON. Viser kildeteksten.')
    expect(note?.getAttribute('role')).toBe('note')
  })

  it('has a message for every way a spec can fail', () => {
    const reasons = [
      'empty', 'too-large', 'not-json', 'not-object', 'bad-type', 'no-categories', 'too-many-categories', 'no-series',
      'too-many-series', 'too-many-slices', 'bad-values', 'length-mismatch', 'pie-needs-one-series', 'pie-values',
    ] as const
    for (const reason of reasons) {
      expect(chartFailureMessage(reason, (no) => no)).toMatch(/^Diagrammet kunne ikke tegnes: .+\. Viser kildeteksten\.$/)
      expect(chartFailureMessage(reason, (_no, en) => en)).toMatch(/^The chart could not be drawn: .+\. Showing the source\.$/)
    }
  })
})
