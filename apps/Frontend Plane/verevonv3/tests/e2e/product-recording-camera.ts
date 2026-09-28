import { expect, type Page, type TestInfo } from '@playwright/test'
import { createHash } from 'node:crypto'
import { readFileSync, writeFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { PRODUCT_MODEL, PRODUCT_PROVIDER } from './product-subscription-route'

/** Camera movement uses the real controls and scrolling; never replaces UI data. */
export async function frameRecordingResult(page: Page, title?: string) {
  await page.getByRole('tab', { name: /^Resultat/ }).click()
  if (title) await page.getByRole('tablist', { name: 'Artefakter i samtalen', exact: true }).getByRole('tab').filter({ hasText: title }).first().click()
  const divider = page.getByRole('separator', { name: 'Endre bredde på arbeidsflaten', exact: true })
  await divider.focus()
  await divider.press('End')
  await expect.poll(async () => await divider.getAttribute('aria-valuenow')).toBe(await divider.getAttribute('aria-valuemax'))
  const view = page.locator('.verevon-chat-artifact-view')
  await view.evaluate(element => element.scrollIntoView({ block: 'start', behavior: 'instant' }))
  const first = view.locator('.verevon-chat-artifact-view__body .verevon-chat-markdown').first()
  await expect(first).toBeVisible()
  await expect.poll(() => first.evaluate(element => {
    const box = element.getBoundingClientRect()
    return box.top >= 0 && box.top < innerHeight - 150
  })).toBe(true)
}

export function scenarioRecording(page: Page, info: TestInfo, scenario: string) {
  if (!process.env.PRODUCT_RECORDING_MODE && process.env.PRODUCT_RECORDING_CAPTURE !== '1') return undefined
  expect(process.env.PRODUCT_RECORDING_BUILD).toMatch(/^sha256:[a-f0-9]{64}$/)
  const pack = resolve('apps/verevon-web/plans/product-recordings')
  const task = JSON.parse(readFileSync(resolve(pack, 'manifest.json'), 'utf8')).tasks.find((task: { id: string }) => task.id === scenario)
  const sourceHashes: Record<string, string> = {}
  for (const [path, expected] of Object.entries(task.sourceHashes)) {
    const hash = createHash('sha256').update(readFileSync(resolve(pack, path))).digest('hex')
    expect(hash, path).toBe(expected)
    sourceHashes[path.slice(scenario.length + 1)] = hash
  }
  const startedAt = Date.now()
  const markers: Array<{ name: string; offsetMs: number }> = []
  const mark = (name: string) => markers.push({ name, offsetMs: Date.now() - startedAt })
  let accepted = false
  let threadId: string | undefined
  return {
    mark,
    thread: (id: string) => { threadId = id },
    prompt: async (label: string) => {
      mark(`${label}-prompt`)
      await page.waitForTimeout(4000) // Editorial pause before latency timing starts.
      mark(`${label}-submit`)
    },
    source: async () => {
      await page.locator('.verevon-chat-message--user').first().getByRole('button', { name: 'kildepakke.md', exact: true }).click()
      await expect(page.getByRole('tabpanel', { name: /^kildepakke\.md/ })).toBeVisible()
      mark('source-visible')
      await page.waitForTimeout(5000)
    },
    result: async (label: string, title: string) => {
      await frameRecordingResult(page, title)
      mark(`${label}-result`)
      await page.screenshot({ path: info.outputPath(`${label}-result.png`) })
      await page.waitForTimeout(7000)
      if (label === 'initial') {
        const table = page.locator('.verevon-chat-artifact-view__body table').first()
        if (await table.count()) {
          await table.evaluate(element => element.scrollIntoView({ block: 'center', behavior: 'instant' }))
          mark('initial-table')
          await page.waitForTimeout(6000)
          const wrapper = table.locator('..')
          if (await wrapper.evaluate(element => element.scrollWidth > element.clientWidth + 10)) {
            await wrapper.evaluate(element => element.scrollTo({ left: element.scrollWidth, behavior: 'smooth' }))
            await page.waitForTimeout(700)
            mark('initial-table-end')
            await page.waitForTimeout(4000)
          }
        }
      }
      if (scenario === '03-kampanje') {
        const body = page.locator('.verevon-chat-artifact-view__body .verevon-chat-markdown')
        const focus = label === 'initial'
          ? body.getByRole('heading', { name: /e-post|epost/i }).last()
          : body.locator('h1,h2,h3,h4,p').filter({ hasText: /^(?:(?:LinkedIn|innlegg).*\b23\b|23\.\s*sep|Dato\s*:\s*23\.\s*sep|(?:LinkedIn|innlegg)\s*2\b)/i }).first()
        // Heading/date variants are allowed by the task, so a missing optional
        // close-up does not change acceptance of the actual deliverable.
        if (await focus.count()) {
          await focus.evaluate(element => element.scrollIntoView({ block: 'start', behavior: 'instant' }))
          mark(`${label}-${label === 'initial' ? 'email' : 'changed-post'}`)
          await page.waitForTimeout(6000)
        }
      }
    },
    complete: () => { accepted = true; mark('complete') },
    save: async () => {
      const metadata = { schemaVersion: 1, scenario, mode: process.env.PRODUCT_RECORDING_MODE ?? 'release', publicationApproved: false,
        accepted, startedAt: new Date(startedAt).toISOString(), elapsedMs: Date.now() - startedAt, threadId,
        timelineBasis: 'milliseconds after test start; align to probed raw frames before editing',
        model: PRODUCT_MODEL, provider: PRODUCT_PROVIDER, build: process.env.PRODUCT_RECORDING_BUILD,
        viewport: { width: 1440, height: 900 }, sourceHashes, markers }
      writeFileSync(info.outputPath('capture.json'), JSON.stringify(metadata, null, 2))
      await info.attach('capture-metadata', { body: JSON.stringify(metadata), contentType: 'application/json' })
    },
  }
}
