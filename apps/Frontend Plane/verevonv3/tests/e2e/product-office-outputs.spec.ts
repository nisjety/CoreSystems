import { expect, test, type Page } from '@playwright/test'
import { resolve } from 'node:path'
import { composer, openFresh, restrictSources, send, type Transcript } from './product-acceptance-helpers'

/**
 * Chat gap-closure plan, workstream H (Office, data and code outputs), task H1:
 * an Excel workbook and a PowerPoint deck attached in /chat are read the way
 * Excel and PowerPoint store them, not as raw cell numbers and a flat pile of
 * text.
 *
 * The two files in tests/e2e/fixtures are written by Excel and PowerPoint
 * themselves (imports-core/tests/fixtures/real/make-real-fixtures.ps1) and hold
 * invented values only. What they contain, and what a faithful read must hand
 * the model:
 *
 *   regneark.xlsx   sheet Budsjett: A3 is the date 15.03.2024 (a bare serial,
 *                   45366, with a date style); B10 is =SUM(B3:B9) = 35000;
 *                   column D holds percentages; a merged title in A1; a hidden
 *                   sheet; a TRUE and a #DIV/0! on the sheet Sjekk.
 *   presentasjon.pptx
 *                   slide 2 has a table and the speaker notes "Husk å nevne at
 *                   Sør-regionen har ny leder fra mars."; slide 3 has a chart
 *                   (Omsetning: 100, 120, 140 over Q1 to Q3); slide 4 is hidden.
 *
 * Two tests, cheapest first:
 *
 *   1. Extraction through the gateway, with chat inference blocked. It reads
 *      back exactly what imports-core returned for each file and costs no model
 *      turn. It needs imports-api rebuilt from the H1 source, and is the first
 *      dated record of a real .xlsx and .pptx upload through the product.
 *   2. The same two files on the ChatGPT Terra subscription route (one turn).
 *      Like every product spec it never falls back to Verevon Balance or Claude.
 */

const WORKBOOK = resolve('tests/e2e/fixtures/regneark.xlsx')
const DECK = resolve('tests/e2e/fixtures/presentasjon.pptx')
const FILES = { 'regneark.xlsx': WORKBOOK, 'presentasjon.pptx': DECK }

/** Record what the gateway returns for every attached document. */
function recordExtractions(page: Page) {
  const extracted = new Map<string, { content: string; truncated?: boolean }>()
  page.on('response', async response => {
    if (!response.url().endsWith('/api/v1/chat/documents/extract') || response.request().method() !== 'POST') return
    const { filename } = response.request().postDataJSON() as { filename: string }
    if (!response.ok()) { extracted.set(filename, { content: `HTTP ${response.status()}` }); return }
    const body = await response.json() as { data?: { content?: string; truncated?: boolean } }
    extracted.set(filename, { content: body.data?.content ?? '', truncated: body.data?.truncated })
  })
  return extracted
}

async function attachBoth(page: Page) {
  await page.locator('input[type="file"]').first().setInputFiles(Object.values(FILES))
}

const assistantText = (value: Transcript | null) => value?.turns.filter(turn => turn.role === 'assistant').at(-1)?.content ?? ''

test('H1 extraction: a real workbook and a real deck are read as stored (no model turn)', async ({ page }, info) => {
  const extracted = recordExtractions(page)
  // Mocked inference: the stream request is aborted by the route guard, so this
  // test can never spend a Terra turn. Extraction runs at submit, before it.
  await openFresh(page, '/chat', { mockedInference: true })
  await attachBoth(page)
  await composer(page).fill('Les de to filene.')
  await page.locator('form button[type="submit"]').first().click()
  await expect.poll(() => extracted.size, { timeout: 30_000, message: 'both attachments should reach /chat/documents/extract' }).toBe(2)

  const workbook = extracted.get('regneark.xlsx')!
  const deck = extracted.get('presentasjon.pptx')!
  await info.attach('extracted-regneark.txt', { body: workbook.content, contentType: 'text/plain' })
  await info.attach('extracted-presentasjon.txt', { body: deck.content, contentType: 'text/plain' })

  // Workbook: row n is line n of its sheet, so A3 and B10 can be found by position.
  const lines = workbook.content.split('\n')
  expect(lines[0]).toBe('[Budsjett]')
  expect(lines[3]!.split('\t')[0], 'A3 is a date, not the serial 45366').toBe('2024-03-15')
  expect(lines[10]!.split('\t')[1], 'B10 is the sum').toBe('35000')
  expect(lines[3]).toContain('34.3%') // 0.342857… in a percent style
  expect(workbook.content).toContain('[Skjult] (skjult)')
  expect(workbook.content).toContain('Over grense?\tTRUE')
  expect(workbook.content).toContain('Feil\t#DIV/0!')
  expect(workbook.content).toContain('Støy\t0.3\n') // 0.30000000000000004 as Excel stores it
  expect(workbook.content).toContain('[Formler]\n')
  expect(workbook.content).toContain('Budsjett!B10: =SUM(B3:B9)')
  expect(workbook.content).not.toMatch(/\b45366\b/)
  expect(workbook.truncated).toBeFalsy()

  // Deck.
  expect(deck.content).toContain('[Slide 2]\nResultater\nRegion\tOmsetning\tVekst\nNord\t120\t5 %\nSør\t95\t3 %\n[Notater]\nHusk å nevne at Sør-regionen har ny leder fra mars.')
  expect(deck.content).toContain('[Diagram: Omsetning per kvartal] Omsetning: 100, 120, 140; kategorier: Q1, Q2, Q3')
  expect(deck.content).toContain('[Bilde: Firmalogo (test)]')
  expect(deck.content).toContain('[Slide 4] (skjult)')
  expect(deck.content).toMatch(/^\[Slide 1\]\nKvartalsrapport Q1\nIntern gjennomgang\n\n\[Slide 2\]/)
})

test('H1 live: Terra answers from the extracted workbook and speaker notes', async ({ page }, info) => {
  const extracted = recordExtractions(page)
  await openFresh(page)
  await restrictSources(page)
  await attachBoth(page)
  const result = await send(
    page,
    'Jeg har lagt ved et regneark og en presentasjon. Hvilken dato står i celle A3 i regnearket (arket Budsjett), hva er totalen i celle B10, og hva sier talernotatene på lysbilde 2? Svar kort, ett punkt per spørsmål.',
    info,
    'h1-office-read',
  )
  expect(extracted.size).toBe(2)
  const answer = assistantText(result)
  await info.attach('h1-answer.txt', { body: answer, contentType: 'text/plain' })

  // The values are known from the fixtures; a date may be written several ways.
  expect(answer, 'A3 is 15 March 2024').toMatch(/2024-03-15|15\.\s?0?3\.\s?2024|15\.\s?mars\s+2024|15\/0?3\/2024|March 15, 2024|15 March 2024/i)
  expect(answer, 'B10 is 35 000').toMatch(/\b35[\s  .]?000\b/)
  expect(answer, 'the speaker notes on slide 2').toMatch(/ny leder/i)
  expect(answer).toMatch(/mars/i)
  // The defect this task fixes: the raw serial leaking through as if it were the date.
  expect(answer).not.toMatch(/\b45[\s ]?366\b/)
})
