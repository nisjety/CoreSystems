import { expect, test, type Page } from '@playwright/test'
import { memberContext, requireMembers, requireSpaceRef } from './space-fixtures'

/**
 * WS13-S5 room search, end to end (later tier: needs the seeded Space fixtures with message_search, thread_pane and
 * entry_links on). Member A owns the Space and posts the searchable word herself, so nothing here starts a model run
 * and nothing selects a Verevon, budget or genius model.
 *
 * Owed: Ctrl+F types a prefix, Enter on the full word opens the hit with ?thread_id&m, and the hit is scrolled to,
 * highlighted and focused. The query text itself never enters the URL.
 */

const spaceUrl = (ref: string) => `/spaces/${encodeURIComponent(ref)}#chat`
const box = (page: Page) => page.getByRole('textbox').first()
const sendButton = (page: Page) => page.getByRole('button', { name: /^(Send|Send til rommet)$/ })
const searchPanel = (page: Page) => page.getByRole('search', { name: /^(Søk i meldinger i rommet|Search messages in this room)$/ })
const searchInput = (page: Page) => page.getByRole('combobox', { name: /^(Søk i meldinger|Search messages)$/ })
const hitRows = (page: Page) => page.getByRole('listbox', { name: /^(Søketreff|Search results)$/ }).getByRole('option')

test('Ctrl+F finds a word by prefix, Enter on the full word opens the hit, and the message is highlighted and focused', async ({ browser, baseURL }) => {
  const ref = requireSpaceRef(test)
  requireMembers(test, 'A')
  const a = await memberContext(browser, baseURL, 'A')
  try {
    const page = await a.newPage()
    // One unique word, so the prefix query and the whole-word query both match only this post.
    const word = `zsearch${Date.now().toString(36)}`
    await page.goto(spaceUrl(ref))
    await box(page).fill(`Dette er ${word}`)
    await sendButton(page).click()
    await expect(page.getByText(word).first()).toBeVisible()

    await page.keyboard.press('Control+f')
    await expect(searchPanel(page)).toBeVisible()
    await expect(searchInput(page)).toBeFocused()

    // Typing a prefix lists the hit after the 250 ms debounce, without opening it.
    await searchInput(page).fill(word.slice(0, 9))
    await expect(hitRows(page).filter({ hasText: word }).first()).toBeVisible({ timeout: 15_000 })
    await expect(page).not.toHaveURL(/thread_id=/)

    // Enter on the full word runs a whole-word search at once and makes the first hit active.
    await searchInput(page).fill(word)
    await searchInput(page).press('Enter')
    await expect(hitRows(page).filter({ hasText: word }).first()).toHaveAttribute('data-active', 'true', { timeout: 15_000 })

    // A second Enter opens the hit: the URL names the thread and the message.
    await searchInput(page).press('Enter')
    await expect(page).toHaveURL(/[?&]thread_id=[^&]+/)
    await expect(page).toHaveURL(/[?&]m=[^&]+/)

    // The thread pane scrolls the message into view, highlights it, and moves focus onto it.
    await expect(page.locator('[data-message-id][data-highlight="true"]').first()).toBeVisible()
    expect(await page.evaluate(() => Boolean(document.activeElement?.closest('[data-message-id]')))).toBe(true)
  } finally {
    await a.close()
  }
})
