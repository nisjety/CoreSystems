import { expect, test } from '@playwright/test'
import { memberContext, requireMembers, requireSpaceRef } from './space-fixtures'

/**
 * WS01-S7a thread side pane, end to end (later tier: needs the seeded Space fixtures and a room with
 * at least one thread). The deep link is `/spaces/<ref>?thread_id=<id>#chat`. Member A owns the
 * Space; E is a same-org member who is not in it. No call here runs a model.
 */

async function firstThreadId(page: import('@playwright/test').Page, ref: string): Promise<string | undefined> {
  const response = await page.request.get(`/api/v1/spaces/${encodeURIComponent(ref)}/threads`)
  if (response.status() !== 200) return undefined
  const body = (await response.json()) as { data?: { threads?: Array<{ thread_id?: string }> }; threads?: Array<{ thread_id?: string }> }
  const threads = body.data?.threads ?? body.threads ?? []
  return threads[0]?.thread_id
}

test('a deep link opens the pane, Escape returns focus, and the arrow keys resize it', async ({ browser, baseURL }) => {
  const ref = requireSpaceRef(test)
  requireMembers(test, 'A')
  const a = await memberContext(browser, baseURL, 'A')
  try {
    const page = await a.newPage()
    await page.goto(`/spaces/${encodeURIComponent(ref)}`)
    const threadId = await firstThreadId(page, ref)
    test.skip(!threadId, 'the fixture Space has no thread yet')

    await page.goto(`/spaces/${encodeURIComponent(ref)}?thread_id=${encodeURIComponent(threadId ?? '')}#chat`)
    const pane = page.locator('.verevon-space-auxpane')
    await expect(pane).toBeVisible()

    // Keyboard resize on the separator: 32px steps, within 320..720.
    const separator = pane.getByRole('separator')
    const before = Number(await separator.getAttribute('aria-valuenow'))
    await separator.focus()
    await page.keyboard.press('ArrowLeft')
    await expect(separator).toHaveAttribute('aria-valuenow', String(Math.min(720, before + 32)))
    await page.keyboard.press('ArrowRight')
    await expect(separator).toHaveAttribute('aria-valuenow', String(before))
    // The width is the only thing stored.
    expect(await page.evaluate(() => window.localStorage.getItem('verevon.space.auxpane.width.v1'))).toBe(String(before))

    // Escape closes the pane and the URL drops the parameter.
    await pane.focus()
    await page.keyboard.press('Escape')
    await expect(pane).toHaveCount(0)
    await expect(page).not.toHaveURL(/thread_id=/)
  } finally {
    await a.close()
  }
})

test('the Open thread button opens the pane and focus returns to it when the pane closes', async ({ browser, baseURL }) => {
  const ref = requireSpaceRef(test)
  requireMembers(test, 'A')
  const a = await memberContext(browser, baseURL, 'A')
  try {
    const page = await a.newPage()
    await page.goto(`/spaces/${encodeURIComponent(ref)}#chat`)
    const open = page.getByRole('button', { name: /Åpne tråd|Open thread/ }).first()
    test.skip((await open.count()) === 0, 'the fixture Space has no thread yet')
    await open.focus()
    await open.click()
    const pane = page.locator('.verevon-space-auxpane')
    await expect(pane).toBeVisible()
    await page.keyboard.press('Escape')
    await expect(pane).toHaveCount(0)
    await expect(open).toBeFocused()
  } finally {
    await a.close()
  }
})

test('a member outside the Space sees the restricted or not-available state, never a sign-in message', async ({ browser, baseURL }) => {
  const ref = requireSpaceRef(test)
  requireMembers(test, 'A', 'E')
  const a = await memberContext(browser, baseURL, 'A')
  const outsider = await memberContext(browser, baseURL, 'E')
  try {
    const owner = await a.newPage()
    await owner.goto(`/spaces/${encodeURIComponent(ref)}`)
    const threadId = await firstThreadId(owner, ref)
    test.skip(!threadId, 'the fixture Space has no thread yet')

    const page = await outsider.newPage()
    await page.goto(`/spaces/${encodeURIComponent(ref)}?thread_id=${encodeURIComponent(threadId ?? '')}#chat`)
    // The parameter grants nothing: the room refuses the outsider, in words.
    await expect(page.getByText(/ikke delt med deg|ikke tilgjengelig|do not have access|not available|not shared/i).first()).toBeVisible()
    await expect(page.getByText(/logg inn|sign in|log in/i)).toHaveCount(0)
  } finally {
    await a.close()
    await outsider.close()
  }
})
