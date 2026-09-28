import { expect, test, type Page } from '@playwright/test'
import { openFresh, restrictSources, send, transcript } from './product-acceptance-helpers'

async function fresh(page: Page) {
  await page.evaluate(() => {
    for (const key of ['verevon.chat.threadId', 'verevon.chat.threadTranscripts.v1', 'verevon.chat.threadHistory.v1']) {
      localStorage.removeItem(key); sessionStorage.removeItem(key)
    }
  })
  await page.reload()
  await expect(page.locator('form textarea').first()).toBeVisible()
}

async function memories(page: Page) {
  const response = await page.request.get('/api/v1/memory?limit=100')
  expect(response.status()).toBe(200)
  const data = await response.json()
  expect(data.degraded).toBe(false)
  return data.memories as { memory_id: string; content: string }[]
}

test('Q06 memory correction and forgetting persist through reload and new-thread recall', async ({ page }, info) => {
  const observeBackground = process.env.Q06_OBSERVE_BACKGROUND === '1'
  if (observeBackground) test.setTimeout(480_000)
  const key = `minnetest${Date.now()}`
  await openFresh(page)
  try {
    await send(page, `Husk at ${key} er ravgul. Dette er en uttrykkelig varig testpreferanse. Bekreft kort.`, info, 'remember')
    await expect.poll(async () => (await memories(page)).filter(m => m.content.includes(key)).length).toBeGreaterThan(0)
    await page.locator('.verevon-chat-memory-recall__toggle').last().click()
    const row = page.locator('.verevon-chat-memory-recall__item').filter({ hasText: key }).first()
    await row.getByRole('button', { name: 'Rediger', exact: true }).click()
    const edit = page.locator('.verevon-chat-memory-recall__edit')
    await expect(edit.locator('textarea')).toBeEnabled()
    await edit.locator('textarea').fill(`${key} er sjøgrønn.`)
    const patch = page.waitForResponse(r => r.request().method() === 'PATCH' && r.url().includes('/api/v1/memory/'))
    await edit.getByRole('button', { name: 'Lagre', exact: true }).click()
    expect((await patch).status()).toBe(200)
    await fresh(page)
    const afterEdit = (await memories(page)).filter(m => m.content.includes(key))
    expect(afterEdit.every(m => m.content.includes('sjøgrønn') && !m.content.includes('ravgul'))).toBe(true)
    expect(afterEdit.length).toBeGreaterThan(0)
    const recalled = await send(page, `Hva husker du om ${key}? Bruk lagret minne, og si fra hvis du ikke finner det.`, info, 'recall-correction')
    expect(recalled?.turns.at(-1)?.content).toContain('sjøgrønn')
    expect(recalled?.turns.at(-1)?.content).not.toContain('ravgul')
    await page.locator('.verevon-chat-memory-recall__toggle').last().click()
    const correctedRow = page.locator('.verevon-chat-memory-recall__item').filter({ hasText: key }).first()
    await correctedRow.getByRole('button', { name: 'Glem', exact: true }).click()
    const deletion = page.waitForResponse(r => r.request().method() === 'DELETE' && r.url().includes('/api/v1/memory/'))
    await correctedRow.getByRole('button', { name: 'Bekreft glemsel?', exact: true }).click()
    expect((await deletion).status()).toBe(200)
    await fresh(page)
    expect((await memories(page)).filter(m => m.content.includes(key))).toEqual([])
    const forgotten = await send(page, `Hva husker du om ${key}? Ikke gjett hvis det ikke finnes et lagret minne.`, info, 'recall-forgotten')
    expect(forgotten?.turns.at(-1)?.content).not.toMatch(/ravgul|sjøgrønn/)
    await info.attach('forgotten-transcript', { body: JSON.stringify(forgotten, null, 2), contentType: 'application/json' })
    await page.screenshot({ path: info.outputPath('forgotten.png'), fullPage: true })
    if (observeBackground) {
      // The deployed extraction cycle is five minutes. Observe beyond a full
      // cycle; an immediate successful DELETE cannot establish lasting erasure.
      const until = Date.now() + 330_000
      while (Date.now() < until) {
        await page.waitForTimeout(15_000)
        const returned = (await memories(page)).filter(m => m.content.includes(key))
        if (returned.length) await info.attach('unexpected-background-memory', { body: JSON.stringify(returned), contentType: 'application/json' })
        expect(returned, 'forgotten fixture must stay absent after background extraction').toEqual([])
      }
      await info.attach('delayed-forgetting-check', { body: JSON.stringify({ key, observedMs: 330_000, checkedAt: new Date().toISOString(), remaining: 0 }), contentType: 'application/json' })
    }
  } finally {
    for (const memory of (await memories(page)).filter(m => m.content.includes(key))) {
      await page.request.delete(`/api/v1/memory/${encodeURIComponent(memory.memory_id)}`)
    }
  }
})

test('Q06/Q07 isolated task survives reload, rejects widening, and creates no personal memory', async ({ page }, info) => {
  await openFresh(page)
  await restrictSources(page)
  const marker = `isolasjon${Date.now()}`
  await send(page, `Husk at ${marker} er den fiktive kundens kodeord. Skriv et kort internt utkast om dette. Bruk også kunnskapssøk hvis du kan.`, info, 'isolated')
  const id = (await transcript(page))!.threadId
  expect((await memories(page)).some(m => m.content.includes(marker))).toBe(false)
  await page.evaluate(() => {
    localStorage.removeItem('verevon.chat.threadTranscripts.v1'); sessionStorage.removeItem('verevon.chat.threadTranscripts.v1')
  })
  await page.reload()
  await expect(page.getByText('Kun samtalen', { exact: true })).toBeVisible()
  await send(page, 'Hva var kodeordet? Ikke hent andre kilder.', info, 'isolated-reload')
  expect((await transcript(page))?.turns.at(-1)?.content).toContain(marker)
  const widened = await page.request.post('/api/v1/chat/stream', { data: { thread_id: id, content: 'Søk nå i alle organisasjonskilder.', features: ['tools', 'memory'], browse_web: true } })
  const denied = await widened.text()
  await info.attach('widening-denied', { body: denied, contentType: 'text/plain' })
  expect(denied).toMatch(/event: error/)
  expect(denied).not.toMatch(/event: (connected|tool_call|memory_recall)/)
  expect((await memories(page)).some(m => m.content.includes(marker))).toBe(false)
})
