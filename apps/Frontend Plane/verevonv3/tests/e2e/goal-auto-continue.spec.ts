import { expect, test, type Page, type Route } from '@playwright/test'

/**
 * "Keep going" on the thread goal bar (EP-7 S4), in the browser against a mocked API.
 *
 * The chat stream, the goal read and the run's approvals are answered by this spec (`page.route`),
 * so no model is called. The mocked goal plays the judge: after each goal turn it reports one more
 * turn used and the verdict `not_met`, then `met` after the third. What is checked:
 *
 *  - with Keep going on, a completed goal turn is followed by a visible countdown and one more
 *    Continue that says it was pump-sent (`goal_auto: true` beside `goal_id`), until the goal is met;
 *  - an approval that waits on the person stops the pump, and nothing more is sent;
 *  - Stop cancels a countdown, and nothing more is sent.
 *
 * The goal bar needs the gateway's goal routes, which this spec mocks, so any dev server serving
 * this branch will do (5173 is the main checkout):
 *
 *   E2E_GOAL_AUTO=1 LOCAL_E2E_BASE_URL=http://localhost:5174 pnpm exec playwright test --project=chat tests/e2e/goal-auto-continue.spec.ts
 */

const ENABLED = process.env.E2E_GOAL_AUTO === '1'
const STREAM_PATH = '/api/v1/chat/stream'
const THREAD_ID = 'goal-thread-1'
const GOAL_ID = 'goal_auto_1'
const COMPOSER = 'form textarea'
/** Longer than the five-second countdown, so a send that was going to happen has happened. */
const PAST_COUNTDOWN_MS = 7_000

/** One server-sent event: `event:` names the frame, `data:` carries its JSON payload. */
function frame(event: string, data: Record<string, unknown>): string {
  return `event: ${event}\ndata: ${JSON.stringify(data)}\n\n`
}

/** A turn that answers and completes; `paused` leaves its run waiting on an approval instead. */
function turn(index: number, paused = false): string {
  const runId = `run_goal_${index}`
  return [
    frame('connected', { ok: true, request_id: `goal-req-${index}`, thread_id: THREAD_ID, run_id: runId }),
    frame('chunk', { delta: `Round ${index} of the work. ` }),
    ...(paused ? [frame('step_update', { id: runId, status: 'paused', title: 'Approval' })] : []),
    frame('done', { request_id: `goal-req-${index}`, done: true, model_used: 'balanced' }),
  ].join('')
}

const json = (route: Route, status: number, body: unknown) =>
  route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) })

type Mocks = {
  /** Every stream body the page sent. */
  bodies: Record<string, unknown>[]
  /** The goal turn (1-based) whose run waits on an approval, if any. */
  approvalOnGoalTurn?: number
  /** The run of that turn, once it was sent: the only run with an approval. */
  pausedRunId?: string
}

/** The goal turns the page sent: every stream body that names the goal. */
const goalTurns = (mocks: Mocks) => mocks.bodies.filter((body) => body.goal_id === GOAL_ID)

async function mockApi(page: Page, mocks: Mocks) {
  await page.route(`**${STREAM_PATH}`, (route) => {
    if (route.request().method() !== 'POST') return route.continue()
    const body = route.request().postDataJSON() as Record<string, unknown>
    mocks.bodies.push(body)
    const goalTurn = goalTurns(mocks).length
    const paused = body.goal_id === GOAL_ID && goalTurn === mocks.approvalOnGoalTurn
    if (paused) mocks.pausedRunId = `run_goal_${mocks.bodies.length}`
    return route.fulfill({ status: 200, contentType: 'text/event-stream', body: turn(mocks.bodies.length, paused) })
  })
  await page.route(`**/api/v1/chat/threads/${THREAD_ID}/goal`, (route) => {
    // The judge, played by the mock: one more turn used per goal turn, met after the third.
    const used = goalTurns(mocks).length
    const met = used >= 3
    return json(route, 200, {
      data: {
        found: true,
        autoContinueAvailable: true,
        goal: {
          id: GOAL_ID,
          threadId: THREAD_ID,
          objective: 'Find three EU suppliers with a website each',
          successCriteria: ['Three suppliers', 'A website for each'],
          status: met ? 'met' : 'active',
          maxTurns: 10,
          revision: 1,
          tokensUsed: 0,
          costUsedUsdMicros: 0,
          turnsUsed: used,
          ...(used > 0 ? { lastVerdict: met ? 'met' : 'not_met' } : {}),
          createdAt: '2026-11-04T08:00:00Z',
          updatedAt: '2026-11-04T08:00:00Z',
        },
      },
    })
  })
  await page.route('**/api/v1/orchestration/runs/*/approvals', (route) => {
    const runId = route.request().url().split('/runs/')[1]?.split('/')[0]
    const approvals =
      runId && runId === mocks.pausedRunId
        ? [
            {
              id: 'appr_goal_1',
              run_id: runId,
              kind: 'APPROVAL_KIND_TOOL_CALL',
              state: 'APPROVAL_STATE_REQUESTED',
              reason: 'open a supplier site',
            },
          ]
        : []
    return json(route, 200, { data: { approvals } })
  })
}

/** Opens a thread, waits for its goal bar and turns Keep going on. */
async function openGoal(page: Page) {
  await page.addInitScript(() => window.localStorage.setItem('verevon.locale', 'en'))
  await page.goto('/chat')
  const composer = page.locator(COMPOSER)
  await expect(composer).toBeVisible()
  await composer.fill('Find three EU suppliers')
  await page.locator('form button[type="submit"]').click()
  const bar = page.locator('.verevon-chat-goal-bar')
  await expect(bar).toBeVisible()
  const toggle = bar.getByRole('switch', { name: 'Keep going' })
  await toggle.click()
  await expect(toggle).toHaveAttribute('aria-checked', 'true')
  return bar
}

test.describe('goal Keep going (EP-7-S4)', () => {
  test.skip(!ENABLED, 'E2E_GOAL_AUTO=1 and a dev server serving this branch are needed')

  test('three goal turns, the last two sent by the pump, then the goal is met', async ({ page }) => {
    test.setTimeout(90_000)
    const mocks: Mocks = { bodies: [] }
    await mockApi(page, mocks)
    const bar = await openGoal(page)

    await bar.getByRole('button', { name: 'Continue', exact: true }).click()
    await expect(bar.getByRole('timer')).toContainText('Continuing in')
    await page.screenshot({ path: test.info().outputPath('goal-auto-countdown.png') })

    await expect.poll(() => goalTurns(mocks).length, { timeout: 20_000 }).toBe(3)
    const [first, second, third] = goalTurns(mocks)
    // The person's Continue carries no mark; each pump-sent turn says so, beside its goal.
    expect(first?.goal_auto).toBeUndefined()
    expect(second?.goal_auto).toBe(true)
    expect(third?.goal_auto).toBe(true)

    await expect(bar.locator('.verevon-chat-goal-bar__status')).toHaveAttribute('data-status', 'met')
    await page.waitForTimeout(PAST_COUNTDOWN_MS)
    expect(goalTurns(mocks)).toHaveLength(3)
  })

  test('an approval that waits on the person stops the pump', async ({ page }) => {
    test.setTimeout(60_000)
    const mocks: Mocks = { bodies: [], approvalOnGoalTurn: 2 }
    await mockApi(page, mocks)
    const bar = await openGoal(page)

    await bar.getByRole('button', { name: 'Continue', exact: true }).click()
    await expect.poll(() => goalTurns(mocks).length, { timeout: 15_000 }).toBe(2)
    await page.waitForTimeout(PAST_COUNTDOWN_MS)
    // The pump never sends past the approval, and never decides it.
    expect(goalTurns(mocks)).toHaveLength(2)
    await expect(bar.getByRole('timer')).toHaveCount(0)
  })

  test('Stop cancels a countdown', async ({ page }) => {
    test.setTimeout(60_000)
    const mocks: Mocks = { bodies: [] }
    await mockApi(page, mocks)
    const bar = await openGoal(page)

    await bar.getByRole('button', { name: 'Continue', exact: true }).click()
    await expect(bar.getByRole('timer')).toBeVisible()
    await bar.getByRole('button', { name: 'Stop', exact: true }).click()
    await expect(bar.getByRole('timer')).toHaveCount(0)
    await expect(bar.getByRole('switch', { name: 'Keep going' })).toHaveAttribute('aria-checked', 'false')
    await expect(bar.locator('.verevon-chat-goal-bar__auto-stopped')).toHaveAttribute('data-reason', 'user_stopped')
    await page.waitForTimeout(PAST_COUNTDOWN_MS)
    expect(goalTurns(mocks)).toHaveLength(1)
  })
})
