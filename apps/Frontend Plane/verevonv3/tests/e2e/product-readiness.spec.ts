import { expect, test } from '@playwright/test'
import { composer, openFresh, restrictSources, transcript, send, latestArtifacts, type Artifact } from './product-acceptance-helpers'
import { assertCustomerDraft, internalHeading } from './product-customer-oracle'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

const fixtureRoot = resolve('apps/verevon-web/plans/product-recordings/01-kundesvar')
const attachmentFixtures = JSON.parse(readFileSync(resolve('tests/e2e/fixtures/chat-attachments.json'), 'utf8')) as Array<{ name: string; mimeType: string; base64: string }>
test('Q02 an extraction failure keeps the draft and file and starts no inference', async ({ page }) => {
  await openFresh(page)
  let sent = 0
  page.on('request', r => { if (r.url().endsWith('/api/v1/chat/stream')) sent++ })
  await page.route('**/api/v1/chat/documents/extract', route => route.fulfill({ status: 503, contentType: 'application/json', body: JSON.stringify({ error: { code: 'unavailable', message: 'Extraction unavailable' } }) }))
  const file = attachmentFixtures.find(f => f.name.endsWith('.pdf'))!
  await page.locator('input[type="file"]').first().setInputFiles({ name: file.name, mimeType: file.mimeType, buffer: Buffer.from(file.base64, 'base64') })
  await composer(page).fill('Behold dette utkastet og les filen.')
  await page.locator('form button[type="submit"]').first().click()
  await expect(page.getByRole('alert')).toContainText('source.pdf')
  await expect(composer(page)).toHaveValue('Behold dette utkastet og les filen.')
  await expect(page.getByRole('button', { name: 'Fjern source.pdf', exact: true })).toBeVisible()
  expect(sent).toBe(0)
})

test('Q02 full storage keeps a dashboard draft before navigation', async ({ page }) => {
  await openFresh(page, '/dashboard')
  await page.evaluate(() => {
    const original = Storage.prototype.setItem
    Storage.prototype.setItem = function(key, value) {
      if (key === 'verevon.chat.pendingLaunch') throw new DOMException('Full', 'QuotaExceededError')
      return original.call(this, key, value)
    }
  })
  await composer(page).fill('Utkast som skal overleve lagringsfeil.')
  await page.locator('form button[type="submit"]').first().click()
  await expect(page.getByRole('alert')).toContainText('beholdt')
  await expect(composer(page)).toHaveValue('Utkast som skal overleve lagringsfeil.')
  await expect(page).toHaveURL(/\/dashboard$/)
})

for (const entry of ['/chat', '/dashboard']) test(`Q02 PDF DOCX CSV MD are read completely from ${entry}`, async ({ page }, info) => {
  const errors: string[] = []; page.on('pageerror', e => errors.push(e.message))
  await openFresh(page, entry)
  await page.locator('input[type="file"]').first().setInputFiles(attachmentFixtures.map(f => ({ name: f.name, mimeType: f.mimeType, buffer: Buffer.from(f.base64, 'base64') })))
  const result = await send(page, 'Les alle fire vedlegg, inkludert tabellen i Word-filen. Svar kun med filnavn og den eksakte koden fra hver fil. Ikke søk i andre kilder.', info, 'attachments')
  const output = result?.turns.filter(t => t.role === 'assistant').map(t => t.content + (t.artifacts ?? []).map(a => a.content).join('\n')).join('\n') ?? ''
  for (const code of ['HARBOR-739', 'FJORD-482', 'LIGHT-916', 'NORTH-257']) expect(output).toContain(code)
  await page.screenshot({ path: info.outputPath('attachments.png'), fullPage: true })
  expect(errors).toEqual([])
})

test('Q04 customer draft is revised in the same artifact and survives server reload', async ({ page }, info) => {
  const errors: string[] = []; page.on('pageerror', e => errors.push(e.message))
  page.on('console', entry => { if (entry.type() === 'error') errors.push(entry.text()) })
  await openFresh(page)
  await restrictSources(page)
  await page.locator('input[type="file"]').first().setInputFiles(resolve(fixtureRoot, 'kildepakke.md'))
  const initial = await send(page, readFileSync(resolve(fixtureRoot, 'prompt.txt'), 'utf8').trim(), info, 'initial')
  await info.attach('initial-transcript', { body: JSON.stringify(initial, null, 2), contentType: 'application/json' })
  const artifacts = latestArtifacts(initial)
  expect(artifacts, 'customer draft should be a usable artifact').toHaveLength(1)
  const first = artifacts[0]!
  const initialStream = info.attachments.find(attachment => attachment.name === 'initial-stream')?.body?.toString('utf8') ?? ''
  expect(initialStream, 'the runtime counts this customer body without another model round').not.toMatch(/"name":"count_words"/)
  const initialReceiptFrame = initialStream.split(/\r?\n\r?\n/).find(frame => /^event: result_receipt\s*$/m.test(frame))
  const initialReceipt = JSON.parse(/^data: (.+)$/m.exec(initialReceiptFrame ?? '')?.[1] ?? '{}').receipt
  expect(initialReceipt, 'questions inside source files must not trigger a second unrestricted answer').toMatchObject({
    scope: 'artifact_creation_binding', artifactId: first.id, version: first.version,
  })
  assertCustomerDraft(first, 150, initial?.turns.filter(turn => turn.role === 'assistant').at(-1)?.content ?? '')
  await page.screenshot({ path: info.outputPath('initial.png'), fullPage: true })
  const followUp = 'Gjør svaret litt varmere og kortere, maks 100 ord. Behold usikkerheten rundt leveringsdatoen og den interne kildeoversikten.'
  const revised = await send(page, followUp, info, 'revision')
  await info.attach('revised-transcript', { body: JSON.stringify(revised, null, 2), contentType: 'application/json' })
  const latest = latestArtifacts(revised)
  expect(latest).toHaveLength(1)
  expect(latest[0]!.id).toBe(first.id)
  expect(latest[0]!.version).toBeGreaterThan(first.version)
  const revisionStream = info.attachments.find(attachment => attachment.name === 'revision-stream')?.body?.toString('utf8') ?? ''
  const countCalls = revisionStream.split(/\r?\n\r?\n/).filter(frame => /^event: tool_call\s*$/m.test(frame))
    .map(frame => JSON.parse(/^data: (.+)$/m.exec(frame)?.[1] ?? '{}')).filter(call => call.name === 'count_words')
  expect(countCalls.length, 'the runtime counts this revision without another model round').toBe(0)
  await expect(page.locator('.verevon-chat-page')).not.toContainText('Do not run another review')
  const receiptFrame = revisionStream.split(/\r?\n\r?\n/).find(frame => /^event: result_receipt\s*$/m.test(frame))
  const receipt = JSON.parse(/^data: (.+)$/m.exec(receiptFrame ?? '')?.[1] ?? '{}').receipt
  expect(receipt, 'revision completion must reference the actual artifact version').toMatchObject({
    scope: 'artifact_revision_binding', artifactId: first.id, previousVersion: first.version, version: latest[0]!.version,
  })
  expect(revised?.turns.at(-1)?.content).toContain(`versjon ${first.version} til ${latest[0]!.version}`)
  const persisted = await page.request.get(`/api/v1/chat/threads/${encodeURIComponent(revised!.threadId)}/messages`)
  expect(persisted.ok()).toBe(true)
  const payload = await persisted.json()
  const saved = (payload.data ?? payload).messages.filter((message: { role: string }) => message.role === 'assistant').at(-1)
  expect(saved.resultReceipt).toEqual(receipt)
  expect(receipt.artifactChecks).toHaveLength(1)
  expect(receipt.artifactChecks[0]).toMatchObject({ artifactId: first.id, preservationApplied: true,
    sourceReview: { checker: 'attachment-source-review-v16', scope: 'semantic_source_review' } })
  const { id, title, content, version } = latest[0]!
  expect(saved.artifacts.find((artifact: Artifact) => artifact.id === first.id)).toMatchObject({ id, title, content, version })
  assertCustomerDraft(latest[0]!, 100, revised?.turns.filter(turn => turn.role === 'assistant').at(-1)?.content ?? '')
  const note = (value: string) => { const start = value.search(internalHeading); return start < 0 ? '' : value.slice(start) }
  expect(note(first.content), 'the source table must be in the durable document').not.toBe('')
  expect(note(latest[0]!.content), 'preserve the requested internal notes exactly, irrespective of summary wording').toBe(note(first.content))
  // Remove the local transcript to prove the server supplies the revision.
  await page.evaluate(() => {
    localStorage.removeItem('verevon.chat.threadTranscripts.v1')
    sessionStorage.removeItem('verevon.chat.threadTranscripts.v1')
  })
  await page.reload()
  await expect(page.locator('.verevon-chat-page')).toBeVisible()
  await expect.poll(async () => latestArtifacts(await transcript(page)).find(a => a.id === first.id)?.content, { timeout: 20_000 }).toBe(latest[0]!.content)
  await expect.poll(async () => latestArtifacts(await transcript(page)).find(a => a.id === first.id)?.version).toBe(latest[0]!.version)
  await expect(page.locator('.verevon-chat-message--user').first()).not.toContainText('--- VEDLEGG:')
  await expect(page.locator('.verevon-chat-message--user').first()).toContainText('kildepakke.md')
  await page.locator('.verevon-chat-message--user').first().getByRole('button', { name: 'kildepakke.md', exact: true }).click()
  await expect(page.getByRole('tabpanel', { name: /^kildepakke\.md/ })).toContainText('9 lamper er plukket og pakket')
  await expect(page.getByRole('tabpanel', { name: /^kildepakke\.md/ })).toContainText('K3')
  await page.screenshot({ path: info.outputPath('source-reopened.png'), fullPage: true })
  await page.getByRole('tab', { name: /^Resultat/ }).click()
  await page.locator('.verevon-chat-artifact-view').scrollIntoViewIfNeeded()
  await expect(page.locator('.verevon-chat-artifact-view')).not.toContainText('<!--')
  const history = page.locator('.verevon-chat-artifact-versions')
  await expect(history).toContainText(`v${latest[0]!.version}`)
  await page.getByRole('button', { name: 'Forrige versjon', exact: true }).click()
  await expect(history).toContainText(`v${first.version}`)
  await page.getByRole('button', { name: 'Neste versjon', exact: true }).click()
  await expect(history).toContainText(`v${latest[0]!.version}`)
  await page.locator('.verevon-chat-artifact-view').screenshot({ path: info.outputPath('customer-artifact.png') })
  await page.screenshot({ path: info.outputPath('revision-reloaded.png'), fullPage: true })
  await page.setViewportSize({ width: 390, height: 844 })
  await page.getByRole('tab', { name: /^Resultat/ }).click()
  await expect(page.locator('.verevon-chat-artifact-view')).toBeVisible()
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1)).toBe(true)
  // The page can hide overflow while an implicit grid column clips the
  // document inside its own panel. Check prose bounds, not only page width.
  expect(await page.locator('.verevon-chat-artifact-view').evaluate(panel => {
    const right = panel.getBoundingClientRect().right
    return [...panel.querySelectorAll('.verevon-chat-artifact-view__head, .verevon-chat-artifact-view__body, .verevon-chat-markdown > p')]
      .every(element => element.getBoundingClientRect().right <= right + 1)
  })).toBe(true)
  await page.screenshot({ path: info.outputPath('customer-artifact-mobile.png'), fullPage: true })
  await expect(page.locator('.feedback-widget__trigger')).toHaveCount(0)
  await page.setViewportSize({ width: 1440, height: 1000 })
  await page.locator('.verevon-chat-header').getByRole('button', { name: 'Flere handlinger', exact: true }).click()
  await page.getByRole('menuitem', { name: 'Gi tilbakemelding', exact: true }).click()
  await expect(page.getByRole('dialog', { name: 'Meld friksjon' })).toBeVisible()
  await page.screenshot({ path: info.outputPath('feedback-from-menu.png'), fullPage: true })
  await page.keyboard.press('Escape')
  await expect(page.getByRole('dialog', { name: 'Meld friksjon' })).toBeHidden()
  await expect(page.locator('.verevon-chat-header').getByRole('button', { name: 'Flere handlinger', exact: true })).toBeFocused()
  await expect(page.locator('.verevon-chat-artifact-view')).toBeVisible()
  await expect(page.locator('vite-error-overlay')).toHaveCount(0)
  expect(errors).toEqual([])
})
