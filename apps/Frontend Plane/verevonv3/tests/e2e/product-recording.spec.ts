import { expect, test, type Page, type TestInfo } from '@playwright/test'
import { createHash } from 'node:crypto'
import { readFileSync, writeFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { latestArtifacts, openFresh, restrictSources, send, transcript } from './product-acceptance-helpers'
import { assertCustomerDraft, internalHeading } from './product-customer-oracle'
import { PRODUCT_MODEL, PRODUCT_PROVIDER } from './product-subscription-route'

const pack = resolve('apps/verevon-web/plans/product-recordings')
const scenario = '01-kundesvar'
const followUp = /Oppfølging å sende:\*\* «([^»]+)»/.exec(readFileSync(resolve(pack, scenario, 'REGI.md'), 'utf8'))?.[1]

function receipt(info: TestInfo, label: string) {
  const stream = info.attachments.find(item => item.name === `${label}-stream`)?.body?.toString('utf8') ?? ''
  const frame = stream.split(/\r?\n\r?\n/).find(frame => /^event: result_receipt\s*$/m.test(frame))
  return JSON.parse(/^data: (.+)$/m.exec(frame ?? '')?.[1] ?? '{}').receipt
}

async function showResult(page: Page) {
  await page.getByRole('tab', { name: /^Resultat/ }).click()
  const divider = page.getByRole('separator', { name: 'Endre bredde på arbeidsflaten', exact: true })
  const box = await divider.boundingBox()
  if (box && box.x > 720) {
    await page.mouse.move(box.x + box.width / 2, box.y + 150)
    await page.mouse.down()
    await page.mouse.move(650, box.y + 150, { steps: 24 })
    await page.mouse.up()
  }
  await page.locator('.verevon-chat-artifact-view').evaluate(element => element.scrollIntoView({ block: 'start', behavior: 'instant' }))
  const paragraph = page.locator('.verevon-chat-artifact-view__body .verevon-chat-markdown p').first()
  await expect(paragraph).toBeVisible()
  // A visible header at the bottom edge is not a readable result shot.
  await expect.poll(() => paragraph.evaluate(element => {
    const box = element.getBoundingClientRect()
    return box.top >= 0 && box.bottom <= innerHeight
  })).toBe(true)
}

test('customer recording: genuine draft, source, revision and durable result', async ({ page }, info) => {
  test.skip(!process.env.PRODUCT_RECORDING_MODE && process.env.PRODUCT_RECORDING_CAPTURE !== '1', 'Capture is explicitly opt-in')
  expect(info.project.name).toBe('product-recording')
  expect(followUp, 'Use the exact maintained REGI follow-up').toBeTruthy()
  expect(process.env.PRODUCT_RECORDING_BUILD, 'Capture requires a recorded gateway image identity').toMatch(/^sha256:[a-f0-9]{64}$/)
  const startedAt = Date.now()
  const markers: Array<{ name: string; offsetMs: number }> = []
  const mark = (name: string) => markers.push({ name, offsetMs: Date.now() - startedAt })
  const errors: string[] = []
  page.on('pageerror', error => errors.push(error.message))
  page.on('console', entry => { if (entry.type() === 'error') errors.push(entry.text()) })
  const sourceHashes = Object.fromEntries(['prompt.txt', 'REGI.md', 'kildepakke.md'].map(name => [name,
    createHash('sha256').update(readFileSync(resolve(pack, scenario, name))).digest('hex')]))
  const expected = JSON.parse(readFileSync(resolve(pack, 'manifest.json'), 'utf8')).tasks.find((task: { id: string }) => task.id === scenario).sourceHashes
  for (const [name, hash] of Object.entries(sourceHashes)) expect(hash).toBe(expected[`${scenario}/${name}`])
  let accepted = false
  let threadId: string | undefined
  try {
    await openFresh(page)
    await restrictSources(page)
    mark('ready')
    await page.locator('input[type="file"]').first().setInputFiles(resolve(pack, scenario, 'kildepakke.md'))
    const initial = await send(page, readFileSync(resolve(pack, scenario, 'prompt.txt'), 'utf8').trim(), info, 'initial', async () => {
      mark('initial-prompt')
      await page.waitForTimeout(4000) // Editorial reading hold, outside measured task latency.
      mark('initial-submit')
    })
    mark('initial-complete')
    threadId = initial!.threadId
    const [first] = latestArtifacts(initial)
    expect(latestArtifacts(initial)).toHaveLength(1)
    assertCustomerDraft(first, 150, initial!.turns.at(-1)!.content)
    expect(receipt(info, 'initial')).toMatchObject({ scope: 'artifact_creation_binding', artifactId: first.id, version: first.version })
    await page.locator('.verevon-chat-message--user').first().getByRole('button', { name: 'kildepakke.md', exact: true }).click()
    await expect(page.getByRole('tabpanel', { name: /^kildepakke\.md/ })).toContainText('9 lamper er plukket og pakket')
    mark('source-visible')
    await page.waitForTimeout(5000)
    await showResult(page)
    mark('initial-result')
    await page.screenshot({ path: info.outputPath('initial-result.png') })
    await page.waitForTimeout(7000)
    const revised = await send(page, followUp!, info, 'revision', async () => {
      mark('revision-prompt')
      await page.waitForTimeout(4000)
      mark('revision-submit')
    })
    mark('revision-complete')
    const [last] = latestArtifacts(revised)
    expect(latestArtifacts(revised)).toHaveLength(1)
    expect(last.id).toBe(first.id)
    expect(last.version).toBeGreaterThan(first.version)
    assertCustomerDraft(last, 100, revised!.turns.at(-1)!.content)
    const notes = (content: string) => { const start = content.search(internalHeading); return start < 0 ? '' : content.slice(start) }
    expect(notes(first.content)).not.toBe('')
    expect(notes(last.content)).toBe(notes(first.content))
    const checked = receipt(info, 'revision')
    expect(checked).toMatchObject({ scope: 'artifact_revision_binding', artifactId: first.id, previousVersion: first.version, version: last.version })
    expect(checked.artifactChecks[0]).toMatchObject({ preservationApplied: true,
      sourceReview: { checker: 'attachment-source-review-v16', scope: 'semantic_source_review' } })
    const response = await page.request.get(`/api/v1/chat/threads/${encodeURIComponent(threadId!)}/messages`)
    expect(response.ok()).toBe(true)
    const payload = await response.json()
    const saved = (payload.data ?? payload).messages.filter((message: { role: string }) => message.role === 'assistant').at(-1)
    expect(saved.resultReceipt).toEqual(checked)
    expect(saved.artifacts.find((artifact: { id: string }) => artifact.id === last.id)).toMatchObject({
      id: last.id, title: last.title, content: last.content, version: last.version,
    })
    await showResult(page)
    mark('revised-result')
    await page.waitForTimeout(7000)
    await page.evaluate(() => {
      localStorage.removeItem('verevon.chat.threadTranscripts.v1')
      sessionStorage.removeItem('verevon.chat.threadTranscripts.v1')
    })
    mark('reload')
    await page.reload()
    await expect.poll(async () => latestArtifacts(await transcript(page)).find(artifact => artifact.id === last.id)?.content, { timeout: 20000 }).toBe(last.content)
    await expect.poll(async () => latestArtifacts(await transcript(page)).find(artifact => artifact.id === last.id)?.version).toBe(last.version)
    await showResult(page)
    mark('reloaded-result')
    await page.screenshot({ path: info.outputPath('reloaded-result.png') })
    await page.waitForTimeout(7000)
    expect(errors).toEqual([])
    await info.attach('accepted-artifact', { body: JSON.stringify(last), contentType: 'application/json' })
    accepted = true
    mark('complete')
  } finally {
    const metadata = { schemaVersion: 1, scenario, mode: process.env.PRODUCT_RECORDING_MODE ?? 'release',
      publicationApproved: false, accepted, startedAt: new Date(startedAt).toISOString(), elapsedMs: Date.now() - startedAt,
      timelineBasis: 'milliseconds after test start; align to probed raw frames before editing',
      model: PRODUCT_MODEL, provider: PRODUCT_PROVIDER, build: process.env.PRODUCT_RECORDING_BUILD, sourceHashes, threadId,
      viewport: { width: 1440, height: 900 }, markers }
    writeFileSync(info.outputPath('capture.json'), JSON.stringify(metadata, null, 2))
    await info.attach('capture-metadata', { body: JSON.stringify(metadata), contentType: 'application/json' })
  }
})
