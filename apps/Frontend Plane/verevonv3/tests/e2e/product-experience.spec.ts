import { expect, test, type Page } from '@playwright/test'
import { PRODUCT_MODEL, PRODUCT_PROVIDER } from './product-subscription-route'
import { composer, openFresh, restrictSources, transcript, send, latestArtifacts } from './product-acceptance-helpers'

const assistant = (page: Page) => page.locator('.verevon-chat-message--assistant').last()
const stopButton = (page: Page) => page.locator('form').getByRole('button', { name: 'Stopp svar', exact: true })
const frame = (event: string, data: object, id: number) => `id: ${id}\nevent: ${event}\ndata: ${JSON.stringify(data)}\n\n`

test('Q13 stopping during source review publishes no artifact candidate', async ({ page }, info) => {
  await openFresh(page)
  await restrictSources(page)
  await page.locator('input[type="file"]').first().setInputFiles({ name: 'status.md', mimeType: 'text/markdown',
    buffer: Buffer.from('Fiktiv status: Team Nord har seks medarbeidere. Eva eier planen. Pilotdato er ikke bekreftet. Ingen invitasjon er sendt. Teknisk avklaring må være ferdig før pilotdatoen bekreftes. Det finnes ingen godkjent budsjettramme.') })
  await composer(page).fill('Lag et dokument med en kort status, risiko og neste anbefalte steg. Bruk bare vedlegget og bevar all usikkerhet. Ikke send noe.')
  await page.locator('form button[type="submit"]').first().click()
  await expect(page.getByText('Kontrollerer utkastet mot kildene', { exact: true }).first()).toBeAttached({ timeout: 90_000 })
  expect(latestArtifacts(await transcript(page))).toHaveLength(0)
  const cancelled = page.waitForResponse(response => response.url().includes('/cancel') && response.request().method() === 'POST')
  await stopButton(page).click()
  expect((await cancelled).ok()).toBe(true)
  await expect(assistant(page)).toContainText('Stoppet')
  expect(latestArtifacts(await transcript(page))).toHaveLength(0)
  await expect(page.locator('.verevon-chat-steps-header p')).toHaveText('Stoppet')
  await expect(composer(page)).toBeEnabled()
  await page.screenshot({ path: info.outputPath('source-review-stopped.png'), animations: 'disabled' })
  const stopped = await transcript(page)
  expect(stopped?.threadId).toBeTruthy()
  // A local snapshot alone cannot prove that a cancelled draft stayed private.
  // Clear it and hydrate this same conversation from the server.
  await page.evaluate(() => {
    localStorage.removeItem('verevon.chat.threadTranscripts.v1')
    sessionStorage.removeItem('verevon.chat.threadTranscripts.v1')
  })
  await page.reload()
  await expect(composer(page)).toBeVisible()
  await expect.poll(async () => (await transcript(page))?.threadId, { timeout: 20_000 }).toBe(stopped!.threadId)
  await expect(assistant(page)).toContainText('Stoppet')
  const reloaded = await transcript(page)
  expect(reloaded?.turns.some(turn => turn.role === 'user')).toBe(true)
  expect(latestArtifacts(reloaded)).toHaveLength(0)
  await expect(composer(page)).toBeEnabled()
  await info.attach('stopped-reloaded-transcript', { body: JSON.stringify(reloaded, null, 2), contentType: 'application/json' })
  await page.screenshot({ path: info.outputPath('source-review-stopped-reloaded.png'), animations: 'disabled' })
})

// Transport fixtures deliberately exercise failure semantics, not provider quality.
test('Q11 live checked answer can be stopped before text is accepted', async ({ page }, info) => {
  await openFresh(page)
  await restrictSources(page)
  await composer(page).fill('Svar direkte i chatten uten verktøy: skriv 100–120 ord om hvordan man kan holde et skrivebord ryddig. Bruk tre korte avsnitt.')
  await page.locator('form button[type="submit"]').first().click()
  await expect.poll(async () => (await transcript(page))?.turns.at(-1)?.requestId, { timeout: 30_000 }).toBeTruthy()
  expect((await transcript(page))?.turns.at(-1)?.content).toBe('')
  const cancelled = page.waitForResponse(response => response.url().includes('/cancel') && response.request().method() === 'POST')
  await stopButton(page).click()
  expect((await cancelled).ok()).toBe(true)
  await expect(assistant(page)).toContainText('Stoppet')
  expect((await transcript(page))?.turns.at(-1)?.content).toBe('')
  await expect(composer(page)).toBeEnabled()
  await page.screenshot({ path: info.outputPath('checked-answer-stopped.png'), animations: 'disabled' })
})

test('Q09 a delayed subscription check keeps the draft and enables one deliberate send', async ({ page }) => {
  await openFresh(page, '/chat', { mockedInference: true })
  let releaseConnection!: () => void
  const lookup = new Promise<void>(resolve => { releaseConnection = resolve })
  await page.route('**/api/v1/integrations/connections', async route => {
    await lookup
    await route.fallback()
  })
  let invocations = 0
  await page.route('**/api/v1/chat/stream', route => {
    expect(route.request().postDataJSON()).toMatchObject({ model: PRODUCT_MODEL, provider: PRODUCT_PROVIDER })
    invocations++
    return route.fulfill({ status: 200, contentType: 'text/event-stream', body:
      frame('connected', { request_id: 'q09-subscription-ready', ok: true }, 1) +
      frame('chunk', { content: 'Svar fra den valgte testtransporten.' }, 2) +
      frame('done', { request_id: 'q09-subscription-ready', done: true, model_used: PRODUCT_MODEL }, 3) })
  })
  try {
    await page.reload()
    await composer(page).fill('Behold dette mens abonnementet kontrolleres.')
    await expect(page.locator('form button[type="submit"]').first()).toBeDisabled()
    await expect(page.getByRole('status').filter({ hasText: 'Kontrollerer ChatGPT-abonnementet' })).toBeVisible()
    await composer(page).press('Enter')
    expect(invocations).toBe(0)
    await expect(composer(page)).toHaveValue('Behold dette mens abonnementet kontrolleres.')
    await expect(page.getByRole('alert')).toHaveCount(0)
  } finally { releaseConnection() }
  await expect(page.locator('form button[type="submit"]').first()).toBeEnabled()
  expect(invocations).toBe(0)
  await page.locator('form button[type="submit"]').first().click()
  await expect(assistant(page)).toContainText('Svar fra den valgte testtransporten.')
  await expect(page.getByRole('status').filter({ hasText: 'Svar fullført.' })).toBeVisible()
  expect(invocations).toBe(1)
})

test('Q11 failed validation stays actionable and never silently submits again', async ({ page }, info) => {
  await openFresh(page, '/chat', { mockedInference: true })
  let invocations = 0
  await page.route('**/api/v1/chat/stream', route => {
    expect(route.request().postDataJSON()).toMatchObject({ model: PRODUCT_MODEL, provider: PRODUCT_PROVIDER })
    invocations++
    return route.fulfill({ status: 200, contentType: 'text/event-stream', body:
      frame('connected', { request_id: 'q11-rejected-answer', ok: true }, 1) +
      frame('error', { code: 'response_validation_failed', message: 'Svaret oppfylte ikke ordgrensen etter flere forsøk. Prøv igjen.', retryable: true }, 2) })
  })
  await composer(page).fill('Svar direkte i chatten med 100–120 ord.')
  await page.locator('form button[type="submit"]').first().click()
  await expect(assistant(page)).toContainText('oppfylte ikke ordgrensen')
  await expect(assistant(page).locator('.verevon-chat-error-notice')).toHaveCount(1)
  await expect(page.locator('.verevon-chat-error[role="alert"]')).toHaveCount(0)
  await expect(assistant(page)).toBeFocused()
  await expect.poll(async () => (await transcript(page))?.turns.at(-1)?.status).toBe('error')
  await expect(page.getByRole('status').filter({ hasText: 'Svaret mislyktes.' })).toBeVisible()
  await expect(page.getByRole('status').filter({ hasText: 'Svar fullført.' })).toHaveCount(0)
  await expect(stopButton(page)).toBeHidden()
  await expect(composer(page)).toBeEnabled()
  expect(invocations).toBe(1)
  await page.screenshot({ path: info.outputPath('validation-failed.png'), animations: 'disabled' })
})

test('Q08 truncated transport preserves partial text and never claims completion', async ({ page }, info) => {
  await openFresh(page, '/chat', { mockedInference: true })
  let invocations = 0
  await page.route('**/api/v1/chat/stream', route => {
    expect(route.request().postDataJSON()).toMatchObject({ model: PRODUCT_MODEL, provider: PRODUCT_PROVIDER })
    invocations++
    return route.fulfill({ status: 200, contentType: 'text/event-stream', body:
      frame('connected', { request_id: 'q08-expired-buffer', ok: true }, 1) +
      frame('chunk', { content: 'Dette er et ufullstendig svar fra en avbrutt forbindelse.' }, 2) })
  })
  await page.route('**/api/v1/chat/stream/resume/*', route => route.fulfill({ status: 404, body: '{}' }))
  await composer(page).fill('Transporttest: behold et ufullstendig svar synlig.')
  await page.locator('form button[type="submit"]').first().click()
  await expect(assistant(page)).toContainText('ufullstendig svar')
  await expect(assistant(page)).toContainText('Stoppet')
  await expect(assistant(page).getByRole('button', { name: 'Fortsett', exact: true })).toBeVisible()
  await expect(page.getByRole('status').filter({ hasText: 'Svaret ble stoppet.' })).toBeVisible()
  await expect(page.getByRole('status').filter({ hasText: 'Svar fullført.' })).toHaveCount(0)
  expect(invocations).toBe(1)
  await page.screenshot({ path: info.outputPath('interrupted.png'), animations: 'disabled' })
})

test('Q08 a failed turn never silently invokes another provider', async ({ page }, info) => {
  await openFresh(page, '/chat', { mockedInference: true })
  let invocations = 0
  await page.route('**/api/v1/chat/stream', route => {
    expect(route.request().postDataJSON()).toMatchObject({ model: PRODUCT_MODEL, provider: PRODUCT_PROVIDER })
    invocations++
    return route.fulfill({ status: 200, contentType: 'text/event-stream', body:
      frame('connected', { request_id: 'q08-failed-run', ok: true }, 1) +
      frame('chunk', { content: 'Et delvis resultat som må beholdes.' }, 2) +
      frame('error', { code: 'tool_execution_failed', message: 'Verktøyet er utilgjengelig. Prøv igjen senere.' }, 3) })
  })
  await composer(page).fill('Transporttest: vis feil og behold det delvise resultatet.')
  await page.locator('form button[type="submit"]').first().click()
  await expect(page.getByText('Verktøyet er utilgjengelig. Prøv igjen senere.', { exact: true }).first()).toBeVisible()
  await expect(assistant(page)).toContainText('Et delvis resultat')
  const transportError = page.locator('.verevon-chat-error[role="alert"]')
  await expect(transportError).toHaveText('Verktøyet er utilgjengelig. Prøv igjen senere.')
  await expect(transportError).toBeFocused()
  await expect(page.getByRole('status').filter({ hasText: 'Svaret mislyktes.' })).toBeVisible()
  await expect(page.getByRole('status').filter({ hasText: 'Svar fullført.' })).toHaveCount(0)
  expect(invocations).toBe(1)
  await page.screenshot({ path: info.outputPath('failed-turn.png'), animations: 'disabled' })
})

test('Q08 live stop, continue and regenerate preserve one exchange', async ({ page }, info) => {
  await openFresh(page)
  await restrictSources(page)
  const prompt = 'Skriv direkte i chatten, uten dokument eller verktøy: nøyaktig 40 nummererte linjer med ett praktisk råd om å organisere et skrivebord per linje. Nummerer fra 1 til 40. Ingen innledning eller avslutning.'
  await composer(page).fill(prompt)
  await page.locator('form button[type="submit"]').first().click()
  await expect.poll(async () => (await transcript(page))?.turns.at(-1)?.content.length ?? 0, { timeout: 90_000 }).toBeGreaterThan(180)
  await stopButton(page).click()
  await expect(assistant(page)).toContainText('Stoppet')
  const partial = (await transcript(page))?.turns.at(-1)?.content ?? ''
  expect(partial.length).toBeGreaterThan(20)
  await page.screenshot({ path: info.outputPath('stopped.png') })
  const continuedResponse = page.waitForResponse(r => r.url().endsWith('/api/v1/chat/stream') && r.request().method() === 'POST')
  await assistant(page).getByRole('button', { name: 'Fortsett', exact: true }).click()
  await (await continuedResponse).finished()
  await expect(stopButton(page)).toBeHidden({ timeout: 120_000 })
  const continued = await transcript(page)
  expect(continued?.turns.filter(t => t.role === 'user')).toHaveLength(1)
  expect(continued?.turns.filter(t => t.role === 'assistant')).toHaveLength(1)
  expect(continued?.turns.at(-1)?.content.startsWith(partial)).toBe(true)
  expect(continued?.turns.at(-1)?.content).toMatch(/40[.)]/)
  const regeneratedResponse = page.waitForResponse(r => r.url().endsWith('/api/v1/chat/stream') && r.request().method() === 'POST')
  await assistant(page).getByRole('button', { name: /^Generer på nytt/ }).click()
  await (await regeneratedResponse).finished()
  await expect(stopButton(page)).toBeHidden({ timeout: 120_000 })
  expect((await transcript(page))?.turns).toHaveLength(2)
  await expect(assistant(page).getByRole('group', { name: 'Svarversjon' })).toContainText('3/3')
  await page.reload()
  await expect.poll(async () => (await transcript(page))?.turns.at(-1)?.content, { timeout: 20_000 }).toMatch(/40[.)]/)
  expect((await transcript(page))?.turns).toHaveLength(2)
  await info.attach('recovered-transcript', { body: JSON.stringify(await transcript(page)), contentType: 'application/json' })
})

test('Q08 live reload resumes the same run without another invocation', async ({ page }, info) => {
  await openFresh(page)
  await restrictSources(page)
  let invokes = 0; let resumes = 0
  page.on('request', r => { if (r.url().endsWith('/api/v1/chat/stream') && r.method() === 'POST') invokes++; if (r.url().includes('/api/v1/chat/stream/resume/')) resumes++ })
  await composer(page).fill('Svar direkte i chatten uten verktøy med 35 nummererte linjer om å organisere et kontor. Hver linje skal være en full setning. Start med 1. og avslutt med 35.')
  await page.locator('form button[type="submit"]').first().click()
  await expect.poll(async () => (await transcript(page))?.turns.at(-1)?.content.length ?? 0, { timeout: 90_000 }).toBeGreaterThan(180)
  await info.attach('before-reload', { body: JSON.stringify(await transcript(page)), contentType: 'application/json' })
  await page.reload()
  await expect.poll(async () => (await transcript(page))?.turns.at(-1)?.content, { timeout: 120_000 }).toMatch(/35[.)]/)
  await expect(stopButton(page)).toBeHidden({ timeout: 120_000 })
  expect(invokes).toBe(1)
  expect(resumes).toBeGreaterThan(0)
  expect((await transcript(page))?.turns).toHaveLength(2)
  await info.attach('replay', { body: JSON.stringify({ invokes, resumes, transcript: await transcript(page) }), contentType: 'application/json' })
  await page.screenshot({ path: info.outputPath('replayed.png') })
})

test('Q08 mobile markdown, keyboard and reduced motion remain usable', async ({ page }, info) => {
  const errors: string[] = []; page.on('pageerror', error => errors.push(error.message))
  page.on('console', entry => { if (entry.type() === 'error') errors.push(entry.text()) })
  await page.setViewportSize({ width: 390, height: 844 })
  await page.emulateMedia({ reducedMotion: 'reduce' })
  await openFresh(page)
  await restrictSources(page)
  await send(page, 'Svar direkte i chatten uten verktøy. Lag en tabell med kolonnene Oppgave, Ansvarlig, Frist og Status og tre fiktive rader. Skriv deretter formelen $a^2+b^2=c^2$ og en kort kodeblokk med console.log("klar"). Ingen andre påstander.', info, 'mobile')
  await expect(assistant(page).locator('table')).toBeVisible()
  await expect(assistant(page).locator('.katex')).toBeVisible()
  await expect(assistant(page).locator('pre')).toBeVisible()
  await expect(page.locator('.verevon-chat-divider time').last()).toHaveText('I dag')
  await expect(assistant(page).locator('time').first()).toHaveText(/Nå nettopp|for \d+ min siden/)
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true)
  await composer(page).focus()
  await expect(composer(page)).toBeFocused()
  await page.keyboard.press('Tab')
  expect(await page.evaluate(() => document.activeElement?.tagName)).not.toBe('BODY')
  await expect(page.locator('vite-error-overlay')).toHaveCount(0)
  expect(errors).toEqual([])
  await page.screenshot({ path: info.outputPath('mobile.png'), fullPage: true })
})
