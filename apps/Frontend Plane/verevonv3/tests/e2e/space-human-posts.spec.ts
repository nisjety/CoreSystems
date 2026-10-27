import { expect, test, type Page } from '@playwright/test'
import { memberContext, requireMembers, requireSpaceRef } from './space-fixtures'

/**
 * WS01-S6 human posts, end to end (later tier: needs the seeded Space fixtures and the human-post
 * routes on). Member A owns the Space, B is an editor, C a viewer (scripts/space-e2e-seed.sh).
 *
 * No call here starts a model run: a post is the member's own words, and the asserts below check
 * that the Verevon agent never answers. Nothing selects a Verevon, budget or genius model.
 */

const spaceUrl = (ref: string) => `/spaces/${encodeURIComponent(ref)}#chat`

// The composer is the only textbox in the room's chat tab.
const box = (page: Page) => page.getByRole('textbox').first()
const sendButton = (page: Page) => page.getByRole('button', { name: /^(Send|Send til rommet)$/ })

async function postFrom(page: Page, text: string) {
  await box(page).fill(text)
  await sendButton(page).click()
}

test('a post from A is seen by B, who replies, and no agent run starts', async ({ browser, baseURL }) => {
  const ref = requireSpaceRef(test)
  requireMembers(test, 'A', 'B')
  const a = await memberContext(browser, baseURL, 'A')
  const b = await memberContext(browser, baseURL, 'B')
  try {
    const pageA = await a.newPage()
    const pageB = await b.newPage()
    const marker = `post-${Date.now().toString(36)}`

    await pageA.goto(spaceUrl(ref))
    await postFrom(pageA, `Hei fra A ${marker}`)
    // The sender's own row settles into the room's record: the text shows once.
    await expect(pageA.getByText(`Hei fra A ${marker}`)).toHaveCount(1)

    await pageB.goto(spaceUrl(ref))
    await expect(pageB.getByText(`Hei fra A ${marker}`)).toBeVisible()

    // B replies in the thread, from the side pane.
    await pageB.getByRole('button', { name: /Åpne tråd|Open thread/ }).last().click()
    const pane = pageB.getByRole('complementary').last()
    await pane.getByRole('textbox').fill(`Svar fra B ${marker}`)
    await pane.getByRole('button', { name: /^Send$/ }).click()
    await expect(pageA.getByText(`Svar fra B ${marker}`)).toBeVisible({ timeout: 30_000 })

    // WS01-S7b: A comes back to the room. B's reply shows as a count on A's own thread, and A's own
    // thread is badged new because its latest message is someone else's (an owner-only rule would
    // have hidden it).
    await pageA.reload()
    const postA = pageA.locator('.verevon-room-post', { hasText: `Hei fra A ${marker}` })
    await expect(postA.getByRole('button', { name: /Åpne tråd · 1 svar|Open thread · 1 reply/ })).toBeVisible({ timeout: 30_000 })
    await expect(postA.getByText(/^(Ny|New)$/)).toBeVisible()

    // Human posts start no run: the room shows no working agent for them.
    await expect(pageA.getByText(/Verevon jobber|Verevon is working/)).toHaveCount(0)
  } finally {
    await a.close()
    await b.close()
  }
})

test('a post sent offline is queued and sent once when the browser is back online', async ({ browser, baseURL }) => {
  const ref = requireSpaceRef(test)
  requireMembers(test, 'A')
  const a = await memberContext(browser, baseURL, 'A')
  try {
    const page = await a.newPage()
    const marker = `offline-${Date.now().toString(36)}`
    const posts: string[] = []
    await page.route('**/api/v1/spaces/*/threads', async (route) => {
      if (route.request().method() === 'POST') posts.push(route.request().postData() ?? '')
      await route.continue()
    })
    await page.goto(spaceUrl(ref))

    await a.setOffline(true)
    await postFrom(page, `Frakoblet ${marker}`)
    await expect(page.getByText(/Sendes når du er på nett igjen|Sends when you are back online/)).toBeVisible()
    expect(posts).toHaveLength(0)

    await a.setOffline(false)
    await expect(page.getByText(`Frakoblet ${marker}`)).toHaveCount(1, { timeout: 30_000 })
    expect(posts).toHaveLength(1)
    expect(JSON.parse(posts[0] ?? '{}')).toMatchObject({ content: `Frakoblet ${marker}` })
  } finally {
    await a.close()
  }
})

test('a viewer sees a disabled box and the reason', async ({ browser, baseURL }) => {
  const ref = requireSpaceRef(test)
  requireMembers(test, 'C')
  const c = await memberContext(browser, baseURL, 'C')
  try {
    const page = await c.newPage()
    await page.goto(spaceUrl(ref))
    await expect(box(page)).toBeDisabled()
    await expect(page.getByText(/lesetilgang i dette rommet|read access in this room/)).toBeVisible()
  } finally {
    await c.close()
  }
})
