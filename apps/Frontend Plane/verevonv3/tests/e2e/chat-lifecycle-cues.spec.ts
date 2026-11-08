import { expect, test, type Page } from '@playwright/test'
import { STREAM_END_MARKER, buildStreamFrames, installPerf, shimConfig, type PerfShimConfig } from './chat-perf-fixtures'

/**
 * Run lifecycle cues in the browser (WS15 S5). The chat endpoints are answered inside the page by the fetch shim from
 * `chat-perf-fixtures.ts`, so the stream is a script of our own: a tool call that is retried, a compaction, a reduced
 * answer, then the answer. What is checked:
 *
 *  - each notice renders as one note, with the sentence the transcript and the announcement both use;
 *  - the live region says each notice once, and never the same sentence twice;
 *  - a plain stream renders no note and says none of them.
 *
 * It needs the signed-in dev stack to render `/chat`, as the other chat specs do. Run it with
 *
 *   pnpm test:e2e -- chat-lifecycle-cues
 */

const ANSWER_END_MARKER = 'Lifecycle-fixture slutt.'
const NOTE = '.verevon-chat-lifecycle-notice'
const COMPOSER = 'form textarea'
const LIVE_REGION = 'div.sr-only[role="status"][aria-live="polite"]'
const SEEN_KEY = '__lifecycleSeen'

const RETRY_SENTENCE = 'Prøver Web search på nytt etter tidsavbrudd (forsøk 2 av 3).'
const COMPACTION_SENTENCE = 'Eldre meldinger ble oppsummert for å få plass i konteksten.'
const REDUCED_SENTENCE =
  'Svaret bygger bare på tekstsøk i kildene, fordi semantisk søk ikke er tilgjengelig under Zero Data Retention.'

/** One SSE frame, serialised as the gateway writes it. */
const frame = (id: number, event: string, data: object) =>
  `id: ${id}\nevent: ${event}\ndata: ${JSON.stringify(data)}\n\n`

/** A turn whose web search is retried once, then compacted, answered in reduced form, and finished. */
function lifecycleFrames(): string[] {
  const frames = [frame(1, 'connected', { ok: true, request_id: 'lc-req-1', thread_id: 'lc-thread' })]
  frames.push(frame(2, 'tool_call', { id: 'lc-tool-1', name: 'web_search', args: {} }))
  frames.push(frame(3, 'retry_scheduled', {
    scope: 'tool', tool: 'web_search', attempt: 2, max_attempts: 3, wait_ms: 500, reason: 'timeout',
  }))
  frames.push(frame(4, 'tool_result', { id: 'lc-tool-1', output: '{}', status: 'done' }))
  frames.push(frame(5, 'compaction', { scope: 'history', outcome: 'summarized', messages_compacted: 8, messages_kept: 4 }))
  frames.push(frame(6, 'retrieval_notice', { code: 'zdr_dense_unavailable', wording_key: 'retrieval.zdr_dense_unavailable' }))
  const words = ['Leveransene', 'ble', 'forsinket', 'to', 'dager.', ANSWER_END_MARKER]
  words.forEach((word, index) => frames.push(frame(7 + index, 'chunk', { delta: `${word} ` })))
  frames.push(frame(7 + words.length, 'done', { request_id: 'lc-req-1', done: true, model_used: 'lifecycle-fixture' }))
  return frames
}

function shim(streamFrames: string[]): PerfShimConfig {
  return { ...shimConfig('empty'), streamFrames, frameIntervalMs: 40 }
}

/** Records every text the polite live region holds, from before the send to the end of the stream. */
async function recordLiveRegion(page: Page) {
  await page.evaluate(
    ([region, key]) => {
      const element = document.querySelector<HTMLElement>(region)
      if (!element) throw new Error('the polite live region is not on the page')
      const seen: string[] = []
      ;(window as unknown as Record<string, string[]>)[key] = seen
      new MutationObserver(() => seen.push(element.textContent ?? '')).observe(element, {
        childList: true,
        characterData: true,
        subtree: true,
      })
    },
    [LIVE_REGION, SEEN_KEY] as const,
  )
}

async function announcements(page: Page): Promise<string[]> {
  return page.evaluate((key) => (window as unknown as Record<string, string[]>)[key] ?? [], SEEN_KEY)
}

async function openChat(page: Page, config: PerfShimConfig) {
  await installPerf(page, config, { selectThread: false })
  await page.goto('/chat')
  await expect(page.locator(COMPOSER)).toBeVisible()
  await recordLiveRegion(page)
}

async function sendMessage(page: Page) {
  const composer = page.locator(COMPOSER)
  await composer.click()
  await composer.fill('Hva skjedde med leveransene i uke 42?')
  await page.locator('form button[type="submit"]').click()
}

test.describe('chat lifecycle cues (WS15 S5)', () => {
  test('a retried, compacted and reduced turn shows one note per notice, announced once each', async ({ page }) => {
    await openChat(page, shim(lifecycleFrames()))
    await sendMessage(page)
    await expect(page.locator('.verevon-chat-message--assistant').last()).toContainText(ANSWER_END_MARKER)

    await expect(page.locator(NOTE)).toHaveCount(3)
    await expect(page.locator(NOTE)).toHaveText([RETRY_SENTENCE, COMPACTION_SENTENCE, REDUCED_SENTENCE])
    await expect(page.locator(NOTE).first()).toHaveAttribute('role', 'note')

    const seen = await announcements(page)
    for (const sentence of [RETRY_SENTENCE, COMPACTION_SENTENCE, REDUCED_SENTENCE]) {
      expect(seen.filter((text) => text.trim() === sentence), sentence).toHaveLength(1)
    }
  })

  test('a plain stream renders no note and announces none', async ({ page }) => {
    await openChat(page, shim(buildStreamFrames('perf-thread-empty')))
    await sendMessage(page)
    await expect(page.locator('.verevon-chat-message--assistant').last()).toContainText(STREAM_END_MARKER)

    await expect(page.locator(NOTE)).toHaveCount(0)
    const seen = await announcements(page)
    for (const sentence of [RETRY_SENTENCE, COMPACTION_SENTENCE, REDUCED_SENTENCE]) {
      expect(seen.some((text) => text.includes(sentence))).toBe(false)
    }
  })
})
