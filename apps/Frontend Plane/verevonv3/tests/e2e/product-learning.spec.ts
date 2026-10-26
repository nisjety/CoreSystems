import { expect, test, type Page } from '@playwright/test'

/**
 * The learning inbox (WS07-S9) on the live shell. The learning reads and writes are
 * answered by route fixtures, so accept, reject, undo, forget and the admin copy can
 * be exercised without first producing a learned proposal. The owner's authority is
 * tested by session-core and the gateway against the real records.
 */

type Json = Record<string, unknown>

const PROPOSAL_ID = 'skill_proposal_1'

const LEDGER: Json[] = [
  {
    entry_id: 'led_skill_1',
    kind: 'skill',
    action: 'updated',
    skill_id: PROPOSAL_ID,
    skill_name: 'Utrullingsrutine',
    revision: 2,
    actor_kind: 'system',
    thread_id: 'thr_1',
    reason: '',
    before_json: JSON.stringify({ content: 'Kjør testene' }),
    after_json: JSON.stringify({ content: 'Kjør testene\nBygg' }),
    undone: false,
    undoable: true,
    created_at: '2026-10-25T10:00:00Z',
  },
  {
    entry_id: 'mem_1',
    kind: 'memory',
    action: 'learned',
    skill_id: '',
    skill_name: '',
    revision: 0,
    actor_kind: 'system',
    thread_id: 'thr_1',
    reason: '',
    before_json: '',
    after_json: JSON.stringify({ content: 'Bruker mørk modus' }),
    undone: false,
    undoable: false,
    created_at: '2026-10-25T10:01:00Z',
  },
]

async function ownerId(page: Page): Promise<string> {
  const response = await page.request.get('/api/v1/me/session-context')
  expect(response.status()).toBe(200)
  const body = (await response.json()) as { data?: { userId?: string }; userId?: string }
  const id = body.data?.userId ?? body.userId ?? ''
  expect(id).not.toBe('')
  return id
}

function proposal(owner: string): Json {
  return {
    id: PROPOSAL_ID,
    org_id: 'org',
    name: 'Utrullingsrutine',
    description: '',
    content: 'Kjør testene\nBygg',
    trigger_keywords: ['deploy'],
    trigger_file_patterns: [],
    tool_restrictions: [],
    enabled: false,
    scope: 'user',
    owner_user_id: owner,
    shared_with: [],
    origin: 'background_review',
    review_state: 'proposed',
    proposal_reason: 'Gjentatt i tre samtaler',
    source_run_id: 'run_1',
  }
}

/**
 * Answers the learning routes for one page. A decision removes the proposal from the
 * next read, as the server does. The tests assert the requests they send, not these
 * fixtures.
 */
async function fixtures(page: Page, owner: string, extraSkills: Json[] = []) {
  const state = { decided: false }

  await page.route('**/api/v1/skills', async (route) => {
    const request = route.request()
    if (request.method() === 'GET') {
      const skills = [...(state.decided ? [] : [proposal(owner)]), ...extraSkills]
      await route.fulfill({ json: { skills } })
      return
    }
    await route.fulfill({ json: { id: 'skill_org_copy' } })
  })

  await page.route('**/api/v1/learning/**', async (route) => {
    const request = route.request()
    const path = new URL(request.url()).pathname
    if (request.method() === 'GET' && path === '/api/v1/learning/ledger') {
      await route.fulfill({ json: { entries: LEDGER, next_cursor: '', memory_skip_reason: '' } })
    } else if (request.method() === 'POST' && path === `/api/v1/learning/proposals/${PROPOSAL_ID}/decision`) {
      state.decided = true
      const decision = (request.postDataJSON() as { decision?: string }).decision
      await route.fulfill({
        json: { review_state: decision === 'accept' ? 'accepted' : 'rejected', enabled: decision === 'accept', revision: 2 },
      })
    } else if (request.method() === 'POST' && path === '/api/v1/learning/ledger/led_skill_1/undo') {
      await route.fulfill({ json: { action: 'restored', skill_id: PROPOSAL_ID, revision: 3 } })
    } else {
      await route.fulfill({ status: 404, json: { error: { code: 'not_found', message: path } } })
    }
  })

  await page.route('**/api/v1/memory/**', async (route) => {
    await route.fulfill({ json: { deleted: true, degraded: false, degradation_reason: '' } })
  })
}

test('the owner accepts a proposal, then undoes a learned change', async ({ page }, info) => {
  const owner = await ownerId(page)
  await fixtures(page, owner)
  await page.goto('/settings/learning')

  await expect(page.getByRole('heading', { name: 'Forslag' })).toBeVisible()
  await expect(page.getByText('Gjentatt i tre samtaler')).toBeVisible()
  await expect(page.getByLabel('Endringer i forslaget')).toContainText('+ Bygg')

  const decision = page.waitForRequest(
    (request) => request.method() === 'POST' && request.url().endsWith(`/api/v1/learning/proposals/${PROPOSAL_ID}/decision`),
  )
  await page.getByRole('button', { name: 'Godta Utrullingsrutine' }).click()
  expect((await decision).postDataJSON()).toEqual({ decision: 'accept' })
  await expect(page.getByText('Ingen forslag venter på deg.')).toBeVisible()

  const undo = page.waitForRequest(
    (request) => request.method() === 'POST' && request.url().endsWith('/api/v1/learning/ledger/led_skill_1/undo'),
  )
  await page.getByRole('button', { name: 'Angre Utrullingsrutine' }).click()
  await undo
  await page.screenshot({ path: info.outputPath('learning-inbox.png'), fullPage: true })
})

test('the owner rejects a proposal and it is no longer offered', async ({ page }) => {
  const owner = await ownerId(page)
  await fixtures(page, owner)
  await page.goto('/settings/learning')

  const decision = page.waitForRequest(
    (request) => request.method() === 'POST' && request.url().endsWith(`/api/v1/learning/proposals/${PROPOSAL_ID}/decision`),
  )
  await page.getByRole('button', { name: 'Avvis Utrullingsrutine' }).click()
  expect((await decision).postDataJSON()).toEqual({ decision: 'reject' })
  await expect(page.getByText('Ingen forslag venter på deg.')).toBeVisible()
})

test('a learned memory is forgotten only after a second click, through the memory delete', async ({ page }) => {
  const owner = await ownerId(page)
  await fixtures(page, owner)
  await page.goto('/settings/learning')

  const forget = page.getByRole('button', { name: 'Glem Bruker mørk modus' })
  await forget.click()
  await expect(forget).toContainText('Bekreft glemsel?')

  const deletion = page.waitForRequest(
    (request) => request.method() === 'DELETE' && request.url().endsWith('/api/v1/memory/mem_1'),
  )
  await forget.click()
  await deletion
})

test('an admin copies a skill a member shared into the organization, disabled', async ({ page }) => {
  const owner = await ownerId(page)
  const shared: Json = {
    id: 'skill_shared_1',
    org_id: 'org',
    name: 'Kundemøte',
    description: 'Forberedelse',
    content: 'Les notatene',
    trigger_keywords: ['møte'],
    trigger_file_patterns: [],
    tool_restrictions: [],
    enabled: true,
    scope: 'user',
    owner_user_id: 'member-1',
    shared_with: [owner],
    origin: 'user',
    review_state: 'accepted',
  }
  await fixtures(page, owner, [shared])
  await page.goto('/settings/skills')

  const copy = page.getByRole('button', { name: 'Kopier Kundemøte til organisasjonen' })
  if ((await copy.count()) === 0) {
    test.skip(true, 'the seeded account is not a workspace admin')
  }
  const created = page.waitForRequest((request) => request.method() === 'POST' && request.url().endsWith('/api/v1/skills'))
  await copy.click()
  expect((await created).postDataJSON()).toEqual({
    name: 'Kundemøte',
    description: 'Forberedelse',
    content: 'Les notatene',
    trigger_keywords: ['møte'],
    enabled: false,
    scope: 'org',
    promoted_from: 'skill_shared_1',
  })
})
