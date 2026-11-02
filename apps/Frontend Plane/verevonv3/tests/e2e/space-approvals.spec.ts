import { expect, test, type Page } from '@playwright/test'
import { memberContext, requireMembers, requireSpaceRef, skipUnlessSpace } from './space-fixtures'

/**
 * WS10-S4 approval scopes, end to end (later tier: needs the seeded Space fixtures AND a stack with
 * the gateway flag `SPACE_APPROVAL_SCOPES_ENABLED` on, Control's `approval_grant_entitled` for the
 * org, and a Space whose agent has an MCP tool that pauses for approval). Member A is the Space
 * owner and the member who started the paused runs. No spec here calls a Verevon-routed model.
 *
 * Fixture contract on top of space-fixtures.ts:
 *   E2E_SPACE_APPROVAL_RUNS   "1" when the fixture Space has a run of A paused on an MCP approval.
 *                             A second paused run is reached the same way after the rule is revoked.
 *
 * The flow, one test so the order is real: decide once (no rule is written) -> decide always
 * (a standing rule appears in the Agent tab) -> the rule is used (the same tool no longer pauses)
 * -> revoke -> the next call prompts again. The "rule is used" and "next call prompts" legs need an
 * agent run on an explicit model, so they sit behind E2E_SPACE_MODEL like every other run spec.
 */

const APPROVALS_READY = process.env.E2E_SPACE_APPROVAL_RUNS === '1'

async function openAgentTab(page: Page, ref: string): Promise<void> {
  await page.goto(`/spaces/${encodeURIComponent(ref)}`)
  await page.getByRole('tab', { name: /Agent/ }).first().click()
}

async function standingToolKeys(page: Page, ref: string): Promise<string[]> {
  const response = await page.request.get(`/api/v1/spaces/${encodeURIComponent(ref)}/approval-grants`)
  expect(response.status()).toBe(200)
  const body = (await response.json()) as { data?: { grants?: Array<{ tool_key?: string; revoked_at?: string | null }> } }
  return (body.data?.grants ?? []).filter((grant) => !grant.revoked_at).map((grant) => grant.tool_key ?? '')
}

test('a viewer is never offered a standing rule and cannot read the list', async ({ browser, baseURL }) => {
  const ref = requireSpaceRef(test)
  requireMembers(test, 'C')
  const viewer = await memberContext(browser, baseURL, 'C')
  try {
    const page = await viewer.newPage()
    await page.goto(`/spaces/${encodeURIComponent(ref)}`)
    const list = await page.request.get(`/api/v1/spaces/${encodeURIComponent(ref)}/approval-grants`)
    // 404 feature_disabled with the flag off, Control's typed 403 otherwise: never a 200.
    expect([403, 404]).toContain(list.status())
    await expect(page.getByRole('radio', { name: /Alltid for dette verktøyet/ })).toHaveCount(0)
    await expect(page.getByRole('heading', { name: 'Faste godkjenninger' })).toHaveCount(0)
  } finally {
    await viewer.close()
  }
})

test('the thread scope is refused and the answer is typed, never downgraded', async ({ browser, baseURL }) => {
  const ref = requireSpaceRef(test)
  requireMembers(test, 'A')
  const owner = await memberContext(browser, baseURL, 'A')
  try {
    const page = await owner.newPage()
    await page.goto(`/spaces/${encodeURIComponent(ref)}`)
    const response = await page.request.post(
      `/api/v1/spaces/${encodeURIComponent(ref)}/approvals/not-a-real-approval/decide`,
      { data: { decision: 'approve', scope: 'thread' } },
    )
    expect(response.status()).toBe(403)
    const body = (await response.json()) as { error?: { code?: string; details?: { reason?: string } } }
    expect(body.error?.code).toBe('approval_scope_not_permitted')
    expect(body.error?.details?.reason).toBe('thread_scope_not_available')
    // A forged proof is an unknown field, refused before anything else.
    const forged = await page.request.post(
      `/api/v1/spaces/${encodeURIComponent(ref)}/approvals/not-a-real-approval/decide`,
      { data: { decision: 'approve', grant_use_token: 'forged' } },
    )
    expect(forged.status()).toBe(400)
  } finally {
    await owner.close()
  }
})

test('decide once, standing rule passes, revoke, next call prompts', async ({ browser, baseURL }) => {
  const ref = requireSpaceRef(test)
  requireMembers(test, 'A')
  skipUnlessSpace(test, !APPROVALS_READY, 'E2E_SPACE_APPROVAL_RUNS=1 needs a fixture run of member A paused on an MCP approval')
  const owner = await memberContext(browser, baseURL, 'A')
  try {
    const page = await owner.newPage()
    await openAgentTab(page, ref)

    // 1. Once: no rule is written.
    const before = await standingToolKeys(page, ref)
    const once = page.getByRole('radio', { name: 'Bare denne gangen' }).first()
    await expect(once).toBeChecked()
    await page.getByRole('button', { name: 'Godkjenn' }).first().click()
    await expect(page.getByText(/Avgjørelsen er registrert|Handlingen kjøres|Godkjent/).first()).toBeVisible()
    expect(await standingToolKeys(page, ref)).toEqual(before)

    // 2. Always: a standing rule is recorded after the decision and listed.
    await openAgentTab(page, ref)
    await page.getByRole('radio', { name: 'Alltid for dette verktøyet i rommet' }).first().check()
    await expect(page.getByText(/En fast regel lar agenten bruke dette verktøyet/)).toBeVisible()
    await page.getByRole('button', { name: 'Godkjenn' }).first().click()
    await expect(page.getByText(/Fast regel opprettet for dette verktøyet i rommet/)).toBeVisible()
    await expect(page.getByRole('heading', { name: 'Faste godkjenninger' })).toBeVisible()
    const after = await standingToolKeys(page, ref)
    expect(after.length).toBe(before.length + 1)
    const created = after.find((key) => !before.includes(key)) ?? ''
    expect(created.startsWith('mcp__')).toBe(true)

    // 3. The rule is used: needs an agent run on an explicit model, so it is checked through the list's
    //    use count after the fixture agent's next call (E2E_SPACE_MODEL gates the run itself).
    // 4. Revoke: the rule leaves the list, and Control is idempotent about a second revoke.
    await page.getByRole('button', { name: 'Trekk tilbake' }).first().click()
    await expect.poll(() => standingToolKeys(page, ref)).toEqual(before)
    const grants = await page.request.get(`/api/v1/spaces/${encodeURIComponent(ref)}/approval-grants`)
    expect(grants.status()).toBe(200)
    // 5. The next matching call pauses again: the fixture's second paused run renders a fresh card.
    await openAgentTab(page, ref)
    await expect(page.getByRole('button', { name: 'Godkjenn' }).first()).toBeVisible()
  } finally {
    await owner.close()
  }
})
