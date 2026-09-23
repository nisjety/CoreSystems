import { expect, type Page, type TestInfo } from '@playwright/test'
import { requireProductSubscription, PRODUCT_MODEL, PRODUCT_PROVIDER } from './product-subscription-route'

export const composer = (page: Page) => page.locator('form textarea').first()
export type Artifact = { id: string; content: string; version: number; title: string }
export type Turn = { role: string; content: string; status?: string; requestId?: string; artifacts?: Artifact[]; modelUsed?: string; provider?: string }
export type Transcript = { threadId: string; turns: Turn[] }

export async function openFresh(page: Page, path = '/chat', options: { mockedInference?: boolean } = {}) {
  await requireProductSubscription(page, options.mockedInference)
  await page.addInitScript(() => {
    if (sessionStorage.getItem('q01.fixture.initialized')) return
    sessionStorage.setItem('q01.fixture.initialized', 'yes')
    localStorage.setItem('verevon.locale', 'no')
    for (const key of ['verevon.chat.threadId', 'verevon.chat.threadHistory.v1', 'verevon.chat.threadTranscripts.v1', 'verevon.chat.pendingLaunch']) {
      localStorage.removeItem(key); sessionStorage.removeItem(key)
    }
  })
  await page.goto(path)
  expect(new URL(page.url()).pathname).toBe(new URL(path, 'http://localhost:5173').pathname)
  await expect(page).toHaveTitle(/verevon/i)
  await expect(composer(page)).toBeVisible()
  await expect(page.locator('vite-error-overlay')).toHaveCount(0)
  // The maintained fixture is attachment-grounded. Select the existing UI
  // control explicitly instead of inheriting an account's browsing preference.
  const browse = page.getByRole('button', { name: /^Søk på nett/ })
  if (await browse.isEnabled() && await browse.getAttribute('aria-pressed') === 'true') await browse.click()
  await expect(browse).toHaveAttribute('aria-pressed', 'false')
}

export async function transcript(page: Page): Promise<Transcript | null> {
  return page.evaluate(() => {
    const rows = JSON.parse(localStorage.getItem('verevon.chat.threadTranscripts.v1') || '[]') as Transcript[]
    return rows[0] ?? null
  })
}

export async function restrictSources(page: Page) {
  await page.getByRole('button', { name: /^Kontekst for dette svaret:/ }).click()
  await page.getByRole('combobox', { name: 'Kilder for samtalen' }).selectOption('conversation')
  await page.getByRole('button', { name: /^Kontekst for dette svaret:/ }).click()
  await expect(page.getByText('Kun samtalen', { exact: true })).toBeVisible()
}

export function latestArtifacts(value: Transcript | null): Artifact[] {
  const latest = new Map<string, Artifact>()
  for (const turn of value?.turns ?? []) for (const artifact of turn.artifacts ?? []) {
    if (artifact.version >= (latest.get(artifact.id)?.version ?? 0)) latest.set(artifact.id, artifact)
  }
  return [...latest.values()]
}

export async function send(page: Page, prompt: string, info: TestInfo, label: string, beforeSubmit?: () => Promise<void>) {
  await composer(page).fill(prompt)
  // A recording can hold the real, prepared instruction on screen before
  // submission. It must not add that editorial pause to task latency.
  await beforeSubmit?.()
  // DevTools response.text() can decode text/event-stream as Windows-1252
  // when the proxy omits an explicit charset. Capture with the browser's
  // Fetch UTF-8 decoder, matching the application, so Norwegian evidence and
  // offline word counts are not corrupted. This wrapper exists only in QA.
  await page.evaluate(() => {
    const target = window as typeof window & { productStream?: Promise<string>; restoreProductFetch?: () => void }
    target.restoreProductFetch?.()
    delete target.productStream
    const original = window.fetch.bind(window)
    window.fetch = async (...args: Parameters<typeof fetch>) => {
      const response = await original(...args)
      const input = args[0]
      const url = input instanceof Request ? input.url : String(input)
      if (new URL(url, location.href).pathname === '/api/v1/chat/stream') target.productStream = response.clone().text()
      return response
    }
    target.restoreProductFetch = () => { window.fetch = original }
  })
  await page.evaluate(() => {
    const target = window as typeof window & { q08Timing?: { stop: () => { acknowledgementMs: number | null; firstAnswerMs: number | null } } }
    target.q08Timing?.stop()
    const started = performance.now()
    const existing = new Set(document.querySelectorAll('.verevon-chat-message--assistant'))
    const users = document.querySelectorAll('.verevon-chat-message--user').length
    let acknowledgementMs: number | null = null; let firstAnswerMs: number | null = null
    const observer = new MutationObserver(() => {
      if (acknowledgementMs === null && document.querySelectorAll('.verevon-chat-message--user').length > users) acknowledgementMs = performance.now() - started
      const last = Array.from(document.querySelectorAll('.verevon-chat-message--assistant')).at(-1)
      if (firstAnswerMs === null && last && !existing.has(last) && last.querySelector('.verevon-chat-markdown')?.textContent?.trim()) firstAnswerMs = performance.now() - started
    })
    observer.observe(document.body, { childList: true, subtree: true, characterData: true })
    target.q08Timing = { stop: () => { observer.disconnect(); return { acknowledgementMs, firstAnswerMs } } }
  })
  const started = Date.now()
  const responsePending = page.waitForResponse(r => r.url().endsWith('/api/v1/chat/stream') && r.request().method() === 'POST', { timeout: 60_000 })
  await page.locator('form button[type="submit"]').first().click()
  const response = await responsePending
  const headersMs = Date.now() - started
  expect(response.status()).toBe(200)
  await response.finished()
  const stream = await page.evaluate(async () => {
    const target = window as typeof window & { productStream?: Promise<string>; restoreProductFetch?: () => void }
    try {
      if (!target.productStream) throw new Error('The browser did not capture the chat response')
      return await target.productStream
    } finally { target.restoreProductFetch?.() }
  })
  await info.attach(`${label}-stream`, { body: stream, contentType: 'text/plain' })
  const rendered = await page.evaluate(() => (window as typeof window & { q08Timing?: { stop: () => object } }).q08Timing?.stop())
  await info.attach(`${label}-timing`, { body: JSON.stringify({ elapsedMs: Date.now() - started, headersMs, ...rendered }), contentType: 'application/json' })
  await info.attach(`${label}-observed-transcript`, { body: JSON.stringify(await transcript(page)), contentType: 'application/json' })
  const frames = stream.split(/\r?\n\r?\n/).flatMap(frame => {
    const event = /^event: (.+)$/m.exec(frame)?.[1]
    const raw = /^data: (.+)$/m.exec(frame)?.[1]
    return event && raw ? [{ event, data: JSON.parse(raw) }] : []
  })
  const textArtifacts = frames.filter(frame => frame.event === 'artifact' && ['document', 'code', 'html'].includes(frame.data.kind))
  if (frames.some(frame => frame.event === 'error')) {
    expect(textArtifacts, 'failed turns must not expose intermediate artifact versions').toEqual([])
  }
  expect(new Set(textArtifacts.map(frame => frame.data.id)).size, 'only the selected final version may be published per artifact').toBe(textArtifacts.length)
  for (const frame of frames.filter(frame => frame.event === 'tool_call' && ['create_artifact', 'update_artifact'].includes(frame.data.name))) {
    expect(frame.data.args?.content, 'private draft bodies must not leak through tool progress').toBeUndefined()
  }
  for (const frame of frames.filter(frame => frame.event === 'tool_call' && frame.data.name === 'count_words')) {
    expect(frame.data.args?.texts, 'unchecked draft bodies must not leak through word-count progress').toBeUndefined()
  }
  expect(stream).not.toMatch(/^event: error\s*$/m)
  const request = response.request().postDataJSON() as { features?: string[]; model?: string; provider?: string; subscription_connection_id?: string }
  expect(request.model).toBe(PRODUCT_MODEL)
  expect(request.provider).toBe(PRODUCT_PROVIDER)
  expect(Boolean(request.subscription_connection_id)).toBe(true)
  for (const frame of frames.filter(frame => frame.event === 'done')) expect(frame.data.model_used).toBe(PRODUCT_MODEL)
  const checkReviewRoutes = (value: unknown): void => {
    if (Array.isArray(value)) { value.forEach(checkReviewRoutes); return }
    if (!value || typeof value !== 'object') return
    for (const [key, entry] of Object.entries(value)) {
      if (key === 'reviewRoutes' && Array.isArray(entry)) {
        for (const route of entry) expect(route).toMatchObject({ model: PRODUCT_MODEL, provider: PRODUCT_PROVIDER })
      } else checkReviewRoutes(entry)
    }
  }
  for (const frame of frames.filter(frame => frame.event === 'result_receipt')) checkReviewRoutes(frame.data)

  if (request.features?.includes('conversation_only')) {
    expect(stream).not.toMatch(/^event: memory_recall\s*$/m)
    for (const frame of stream.split(/\r?\n\r?\n/)) {
      if (/^event: tool_call$/m.test(frame)) {
        const data = JSON.parse(/^data: (.+)$/m.exec(frame)![1])
        expect(['create_artifact', 'read_artifact', 'update_artifact', 'count_words', 'code_interpreter', 'reattach_context']).toContain(data.name)
      }
    }
  }
  const toolEvents = stream.split(/\r?\n\r?\n/).flatMap(frame => {
    const name = /^event: (tool_call|tool_result)$/m.exec(frame)?.[1]
    const raw = /^data: (.+)$/m.exec(frame)?.[1]
    return name && raw ? [{ name, data: JSON.parse(raw) as { name?: string; status?: string; error?: string } }] : []
  })
  expect(toolEvents.filter(e => e.name === 'tool_result' && (e.data.error || e.data.status === 'error')), 'tool failures must be fixed before a recording').toEqual([])
  for (const event of toolEvents.filter(e => e.name === 'tool_call')) {
    expect(event.data.name).not.toMatch(/send|book|publish|order.*(update|create)|action_execute/)
  }
  await expect.poll(async () => (await transcript(page))?.turns.at(-1)?.status, { timeout: 15_000 }).not.toBe('waiting')
  await expect(page.locator('.verevon-chat-message--assistant').last()).toBeVisible()
  return transcript(page)
}
