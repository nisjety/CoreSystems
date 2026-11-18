import { expect, test, type Page, type Route } from '@playwright/test'

/**
 * WS09-S5 agent run card, in a mock. This needs only the Vite server at E2E_BASE_URL: every Space call is answered by
 * page.route, so there is no backend and no model. Member mock-b started run-1 on a thread of the room, so the card
 * shows for mock-a. The first live attach streams a snapshot and one step, then ends without `finished`, so the SPA
 * re-attaches with Last-Event-ID; the second attach sends `finished`. The card shows while the run is live and gives
 * way to the collapsed summary once it has finished.
 *
 * Frame names and fields follow space-run-live-client.ts. The stored work behind the collapsed summary follows the
 * transcript turn shape assumed here and is not yet checked against a live response.
 */

const REF = 'mock-room'
const MEMBER = 'mock-a'
const STARTER = 'mock-b'
const RUN = 'run-1'
const THREAD = 'thread-1'

type RunState = { attaches: number; finished: boolean }

const json = (route: Route, body: unknown) => route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(body) })
const frame = (event: string, id: number, data: unknown) => `id: ${id}\nevent: ${event}\ndata: ${JSON.stringify(data)}\n\n`

const snapshot = (id: number) =>
  frame('snapshot', id, {
    run_id: RUN,
    thread_id: THREAD,
    started_by: STARTER,
    agent_name: 'Mock-agent',
    phase: 'running',
    started_at: '2026-11-18T10:00:00Z',
    text: { offset: 0, value: '', truncated: false, visible: false },
    steps: [{ id: 'step-1', class: 'tool', state: 'running' }],
    steer: 0,
    plan: null,
    artifacts: [],
    expires_at: '2026-11-18T11:00:00Z',
  })
const stepDone = (id: number) => frame('step', id, { id: 'step-1', class: 'tool', state: 'ok', ms: 120 })
const finished = (id: number) => frame('finished', id, { outcome: 'completed', reason: 'answered' })

const CONTEXT = {
  space: { id: REF, space_id: REF, name: 'Mock-rommet', lifecycle: 'active' },
  membership: { role: 'owner', subject_id: MEMBER, org_id: 'mock-org' },
  features: { human_posts: true, thread_pane: true, timeline_windows: true, timeline_anchor: true },
  message_policy: null,
  limits: {},
}

function timeline(finishedRun: boolean) {
  return {
    page: { windowed: true, has_more: false, next_before: null, next_after: null, newest: 'c:1', total_posts: 1 },
    threads: [
      {
        thread_id: THREAD,
        space_id: REF,
        title: 'Spørsmål',
        preview: 'Spørsmål',
        updated_at: '2026-11-18T10:00:00Z',
        latest_run_id: RUN,
        latest_run_status: finishedRun ? 'completed' : 'running',
        owner_subject_id: STARTER,
        last_author_subject_id: STARTER,
        turn_count: 1,
        last_message_seq: 1,
        pinned: false,
      },
    ],
    pinned: [],
    refreshed: [],
    system_entries: [],
    system_truncated: false,
    membership: { role: 'owner', subject_id: MEMBER, org_id: 'mock-org' },
    pins: [],
    read_marker: null,
    shared_read: null,
    agent_work: { live: !finishedRun, stop_manager: false, text: false },
    unavailable: [],
  }
}

function transcript(finishedRun: boolean) {
  return {
    threadId: THREAD,
    turns: finishedRun
      ? [
          { role: 'user', content: 'Spørsmål', message_id: 'm-1', created_at: '2026-11-18T10:00:00Z', author_subject_id: STARTER, work: null },
          {
            role: 'assistant',
            content: 'Svar',
            message_id: 'm-2',
            created_at: '2026-11-18T10:00:05Z',
            agent_name: 'Mock-agent',
            work: { steps: [{ id: 'step-1', class: 'tool', state: 'ok', ms: 120 }], steer_count: 0, duration_ms: 1200 },
          },
        ]
      : [],
    window: { omitted_before: 0, has_more_before: false },
  }
}

async function mockRoom(page: Page, state: RunState): Promise<void> {
  // Registered first, so it is the lowest priority: anything not mocked below answers 404 and nothing hangs.
  await page.route(/\/api\/v1\//, (route) => route.fulfill({ status: 404, contentType: 'application/json', body: JSON.stringify({ error: { code: 'not_mocked' } }) }))
  await page.route(/\/api\/v1\/me$/, (route) => json(route, { data: { id: MEMBER, subject_id: MEMBER, org_id: 'mock-org', email: 'mock-a@example.test' } }))
  await page.route(new RegExp(`/api/v1/spaces/${REF}/context`), (route) => json(route, { data: CONTEXT }))
  await page.route(new RegExp(`/api/v1/spaces/${REF}/timeline`), (route) => json(route, { data: timeline(state.finished) }))
  await page.route(new RegExp(`/api/v1/spaces/${REF}/threads/${THREAD}/transcript`), (route) =>
    json(route, { data: { transcript: transcript(state.finished) } }),
  )
  await page.route(new RegExp(`/api/v1/spaces/${REF}/runs/${RUN}/live`), async (route) => {
    state.attaches += 1
    const body = state.attaches === 1 ? snapshot(1) + stepDone(2) : snapshot(3) + finished(4)
    if (state.attaches > 1) state.finished = true
    await route.fulfill({ status: 200, contentType: 'text/event-stream', body })
  })
  // No room stream in this mock: the room polls, and the run card is driven by its own attach above.
  await page.route(new RegExp(`/api/v1/spaces/${REF}/events`), (route) => route.abort())
  await page.route(new RegExp(`/api/v1/spaces/${REF}/presence`), (route) => json(route, { data: { present: [], ttl_seconds: 30 } }))
  await page.route(new RegExp(`/api/v1/spaces/${REF}/read`), (route) => json(route, { data: { last_read_at: null, manual_unread: false } }))
  await page.route(/\/api\/v1\/spaces\/attention/, (route) => json(route, { data: { rooms: [], totals: { badge: 0 } }, meta: { stale_sources: [] } }))
}

test('a run another member started shows a live card, then a collapsed summary once it finishes', async ({ page }) => {
  const state: RunState = { attaches: 0, finished: false }
  await mockRoom(page, state)
  await page.goto(`/spaces/${REF}#chat`)

  const card = page.locator('.verevon-run-card')
  await expect(card).toHaveAttribute('data-ended', 'false', { timeout: 30_000 })
  await expect(card).toHaveAttribute('aria-label', /^(Agent work|Agentens arbeid)$/)

  // The first attach ends without `finished`; the second brings it. Then the card leaves for the summary.
  await expect.poll(() => state.attaches, { timeout: 60_000 }).toBeGreaterThanOrEqual(2)
  await expect(card).toHaveCount(0, { timeout: 30_000 })
  await expect(page.locator('details.verevon-run-work > summary')).toContainText(/Brukte 1 verktøy|Used 1 tool/, { timeout: 30_000 })
})
