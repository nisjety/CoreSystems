import { expect, test, type Page } from '@playwright/test'
import { memberContext, requireMembers, requireSpaceRef, skipUnlessSpace, spaceTestModel } from './space-fixtures'

/**
 * WS08-S6 addressed wake, end to end (later tier: needs the seeded Space fixtures, human posts, the addressed-wake and
 * mention-picker flags on, and two bound agents). Member A addresses agent X then agent Y in one post, and they answer
 * in that order. Member B mentions an agent that only managers may call, and the wake board shows the refusal reason.
 *
 * The bound agents run on the fixture's explicit model (spaceTestModel: never a Verevon, budget or genius model).
 *
 * Fixture contract on top of space-fixtures.ts:
 *   E2E_SPACE_AGENT_X_NAME              display name of the first bound agent (answers first)
 *   E2E_SPACE_AGENT_Y_NAME              display name of the second bound agent (answers after X)
 *   E2E_SPACE_AGENT_MANAGERS_ONLY_NAME  a bound agent whose respond-to rule is managers only
 */

const AGENT_X = process.env.E2E_SPACE_AGENT_X_NAME?.trim() ?? ''
const AGENT_Y = process.env.E2E_SPACE_AGENT_Y_NAME?.trim() ?? ''
const AGENT_MANAGERS_ONLY = process.env.E2E_SPACE_AGENT_MANAGERS_ONLY_NAME?.trim() ?? ''
const RUN_BUDGET_MS = 300_000

const spaceUrl = (ref: string) => `/spaces/${encodeURIComponent(ref)}#chat`
const box = (page: Page) => page.getByRole('textbox').first()
const sendButton = (page: Page) => page.getByRole('button', { name: /^(Send|Send til rommet)$/ })
const wakeItem = (page: Page, name: string) => page.locator('.verevon-space-wake__item', { hasText: name }).first()
const escapeRegExp = (text: string) => text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')

/** Picks an agent from the mention list the way a member does: type "@", then choose the name. */
async function mention(page: Page, name: string): Promise<void> {
  await box(page).pressSequentially(`@${name.slice(0, 3)}`)
  await page.locator('.verevon-space-room-mentions').getByRole('option', { name: new RegExp(escapeRegExp(name)) }).first().click()
}

/** The wake state of one agent on the board, or '' while the agent has no item. */
async function stateOf(page: Page, name: string): Promise<string> {
  const item = wakeItem(page, name)
  return (await item.count()) === 0 ? '' : ((await item.getAttribute('data-state')) ?? '')
}

test('A addresses two agents in one post and they answer in order; B sees the refusal for a managers-only agent', async ({ browser, baseURL }) => {
  const ref = requireSpaceRef(test)
  requireMembers(test, 'A', 'B')
  skipUnlessSpace(test, !AGENT_X || !AGENT_Y || !AGENT_MANAGERS_ONLY, 'E2E_SPACE_AGENT_X_NAME, E2E_SPACE_AGENT_Y_NAME and E2E_SPACE_AGENT_MANAGERS_ONLY_NAME must name the bound fixture agents')
  spaceTestModel(test)
  const a = await memberContext(browser, baseURL, 'A')
  const b = await memberContext(browser, baseURL, 'B')
  try {
    const pageA = await a.newPage()
    const pageB = await b.newPage()
    await pageA.goto(spaceUrl(ref))
    await pageB.goto(spaceUrl(ref))

    const marker = Date.now().toString(36)
    await box(pageA).fill(`Svar i rekkefølge ${marker}, `)
    await mention(pageA, AGENT_X)
    await box(pageA).pressSequentially(' ')
    await mention(pageA, AGENT_Y)
    await box(pageA).pressSequentially(` ${marker}`)
    await sendButton(pageA).click()
    await expect(pageA.locator('.verevon-room-post', { hasText: marker })).toBeVisible()

    // X must answer before Y starts: sample both wake states until both are done.
    const samples: Array<{ x: string; y: string }> = []
    const deadline = Date.now() + RUN_BUDGET_MS
    while (Date.now() < deadline) {
      const sample = { x: await stateOf(pageA, AGENT_X), y: await stateOf(pageA, AGENT_Y) }
      samples.push(sample)
      if (sample.x === 'done' && sample.y === 'done') break
      await pageA.waitForTimeout(1_000)
    }
    const xDone = samples.findIndex((sample) => sample.x === 'done')
    const yStarted = samples.findIndex((sample) => sample.y === 'answering' || sample.y === 'done')
    expect(xDone, `X answered; last sample ${JSON.stringify(samples.at(-1))}`).toBeGreaterThanOrEqual(0)
    expect(yStarted === -1 || yStarted >= xDone, 'Y started only after X had answered').toBe(true)
    expect(samples.at(-1)).toEqual({ x: 'done', y: 'done' })

    // B mentions the managers-only agent: the board shows the refusal reason in words.
    await pageB.goto(spaceUrl(ref))
    await box(pageB).fill(`Kan du svare ${marker}? `)
    await mention(pageB, AGENT_MANAGERS_ONLY)
    await box(pageB).pressSequentially(` ${marker}`)
    await sendButton(pageB).click()
    await expect(wakeItem(pageB, AGENT_MANAGERS_ONLY)).toHaveAttribute('data-state', 'refused', { timeout: 60_000 })
    await expect(pageB.getByText(/does not answer messages from you in this room|svarer ikke på meldinger fra deg i dette rommet/)).toBeVisible()
  } finally {
    await a.close()
    await b.close()
  }
})
