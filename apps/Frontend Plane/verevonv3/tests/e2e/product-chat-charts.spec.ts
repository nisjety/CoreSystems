import { expect, test, type Locator, type Page, type TestInfo } from '@playwright/test'
import { readFileSync } from 'node:fs'
import { PRODUCT_MODEL, PRODUCT_PROVIDER } from './product-subscription-route'
import { composer, openFresh, send } from './product-acceptance-helpers'

/**
 * Chat gap-closure plan, workstream H (Office, data and code outputs), task H2:
 * charts drawn natively in the chat. Two tests, cheapest first:
 *
 *   1. "H2 offline": the real chat page, with the chat stream answered by a
 *      fixture (inference blocked, no model turn, nothing billed). A closed
 *      ```chart fence is drawn as an SVG, a plain table gets its toolbar, and
 *      the toolbar's copy, CSV and "Vis som diagram" work in a real browser.
 *
 *        cd "apps/Frontend Plane/verevonv3"
 *        npx playwright test --project=product-readiness tests/e2e/product-chat-charts.spec.ts --grep "H2 offline"
 *
 *   2. "H2 live on Terra": two turns on the ChatGPT Terra subscription route
 *      (gpt-5.6-terra, openai-codex-subscription), text only, no tools. Like
 *      every product spec it never falls back to Verevon Balance or Claude, and
 *      a usage limit stops it with an explicit message.
 *        turn 1 asks for a table and a bar chart. The model has to write the
 *        ```chart fence the steering notice describes; the page has to draw it.
 *        turn 2 asks for a plain table, with no chart words. No chart may be
 *        drawn (the notice must not reach an ordinary turn), and "Vis som
 *        diagram" on that table opens one.
 *
 *        npx playwright test --project=product-readiness tests/e2e/product-chat-charts.spec.ts --grep "H2 live"
 *
 * Pass criteria of the live test, all of them:
 *   - turn 1: exactly one <svg role="img"> chart in the answer with 8 bars
 *     (4 quarters x 2 years, or 8 quarters x 1 series), its title, description
 *     and a hidden data table whose cell count equals the bar count; the raw
 *     fence is not shown as code; the markdown table the prompt asked for is
 *     still there; no error frame.
 *   - turn 2: no chart in the answer until the button is pressed; then one chart
 *     with one bar per region and a data table with the same values.
 *   - the stream request keeps the selected model and provider.
 * Needs the frontend to serve this checkout's source: the frontend container has
 * no hot reload, so restart it (or use a host Vite dev server) first.
 */

const frame = (event: string, data: object, id: number) => `id: ${id}\nevent: ${event}\ndata: ${JSON.stringify(data)}\n\n`

const CHART_PROMPT = 'Lag en tabell med omsetning per kvartal for 2025 og 2026 (fiktive tall) og vis den som søylediagram'
const PLAIN_PROMPT = 'Lag en tabell med antall kunder per region (fiktive tall): Nord, Midt, Vest og Sør.'

const assistant = (page: Page) => page.locator('.verevon-chat-message--assistant').last()
const charts = (scope: Locator) => scope.locator('figure.verevon-chat-chart')
const bars = (chart: Locator) => chart.locator('svg.verevon-chat-chart__svg g > path')

const FIXTURE_FENCE = JSON.stringify({
  type: 'bar',
  title: 'Omsetning per kvartal (MNOK)',
  x: ['K1', 'K2', 'K3', 'K4'],
  series: [
    { name: '2025', values: [10.2, 12.4, 9.8, 14.1] },
    { name: '2026', values: [11.5, 13.2, 12.6, 16.3] },
  ],
  unit: 'MNOK',
})

const FIXTURE_ANSWER = [
  'Her er omsetningen per kvartal (fiktive tall).',
  '',
  '```chart',
  FIXTURE_FENCE,
  '```',
  '',
  '| Kvartal | 2025 | 2026 |',
  '| --- | ---: | ---: |',
  '| K1 | 10,2 | 11,5 |',
  '| K2 | 12,4 | 13,2 |',
  '| K3 | 9,8 | 12,6 |',
  '| K4 | 14,1 | 16,3 |',
  '',
  'Kunder per region:',
  '',
  '| Region | Kunder |',
  '| --- | ---: |',
  '| Nord | 120 |',
  '| Midt | 80 |',
  '| Vest | 150 |',
  '| Sør | 95 |',
].join('\n')

/** The visible numbers of a chart's data table, row by row. */
async function dataTable(chart: Locator): Promise<{ header: string[]; rows: string[][] }> {
  const table = chart.locator('table.verevon-chat-chart__table')
  const header = await table.locator('thead th').allTextContents()
  const rows = await table.locator('tbody tr').evaluateAll(trs => trs.map(tr => Array.from(tr.querySelectorAll('th, td')).map(cell => (cell.textContent ?? '').replace(/[  ]/g, ' ').trim())))
  return { header, rows }
}

test('H2 offline: a chart fence and a plain table are drawn and tooled in the real chat page (no model turn)', async ({ page }, info) => {
  await openFresh(page, '/chat', { mockedInference: true })
  let invocations = 0
  await page.route('**/api/v1/chat/stream', route => {
    const body = route.request().postDataJSON() as Record<string, unknown>
    expect(body).toMatchObject({ model: PRODUCT_MODEL, provider: PRODUCT_PROVIDER })
    invocations++
    // The answer arrives in several chunks, as a stream does.
    const pieces = FIXTURE_ANSWER.match(/[\s\S]{1,120}/g) ?? []
    return route.fulfill({ status: 200, contentType: 'text/event-stream', body:
      frame('connected', { request_id: 'h2-offline', ok: true }, 1)
      + pieces.map((content, index) => frame('chunk', { content }, index + 2)).join('')
      + frame('done', { request_id: 'h2-offline', done: true, model_used: PRODUCT_MODEL }, pieces.length + 2) })
  })

  await composer(page).fill(CHART_PROMPT)
  await page.locator('form button[type="submit"]').first().click()
  const answer = assistant(page)

  // The fence is a native chart, not a code block.
  const chart = charts(answer).first()
  await expect(chart.locator('svg[role="img"]')).toBeVisible({ timeout: 30_000 })
  await expect(answer.locator('.verevon-chat-codeblock')).toHaveCount(0)
  await expect(bars(chart)).toHaveCount(8)
  const svg = chart.locator('svg[role="img"]')
  const titleId = await svg.getAttribute('aria-labelledby')
  expect(await page.locator(`[id="${titleId}"]`).first().textContent()).toBe('Omsetning per kvartal (MNOK)')
  expect(await svg.getAttribute('aria-describedby')).toBeTruthy()

  // The data table is in the page, hidden until asked for, and holds every value.
  const hidden = chart.locator('table.verevon-chat-chart__table')
  await expect(hidden).not.toHaveClass(/verevon-chat-chart__table--visible/)
  const data = await dataTable(chart)
  expect(data.header).toEqual(['Kategori', '2025', '2026'])
  expect(data.rows).toEqual([
    ['K1', '10,2 MNOK', '11,5 MNOK'],
    ['K2', '12,4 MNOK', '13,2 MNOK'],
    ['K3', '9,8 MNOK', '12,6 MNOK'],
    ['K4', '14,1 MNOK', '16,3 MNOK'],
  ])
  await chart.getByRole('button', { name: 'Vis tabell' }).click()
  await expect(hidden).toHaveClass(/verevon-chat-chart__table--visible/)
  await expect(hidden).toBeVisible()
  await chart.getByRole('button', { name: 'Skjul tabell' }).click()

  // Keyboard: one tab stop, arrow keys move, the tooltip follows focus.
  const hits = chart.locator('[data-chart-hit]')
  await expect(hits).toHaveCount(4)
  await hits.first().focus()
  await page.keyboard.press('ArrowRight')
  await expect(hits.nth(1)).toBeFocused()
  await expect(chart.locator('.verevon-chat-chart__tooltip')).toContainText('K2')
  expect(await hits.nth(1).getAttribute('aria-label')).toMatch(/^K2: 2025 12,4.MNOK, 2026 13,2.MNOK$/)
  await page.screenshot({ path: info.outputPath('h2-chart-focus.png'), animations: 'disabled' })

  // Type switch and SVG download.
  await chart.getByRole('button', { name: 'Linje' }).click()
  await expect(chart).toHaveAttribute('data-chart-type', 'line')
  await expect(chart.locator('svg.verevon-chat-chart__svg g > path[fill="none"]')).toHaveCount(2)
  await chart.getByRole('button', { name: 'Søyle' }).click()
  const svgDownload = page.waitForEvent('download')
  await chart.getByRole('button', { name: 'Last ned SVG' }).click()
  const savedSvg = await svgDownload
  expect(savedSvg.suggestedFilename()).toBe('omsetning-per-kvartal-mnok.svg')
  const svgText = readFileSync(await savedSvg.path() ?? '', 'utf8')
  expect(svgText.startsWith('<?xml')).toBe(true)
  expect(svgText).toContain('xmlns="http://www.w3.org/2000/svg"')
  expect(svgText).not.toContain('data-chart-hit')
  expect(svgText, 'colours are written into the file, not left as CSS variables').not.toMatch(/var\(--/)
  await chart.screenshot({ path: info.outputPath('h2-chart-bar.png'), animations: 'disabled' })

  // The tables: toolbar on hover and focus only, the table itself unchanged.
  const tables = answer.locator('.verevon-chat-table-wrap')
  await expect(tables).toHaveCount(2)
  const quarters = tables.nth(0)
  const toolbar = quarters.locator('.verevon-chat-table-tools__bar')
  await expect(toolbar).toHaveCSS('opacity', '0')
  await quarters.hover()
  await expect(toolbar).toHaveCSS('opacity', '1')
  await expect(quarters.locator('tbody tr')).toHaveCount(4)

  const regions = tables.nth(1)
  await regions.hover()
  await regions.getByRole('button', { name: 'Vis som diagram' }).click()
  const regionChart = answer.locator('figure.verevon-chat-chart').nth(1)
  await expect(regionChart).toBeVisible()
  await expect(bars(regionChart)).toHaveCount(4)
  await expect(regionChart).toHaveAttribute('data-chart-type', 'bar')
  expect((await dataTable(regionChart)).rows).toEqual([['Nord', '120'], ['Midt', '80'], ['Vest', '150'], ['Sør', '95']])
  await regionChart.getByRole('button', { name: 'Kake' }).click()
  await expect(regionChart.locator('path.verevon-chat-chart__slice')).toHaveCount(4)
  await regionChart.screenshot({ path: info.outputPath('h2-table-pie.png'), animations: 'disabled' })

  // Copy and CSV.
  await page.context().grantPermissions(['clipboard-read', 'clipboard-write'], { origin: new URL(page.url()).origin })
  await regions.hover()
  await regions.getByRole('button', { name: 'Kopier' }).click()
  await expect(regions.getByRole('button', { name: 'Kopiert' })).toBeVisible()
  // Chromium on Windows hands the clipboard text back with CRLF line ends.
  const clipboard = (await page.evaluate(() => navigator.clipboard.readText())).replace(/\r\n/g, '\n')
  expect(clipboard).toBe('Region\tKunder\nNord\t120\nMidt\t80\nVest\t150\nSør\t95')
  const csvDownload = page.waitForEvent('download')
  await regions.hover()
  await regions.getByRole('button', { name: 'Last ned CSV' }).click()
  const csv = await csvDownload
  expect(csv.suggestedFilename()).toBe('tabell.csv')
  const bytes = readFileSync(await csv.path() ?? '')
  expect([...bytes.subarray(0, 3)]).toEqual([0xef, 0xbb, 0xbf])
  expect(bytes.subarray(3).toString('utf8')).toBe('Region;Kunder\r\nNord;120\r\nMidt;80\r\nVest;150\r\nSør;95\r\n')
  expect(invocations).toBe(1)
  await page.screenshot({ path: info.outputPath('h2-answer.png'), animations: 'disabled', fullPage: false })
})

/** Counts only. The answer text is never stored or printed. */
async function attachCounts(info: TestInfo, name: string, value: Record<string, number | boolean | string[]>) {
  await info.attach(name, { body: JSON.stringify(value), contentType: 'application/json' })
}

/** A usage limit is recorded as one, never worked around with another model. */
async function sendOnTerra(page: Page, prompt: string, info: TestInfo, label: string) {
  try {
    await send(page, prompt, info, label)
  } catch (error) {
    const stream = info.attachments.find(attachment => attachment.name === `${label}-stream`)?.body?.toString('utf8') ?? ''
    if (/subscription_usage_limit/.test(stream)) {
      throw new Error('STOP: the ChatGPT subscription is at its usage limit. Record it and run again after the reset; no other model was used.')
    }
    throw error
  }
}

test('H2 live on Terra: a model-written chart fence is drawn, an ordinary table stays plain until "Vis som diagram"', async ({ page }, info) => {
  await openFresh(page)

  // Turn 1: a table and a bar chart.
  await sendOnTerra(page, CHART_PROMPT, info, 'h2-live-chart')
  const first = assistant(page)
  await expect(charts(first)).toHaveCount(1, { timeout: 30_000 })
  const chart = charts(first).first()
  await expect(chart.locator('svg[role="img"]')).toBeVisible()
  const titleId = await chart.locator('svg[role="img"]').getAttribute('aria-labelledby')
  expect((await page.locator(`[id="${titleId}"]`).first().textContent())?.trim().length ?? 0).toBeGreaterThan(0)
  await expect(first.locator('.verevon-chat-codeblock'), 'the chart fence must not be shown as code').toHaveCount(0)
  await expect(bars(chart)).toHaveCount(8)
  const live = await dataTable(chart)
  const cells = live.rows.flatMap(row => row.slice(1))
  expect(cells.length, 'every bar has a cell in the data table').toBe(8)
  for (const cell of cells) expect(cell).toMatch(/\d/)
  await expect(first.locator('table.verevon-chat-table'), 'the markdown table the prompt asked for').toHaveCount(1)
  await attachCounts(info, 'h2-live-chart-counts', { bars: 8, categories: live.rows.length, series: live.header.length - 1, tableCells: cells.length })
  await chart.screenshot({ path: info.outputPath('h2-live-chart.png'), animations: 'disabled' })

  // Turn 2: an ordinary table. No chart until the reader asks.
  await sendOnTerra(page, PLAIN_PROMPT, info, 'h2-live-plain')
  const second = assistant(page)
  const wrap = second.locator('.verevon-chat-table-wrap').first()
  await expect(wrap).toBeVisible({ timeout: 30_000 })
  await expect(charts(second), 'an ordinary turn must not produce a chart').toHaveCount(0)
  await expect(second.locator('.verevon-chat-codeblock')).toHaveCount(0)
  await wrap.hover()
  await wrap.getByRole('button', { name: 'Vis som diagram' }).click()
  const opened = charts(second).first()
  await expect(opened).toBeVisible()
  const rows = await wrap.locator('tbody tr').count()
  expect(rows).toBeGreaterThanOrEqual(4)
  await expect(bars(opened)).toHaveCount(rows)
  const plain = await dataTable(opened)
  expect(plain.rows).toHaveLength(rows)
  await attachCounts(info, 'h2-live-plain-counts', { rows, bars: rows, chartsBefore: 0 })
  await opened.screenshot({ path: info.outputPath('h2-live-table-chart.png'), animations: 'disabled' })
})
