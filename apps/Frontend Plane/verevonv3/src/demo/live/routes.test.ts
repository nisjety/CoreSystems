/**
 * The route table, called directly: every endpoint the product calls on the
 * chat, runs and approvals answers with the envelope the product reads; the
 * features the demo switches off answer in Norwegian; a thread is never
 * answered as not kept (`retained: false` would make the product forget it).
 * No browser: the table is plain data and the answers are plain values.
 */
import { afterEach, describe, expect, it } from 'vitest'
import { buildHomeWorld } from '../fixtures/fjordform-home'
import { createStreamChannelFactory } from './channel'
import type { Scenario } from './scenario-types'
import { createResponder, type Responder } from './responder'
import { createLiveRoutes } from './routes'
import { scenarios, seededTranscripts } from './scenarios'
import { tilbudScenario } from './scenarios/tilbud-solhaug'
import { createThreadStore, type StoredMessage, type ThreadStore } from './thread-store'
import { DEMO_NOW_ISO, type DemoRoute, type DemoRouteContext, type DemoRouteMethod, type ThreadRow } from './shared-types'

const SEED: ThreadRow[] = [{
  threadId: 'thr_seed_1',
  title: 'Prisliste for 2027',
  preview: 'Utkast til prisliste ligger klart til gjennomgang.',
  updatedAt: '2026-10-08T09:00:00.000Z',
  pinned: false,
  personaEnabled: true,
}]

const TILBUD_QUESTION = tilbudScenario.suggestion.text

/** A real-time scheduler: the routes' streams are cancelled when each test disposes the engine. */
const realScheduler = {
  now: () => performance.now(),
  setTimeout: (callback: () => void, ms: number) => setTimeout(callback, ms),
  clearTimeout: (handle: unknown) => clearTimeout(handle as ReturnType<typeof setTimeout>),
}

type Harness = { routes: DemoRoute[]; store: ThreadStore; engine: Responder }

function harness(
  transcripts: Readonly<Record<string, readonly StoredMessage[]>> = {},
  seed: readonly ThreadRow[] = SEED,
): Harness {
  const store = createThreadStore(seed, transcripts)
  const engine = createResponder({
    store,
    scenarios,
    scheduler: realScheduler,
    openChannel: createStreamChannelFactory(realScheduler),
    wallNow: () => Date.parse('2026-10-09T12:00:00+02:00'),
  })
  const routes = createLiveRoutes({ store, engine, isoNow: () => '2026-10-09T10:00:00.000Z' })
  return { routes, store, engine }
}

type Answer = { status: number; body: unknown; contentType: string | null }

/** Calls the first route that claims the method and path, the way the interceptor does. */
async function call(routes: readonly DemoRoute[], method: DemoRouteMethod, path: string, body?: unknown): Promise<Answer | undefined> {
  const url = new URL(path, 'https://verevon.example/')
  const route = routes.find((candidate) => candidate.method === method && candidate.match.test(url.pathname))
  if (!route) return undefined
  const context: DemoRouteContext = {
    method,
    path: url.pathname,
    search: url.search,
    url,
    body,
    headers: new Headers(),
  }
  const result = await route.respond(context)
  if (result instanceof Response) {
    const contentType = result.headers.get('Content-Type')
    if (contentType?.startsWith('application/json')) {
      return { status: result.status, body: await result.json(), contentType }
    }
    await result.body?.cancel()
    return { status: result.status, body: null, contentType }
  }
  return { status: 200, body: result, contentType: 'application/json' }
}

let active: Harness | null = null

function start(transcripts?: Readonly<Record<string, readonly StoredMessage[]>>, seed?: readonly ThreadRow[]): Harness {
  active = harness(transcripts, seed)
  return active
}

afterEach(() => {
  active?.engine.dispose()
  active = null
})

describe('threads', () => {
  it('lists the seeded conversations in the envelope the product reads, not kept on the device', async () => {
    const { routes } = start()
    const answer = await call(routes, 'GET', '/api/v1/chat/threads')
    expect(answer?.status).toBe(200)
    expect(answer?.body).toEqual({ sessions: SEED, retention: { zdr: false } })
  })

  it('answers a snapshot write with the thread, and never as retained: false', async () => {
    const { routes } = start()
    const answer = await call(routes, 'PUT', '/api/v1/chat/threads/thr_seed_1', {
      title: 'Ny tittel',
      preview: 'Oppdatert',
      updatedAt: '2026-10-09T10:00:00.000Z',
      turns: [],
    })
    const body = answer?.body as { session: ThreadRow; retained?: unknown }
    expect(body.session.title).toBe('Ny tittel')
    expect(body.session.preview).toBe('Oppdatert')
    expect(body).not.toHaveProperty('retained')
  })

  it('answers a snapshot for a thread the demo does not hold with null, and creates nothing', async () => {
    const { routes, store } = start()
    const answer = await call(routes, 'PUT', '/api/v1/chat/threads/thr_unknown', { title: 'X' })
    expect(answer?.body).toEqual({ session: null })
    expect(store.get('thr_unknown')).toBeUndefined()
  })

  it('reads one thread, or answers 404 for an unknown one', async () => {
    const { routes } = start()
    expect((await call(routes, 'GET', '/api/v1/chat/threads/thr_seed_1'))?.body).toEqual({ session: SEED[0] })
    const missing = await call(routes, 'GET', '/api/v1/chat/threads/thr_nope')
    expect(missing?.status).toBe(404)
  })

  it('lists the messages of a thread, and 404s for a thread it does not hold', async () => {
    const { routes } = start()
    expect((await call(routes, 'GET', '/api/v1/chat/threads/thr_seed_1/messages'))?.body).toEqual({ messages: [] })
    expect((await call(routes, 'GET', '/api/v1/chat/threads/thr_nope/messages'))?.status).toBe(404)
  })

  it('a seeded thread opens with the transcript it was given, and a thread without one opens empty', async () => {
    const { routes } = start({
      thr_seed_1: [
        { id: 'msg_u', role: 'user', content: TILBUD_QUESTION, created_at: '2026-10-08T09:00:00.000Z' },
        { id: 'msg_a', role: 'assistant', content: 'Ja, vi rekker det.', created_at: '2026-10-08T09:00:00.000Z' },
      ],
    })
    const messages = await call(routes, 'GET', '/api/v1/chat/threads/thr_seed_1/messages')
    expect((messages?.body as { messages: Array<{ role: string }> }).messages.map((item) => item.role)).toEqual(['user', 'assistant'])
    expect((await call(routes, 'GET', '/api/v1/chat/threads/thr_other/messages'))?.status).toBe(404)
  })

  it('creates a thread, renames and pins it, and deletes it', async () => {
    const { routes, store } = start()
    const created = await call(routes, 'POST', '/api/v1/chat/threads', { title: 'Ny sak' })
    const session = (created?.body as { session: ThreadRow }).session
    expect(session.title).toBe('Ny sak')
    const pinned = await call(routes, 'PATCH', `/api/v1/chat/threads/${session.threadId}`, { pinned: true })
    expect((pinned?.body as { session: ThreadRow }).session.pinned).toBe(true)
    const deleted = await call(routes, 'DELETE', `/api/v1/chat/threads/${session.threadId}`)
    expect((deleted?.body as { sessions: ThreadRow[] }).sessions.map((row) => row.threadId)).toEqual(['thr_seed_1'])
    expect(store.get(session.threadId)).toBeUndefined()
  })

  it('clears every thread on the bulk delete', async () => {
    const { routes, store } = start()
    await call(routes, 'DELETE', '/api/v1/chat/threads')
    expect(store.sessions()).toEqual([])
  })

  it('answers the transcript with 404 (the product rebuilds from messages), and events and context empty', async () => {
    const { routes } = start()
    expect((await call(routes, 'GET', '/api/v1/chat/threads/thr_seed_1/transcript'))?.status).toBe(404)
    expect((await call(routes, 'GET', '/api/v1/chat/threads/thr_seed_1/events'))?.body).toEqual({
      events: [],
      truncated: false,
      has_more: false,
    })
    expect((await call(routes, 'GET', '/api/v1/chat/threads/thr_seed_1/context'))?.body).toEqual({
      thread_id: 'thr_seed_1',
      estimated_tokens: 0,
      budget_tokens: 0,
      segments: [],
    })
  })
})

describe('the answer, its controls and its runs', () => {
  it('streams the answer as text/event-stream', async () => {
    const { routes } = start()
    const answer = await call(routes, 'POST', '/api/v1/chat/stream', { content: TILBUD_QUESTION })
    expect(answer?.status).toBe(200)
    expect(answer?.contentType).toBe('text/event-stream')
  })

  it('answers a resume of a finished answer with 404, so the product settles the turn', async () => {
    const { routes } = start()
    const answer = await call(routes, 'GET', '/api/v1/chat/stream/resume/req_demo_1')
    expect(answer?.status).toBe(404)
    expect(answer?.body).toMatchObject({ error: { code: 'stream_not_resumable' } })
  })

  it('queues, stops and reports unknown invocations without an error', async () => {
    const { routes } = start()
    expect((await call(routes, 'POST', '/api/v1/chat/invocations/req_nope/queue', { content: 'Hei' }))?.body)
      .toEqual({ queued: false, resend_as_new_turn: true })
    expect((await call(routes, 'POST', '/api/v1/chat/invocations/req_nope/cancel', {}))?.body)
      .toEqual({ ok: true, cancelled: false })
  })

  it('reads a run and its approvals once a case has started, and 404s for an unknown run', async () => {
    const { routes } = start()
    await call(routes, 'POST', '/api/v1/chat/stream', { content: TILBUD_QUESTION })
    const run = await call(routes, 'GET', '/api/v1/agents/runs/run_demo_1')
    expect((run?.body as { run: { status: string; mode: string } }).run).toMatchObject({ status: 'running', mode: 'ask' })
    const approvals = await call(routes, 'GET', '/api/v1/orchestration/runs/run_demo_1/approvals')
    expect((approvals?.body as { approvals: unknown[] }).approvals).toHaveLength(1)
    expect((await call(routes, 'GET', '/api/v1/agents/runs/run_nope'))?.status).toBe(404)
    expect((await call(routes, 'GET', '/api/v1/runs/run_nope/events'))?.status).toBe(404)
  })

  it('answers a decision with its approval, and 404s for an unknown approval', async () => {
    const { routes } = start()
    await call(routes, 'POST', '/api/v1/chat/stream', { content: TILBUD_QUESTION })
    const decided = await call(routes, 'POST', '/api/v1/orchestration/approvals/apr_demo_1/decide', { decision: 'approve', reason: '' })
    expect((decided?.body as { approval: { status: string } }).approval.status).toBe('APPROVAL_STATE_GRANTED')
    expect((await call(routes, 'POST', '/api/v1/orchestration/approvals/apr_nope/decide', { decision: 'approve' }))?.status).toBe(404)
  })

  it('resumes a known run after its decision, and 404s for an unknown one', async () => {
    const { routes } = start()
    await call(routes, 'POST', '/api/v1/chat/stream', { content: TILBUD_QUESTION })
    expect((await call(routes, 'POST', '/api/v1/orchestration/runs/run_demo_1/resume', {}))?.body).toEqual({ ok: true, run_id: 'run_demo_1' })
    expect((await call(routes, 'POST', '/api/v1/orchestration/runs/run_nope/resume', {}))?.status).toBe(404)
  })

  it('cancels a run and reports it cancelled, and 404s for an unknown one', async () => {
    const { routes } = start()
    await call(routes, 'POST', '/api/v1/chat/stream', { content: TILBUD_QUESTION })
    expect((await call(routes, 'POST', '/api/v1/orchestration/runs/run_demo_1/cancel', {}))?.body)
      .toEqual({ run_id: 'run_demo_1', cancelled: true, status: 'cancelled' })
    expect((await call(routes, 'POST', '/api/v1/orchestration/runs/run_nope/cancel', {}))?.status).toBe(404)
  })

  it('answers the plan gate, and the empty plan, todo, lineage and proof reads', async () => {
    const { routes } = start()
    expect((await call(routes, 'POST', '/api/v1/agents/runs/run_demo_1/plan-approval', { decision: 'approve', granted_rung: 'read_only' }))?.body)
      .toEqual({ granted_rung: 'read_only', was_in_plan_mode: true, persisted: true })
    expect((await call(routes, 'GET', '/api/v1/orchestration/runs/run_demo_1/plans'))?.body).toEqual({ plans: [] })
    expect((await call(routes, 'GET', '/api/v1/orchestration/threads/thr_demo_1/todos'))?.body).toEqual({ todos: [] })
    expect((await call(routes, 'GET', '/api/v1/orchestration/threads/thr_demo_1/lineage'))?.body).toEqual({ nodes: [], edges: [] })
    expect((await call(routes, 'GET', '/api/v1/orchestration/runs/run_demo_1/proof-bundle'))?.body).toEqual({})
    expect((await call(routes, 'GET', '/api/v1/runs/run_demo_1/events/replay'))?.body).toEqual({
      run_id: 'run_demo_1',
      events: [],
      truncated: false,
    })
  })
})

describe('the features the demo does not run', () => {
  it('answers speech and audio in Norwegian with 503', async () => {
    const { routes } = start()
    for (const path of ['/api/v1/speech/transcribe', '/api/v1/audio/clip']) {
      const answer = await call(routes, 'POST', path, {})
      expect(answer?.status).toBe(503)
      expect(answer?.body).toMatchObject({ error: { code: 'demo_unavailable', message: 'Tale er slått av i demoen.' } })
    }
  })

  it('refuses scheduling writes in Norwegian with 403, and leaves the schedule reads to the fixtures', async () => {
    const { routes } = start()
    const refused = await call(routes, 'POST', '/api/v1/cron/jobs', {})
    expect(refused?.status).toBe(403)
    expect(refused?.body).toMatchObject({ error: { message: 'Planlegging er slått av i demoen.' } })
    expect(await call(routes, 'GET', '/api/v1/cron/jobs')).toBeUndefined()
  })

  it('does not claim the dashboard endpoints, which the home world answers', async () => {
    const { routes } = start()
    expect(await call(routes, 'GET', '/api/v1/navbar')).toBeUndefined()
    expect(await call(routes, 'GET', '/api/v1/notifications')).toBeUndefined()
  })
})

describe('requests the engine does not know', () => {
  it('leaves them unclaimed, so the shared fixtures and the guard answer them', async () => {
    const { routes } = start()
    expect(await call(routes, 'GET', '/api/v1/browser/sessions/run_demo_1/artifacts/art_demo_1')).toBeUndefined()
    expect(await call(routes, 'GET', '/api/v1/unknown/endpoint')).toBeUndefined()
  })
})

describe('the dashboard’s recent work and the cases', () => {
  it('seeds no case: the recent work lists none of the cases by title, names none of their customers or numbers, and seeds no transcript', () => {
    const { threadSeed } = buildHomeWorld(Date.parse(DEMO_NOW_ISO))
    const titles = threadSeed.map((row) => row.title)
    for (const scenario of scenarios) {
      expect(scenario.seedThreadId, scenario.id).toBeUndefined()
      expect(titles, scenario.id).not.toContain(scenario.title)
    }
    expect(JSON.stringify(threadSeed)).not.toMatch(/solhaug|havnebrygga|nordhavn|nordlys|4821|20471/i)
    expect(seededTranscripts(threadSeed)).toEqual({})
  })

  it('keeps the seeded-transcript option: a case that names a seeded thread opens with its question and answer', async () => {
    const seedRow: ThreadRow = { ...SEED[0]!, threadId: 'thr_seed_tilbud' }
    const seeded: Scenario = { ...tilbudScenario, seedThreadId: 'thr_seed_tilbud' }
    const transcripts = seededTranscripts([seedRow], [seeded])
    const { routes } = start(transcripts, [seedRow])
    const answer = await call(routes, 'GET', '/api/v1/chat/threads/thr_seed_tilbud/messages')
    const messages = (answer?.body as { messages: Array<{ role: string; content: string }> }).messages
    expect(messages.map((item) => item.role)).toEqual(['user', 'assistant'])
    expect(messages[0]?.content).toBe(TILBUD_QUESTION)
    expect(messages[1]?.content.length).toBeGreaterThan(20)
  })

  it('a case whose seeded thread is not in the seed gets no transcript', () => {
    const seeded: Scenario = { ...tilbudScenario, seedThreadId: 'thr_missing' }
    expect(seededTranscripts(SEED, [seeded])).toEqual({})
  })
})
