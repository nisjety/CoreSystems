// @vitest-environment jsdom

/**
 * Charts inside a rendered answer.
 *
 *  - A closed ```chart fence is drawn natively; one that is still streaming in,
 *    or whose JSON does not read, stays the ordinary code block (the second with
 *    a one-line note), exactly like a mermaid fence.
 *  - A markdown table gets a hover toolbar: copy as tab-separated text, download
 *    CSV (UTF-8 byte order mark, semicolons for Norwegian) and, when the table
 *    can honestly be a chart, "Vis som diagram".
 *  - What the reader chose survives the remount `ChatMarkdown` causes on every
 *    streamed token.
 */

import { fireEvent, render, screen } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ChatMarkdown, MarkdownCodeBlock, MarkdownTable } from './ChatMessages'
import type { MarkdownBlock } from './chat-types'

const SPEC = JSON.stringify({
  type: 'bar',
  title: 'Omsetning',
  x: ['K1', 'K2'],
  series: [{ name: '2026', values: [10, 12] }],
  unit: 'MNOK',
})

function codeBlock(overrides: Partial<Extract<MarkdownBlock, { kind: 'code' }>> = {}): Extract<MarkdownBlock, { kind: 'code' }> {
  return { kind: 'code', lang: 'chart', text: SPEC, closed: true, ...overrides }
}

function tableBlock(
  overrides: Partial<Extract<MarkdownBlock, { kind: 'table' }>> = {},
): Extract<MarkdownBlock, { kind: 'table' }> {
  return {
    kind: 'table',
    align: [null, null],
    header: ['Kvartal', 'Omsetning'],
    rows: [['K1', '1 200,5'], ['K2', '1 350']],
    ...overrides,
  }
}

afterEach(() => {
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
})

describe('MarkdownCodeBlock with a chart fence', () => {
  it('draws a closed, valid chart fence as a chart, not as code', () => {
    const { container } = render(() => <MarkdownCodeBlock block={codeBlock()} />)
    expect(container.querySelector('svg[role="img"]')).toBeTruthy()
    expect(container.querySelector('.verevon-chat-codeblock')).toBeNull()
  })

  it('keeps a still-streaming chart fence as a plain code block, never parsed', () => {
    const { container } = render(() => <MarkdownCodeBlock block={codeBlock({ closed: false, text: SPEC.slice(0, 30) })} />)
    expect(container.querySelector('svg[role="img"]')).toBeNull()
    expect(container.querySelector('.verevon-chat-chart__note')).toBeNull()
    expect(container.querySelector('pre code')?.textContent).toBe(SPEC.slice(0, 30))
  })

  it('falls back to the code block with a one-line note when the spec is bad', () => {
    const { container } = render(() => <MarkdownCodeBlock block={codeBlock({ text: '{"type":"radar"}' })} />)
    expect(container.querySelector('svg[role="img"]')).toBeNull()
    expect(container.querySelector('pre code')?.textContent).toBe('{"type":"radar"}')
    expect(container.querySelector('.verevon-chat-chart__note')?.textContent).toContain('ukjent diagramtype')
  })

  it('reads the language case-insensitively, and leaves other languages alone', () => {
    const upper = render(() => <MarkdownCodeBlock block={codeBlock({ lang: ' Chart ' })} />)
    expect(upper.container.querySelector('svg[role="img"]')).toBeTruthy()
    upper.unmount()
    const json = render(() => <MarkdownCodeBlock block={codeBlock({ lang: 'json' })} />)
    expect(json.container.querySelector('svg[role="img"]')).toBeNull()
    expect(json.container.querySelector('.verevon-chat-codeblock')).toBeTruthy()
  })

  it('shows the chart once the fence closes while the message streams in', async () => {
    const open = `Her er tallene:\n\n\`\`\`chart\n${SPEC}`
    const [content, setContent] = createSignal(open)
    const { container } = render(() => <ChatMarkdown content={content()} />)
    expect(container.querySelector('svg[role="img"]')).toBeNull()
    expect(container.querySelector('pre code')).toBeTruthy()

    setContent(`${open}\n\`\`\`\n\nFerdig.`)
    await vi.waitFor(() => {
      expect(container.querySelector('svg[role="img"]')).toBeTruthy()
    })
    expect(container.querySelector('pre code')).toBeNull()
    expect(container.textContent).toContain('Ferdig.')
  })
})

describe('MarkdownTable toolbar', () => {
  it('offers copy, CSV and the chart on a table that can be charted, without changing the table', () => {
    const { container } = render(() => <MarkdownTable block={tableBlock()} />)
    const bar = container.querySelector('.verevon-chat-table-tools__bar')
    expect(bar?.getAttribute('role')).toBe('toolbar')
    expect(Array.from(bar?.querySelectorAll('button') ?? []).map((button) => button.textContent)).toEqual([
      'Kopier',
      'Last ned CSV',
      'Vis som diagram',
    ])
    // The table itself is untouched: same wrap, one table, same cells.
    expect(container.querySelectorAll('.verevon-chat-table-wrap > table.verevon-chat-table')).toHaveLength(1)
    expect(Array.from(container.querySelectorAll('tbody td')).map((cell) => cell.textContent)).toEqual(['K1', '1 200,5', 'K2', '1 350'])
  })

  it('shows no chart button when no honest chart can be made of the table', () => {
    const { container } = render(() => (
      <MarkdownTable block={tableBlock({ header: ['Navn', 'Rolle'], rows: [['A', 'Leder'], ['B', 'Utvikler']] })} />
    ))
    expect(container.querySelector('.verevon-chat-table-tools__bar')).toBeTruthy()
    expect(screen.queryByRole('button', { name: /diagram/i })).toBeNull()
    expect(screen.getByRole('button', { name: 'Kopier' })).toBeTruthy()
  })

  it('opens the chart under the table and closes it again', async () => {
    const { container } = render(() => <MarkdownTable block={tableBlock({ rows: [['K1', '10'], ['K2', '12'], ['K3', '9']] })} />)
    expect(container.querySelector('figure.verevon-chat-chart')).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Vis som diagram' }))
    await vi.waitFor(() => {
      expect(container.querySelector('figure.verevon-chat-chart')).toBeTruthy()
    })
    const wrap = container.querySelector('.verevon-chat-table-wrap')
    expect(wrap?.nextElementSibling?.matches('figure.verevon-chat-chart')).toBe(true)
    // K1..K3 are quarters: a line, with the switch offering bar and pie.
    expect(container.querySelector('figure')?.getAttribute('data-chart-type')).toBe('line')
    expect(screen.getByRole('button', { name: 'Skjul diagram' }).getAttribute('aria-pressed')).toBe('true')
    expect(container.querySelectorAll('svg[role="img"] g > path[fill="none"]')).toHaveLength(1)

    fireEvent.click(screen.getByRole('button', { name: 'Skjul diagram' }))
    await vi.waitFor(() => {
      expect(container.querySelector('figure.verevon-chat-chart')).toBeNull()
    })
  })

  it('switches a plain table to a pie from the chart itself', async () => {
    const { container } = render(() => (
      <MarkdownTable block={tableBlock({ header: ['Region', 'Salg'], rows: [['Nord', '50'], ['Sør', '30'], ['Vest', '20']] })} />
    ))
    fireEvent.click(screen.getByRole('button', { name: 'Vis som diagram' }))
    await vi.waitFor(() => {
      expect(container.querySelector('figure')?.getAttribute('data-chart-type')).toBe('bar')
    })
    fireEvent.click(screen.getByRole('button', { name: 'Kake' }))
    await vi.waitFor(() => {
      expect(container.querySelectorAll('path.verevon-chat-chart__slice')).toHaveLength(3)
    })
  })

  it('keeps a chart open while the message keeps streaming', async () => {
    const table = '| Region | Salg |\n| --- | --- |\n| Nord | 5 |\n| Sør | 7 |\n'
    const [content, setContent] = createSignal(`${table}\nVidere`)
    const { container } = render(() => <ChatMarkdown content={content()} />)
    fireEvent.click(screen.getByRole('button', { name: 'Vis som diagram' }))
    await vi.waitFor(() => {
      expect(container.querySelector('figure.verevon-chat-chart')).toBeTruthy()
    })
    setContent(`${table}\nVidere og videre`)
    flush()
    await vi.waitFor(() => {
      expect(container.textContent).toContain('Videre og videre')
    })
    expect(container.querySelector('figure.verevon-chat-chart')).toBeTruthy()
  })

  describe('copy and download', () => {
    it('copies the table as tab-separated text, cells as typed', async () => {
      const writeText = vi.fn(async () => undefined)
      vi.stubGlobal('navigator', { ...navigator, clipboard: { writeText } })
      render(() => <MarkdownTable block={tableBlock()} />)
      fireEvent.click(screen.getByRole('button', { name: 'Kopier' }))
      await vi.waitFor(() => expect(writeText).toHaveBeenCalledWith('Kvartal\tOmsetning\nK1\t1 200,5\nK2\t1 350'))
      await vi.waitFor(() => expect(screen.getByRole('button', { name: 'Kopiert' })).toBeTruthy())
    })

    it('says so when the clipboard refuses, instead of claiming a copy', async () => {
      const writeText = vi.fn(async () => {
        throw new Error('NotAllowedError')
      })
      vi.stubGlobal('navigator', { ...navigator, clipboard: { writeText } })
      render(() => <MarkdownTable block={tableBlock()} />)
      fireEvent.click(screen.getByRole('button', { name: 'Kopier' }))
      await vi.waitFor(() => expect(screen.getByRole('button', { name: 'Kunne ikke kopiere' })).toBeTruthy())
      expect(screen.queryByRole('button', { name: 'Kopiert' })).toBeNull()
    })

    describe('CSV', () => {
      let createObjectURL: ReturnType<typeof vi.fn>
      let downloads: string[]

      beforeEach(() => {
        createObjectURL = vi.fn(() => 'blob:csv')
        downloads = []
        vi.stubGlobal('URL', Object.assign(URL, { createObjectURL, revokeObjectURL: vi.fn() }))
        vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(function (this: HTMLAnchorElement) {
          downloads.push(this.download)
        })
      })

      it('saves UTF-8 with a byte order mark and semicolons for Norwegian', async () => {
        render(() => <MarkdownTable block={tableBlock({ rows: [['Tromsø', '1 200,5'], ['Ålesund', '1 350']] })} />)
        fireEvent.click(screen.getByRole('button', { name: 'Last ned CSV' }))
        await vi.waitFor(() => expect(createObjectURL).toHaveBeenCalledTimes(1))
        const blob = createObjectURL.mock.calls[0]?.[0] as Blob
        expect(blob.type).toBe('text/csv;charset=utf-8')
        const bytes = new Uint8Array(await blob.arrayBuffer())
        expect([...bytes.slice(0, 3)]).toEqual([0xef, 0xbb, 0xbf])
        const text = new TextDecoder().decode(bytes)
        expect(text).toBe('Kvartal;Omsetning\r\nTromsø;1 200,5\r\nÅlesund;1 350\r\n')
        expect(downloads).toEqual(['tabell.csv'])
      })
    })
  })
})
