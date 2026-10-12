/**
 * The engine's routes: every chat-thread, stream, run, approval and
 * orchestration endpoint the demo answers, plus the features the demo switches
 * off. They are matched on the path alone, in the order listed. The home
 * world's routes come after these (index.ts), and the shared fixtures last.
 *
 * Answers follow the shapes the product reads (see the client files named in
 * the comments). A thread is never answered as `retained: false`: that makes
 * the product forget it on the device.
 */
import type { DemoRoute, DemoRouteContext, DemoRouteMethod } from './shared-types'
import type { Responder } from './responder'
import type { ThreadStore } from './thread-store'

export type LiveRouteOptions = {
  store: ThreadStore
  engine: Responder
  /** The fictional clock as ISO 8601, for threads the routes create. */
  isoNow: () => string
}

const THREAD = /^\/api\/v1\/chat\/threads\/([^/]+)$/
const THREAD_MESSAGES = /^\/api\/v1\/chat\/threads\/([^/]+)\/messages$/
const THREAD_TRANSCRIPT = /^\/api\/v1\/chat\/threads\/([^/]+)\/transcript$/
const THREAD_EVENTS = /^\/api\/v1\/chat\/threads\/([^/]+)\/events$/
const THREAD_CONTEXT = /^\/api\/v1\/chat\/threads\/([^/]+)\/context$/
const STREAM_RESUME = /^\/api\/v1\/chat\/stream\/resume\/([^/]+)$/
const INVOCATION_QUEUE = /^\/api\/v1\/chat\/invocations\/([^/]+)\/queue$/
const INVOCATION_CANCEL = /^\/api\/v1\/chat\/invocations\/([^/]+)\/cancel$/
const RUN_EVENTS = /^\/api\/v1\/runs\/([^/]+)\/events$/
const RUN_REPLAY = /^\/api\/v1\/runs\/([^/]+)\/events\/replay$/
const RUN_WATCHERS = /^\/api\/v1\/runs\/([^/]+)\/watchers$/
const AGENT_RUN = /^\/api\/v1\/agents\/runs\/([^/]+)$/
const PLAN_APPROVAL = /^\/api\/v1\/agents\/runs\/([^/]+)\/plan-approval$/
const RUN_APPROVALS = /^\/api\/v1\/orchestration\/runs\/([^/]+)\/approvals$/
const APPROVAL_DECIDE = /^\/api\/v1\/orchestration\/approvals\/([^/]+)\/decide$/
const RUN_RESUME = /^\/api\/v1\/orchestration\/runs\/([^/]+)\/resume$/
const RUN_CANCEL = /^\/api\/v1\/orchestration\/runs\/([^/]+)\/cancel$/
const RUN_PLANS = /^\/api\/v1\/orchestration\/runs\/([^/]+)\/plans$/
const THREAD_TODOS = /^\/api\/v1\/orchestration\/threads\/([^/]+)\/todos$/
const THREAD_LINEAGE = /^\/api\/v1\/orchestration\/threads\/([^/]+)\/lineage$/
const PROOF_BUNDLE = /^\/api\/v1\/orchestration\/runs\/([^/]+)\/proof-bundle$/
const SPEECH = /^\/api\/v1\/(speech|audio)(\/|$)/
const CRON = /^\/api\/v1\/cron(\/|$)/

const WRITE_METHODS = ['POST', 'PUT', 'PATCH', 'DELETE'] as const
const ANY_METHOD = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE'] as const

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })
}

function failure(status: number, code: string, message: string): Response {
  return json({ error: { code, message } }, status)
}

/** The path parameter a route's pattern captured (decoded). */
function param(ctx: DemoRouteContext, pattern: RegExp): string {
  return decodeURIComponent(ctx.path.match(pattern)?.[1] ?? '')
}

function bodyOf(ctx: DemoRouteContext): Record<string, unknown> {
  return ctx.body !== null && typeof ctx.body === 'object' && !Array.isArray(ctx.body)
    ? (ctx.body as Record<string, unknown>)
    : {}
}

function textOf(value: unknown): string {
  return typeof value === 'string' ? value : ''
}

export function createLiveRoutes(options: LiveRouteOptions): DemoRoute[] {
  const { store, engine, isoNow } = options
  const routes: DemoRoute[] = []
  let created = 0

  const add = (method: DemoRouteMethod, match: RegExp, respond: DemoRoute['respond']) => {
    routes.push({ method, match, respond })
  }

  /** The listing row of one thread, as the product reads it. */
  const sessionOf = (threadId: string) => store.sessions().find((row) => row.threadId === threadId)

  // ── Threads (src/shared/api/chat-client.ts) ──────────────────────────────
  add('GET', /^\/api\/v1\/chat\/threads$/, () => ({ sessions: store.sessions(), retention: { zdr: false } }))

  add('POST', /^\/api\/v1\/chat\/threads$/, (ctx) => {
    created += 1
    const threadId = `thr_demo_new_${created}`
    store.ensure(threadId, { title: textOf(bodyOf(ctx).title).trim() || 'Ny samtale', now: isoNow() })
    return { session: sessionOf(threadId) ?? null }
  })

  add('DELETE', /^\/api\/v1\/chat\/threads$/, () => {
    store.clear()
    return { ok: true, sessions: [], retention: { zdr: false } }
  })

  add('GET', THREAD, (ctx) => {
    const session = sessionOf(param(ctx, THREAD))
    return session ? { session } : failure(404, 'thread_not_found', 'Samtalen finnes ikke i demoen.')
  })

  // The product's snapshot write. A thread the demo does not hold is not
  // created here (a temporary chat is never stored), so the answer is null.
  add('PUT', THREAD, (ctx) => {
    const threadId = param(ctx, THREAD)
    const body = bodyOf(ctx)
    store.patch(threadId, {
      ...(typeof body.title === 'string' ? { title: body.title } : {}),
      ...(typeof body.preview === 'string' && body.preview.trim() ? { preview: body.preview } : {}),
      ...(typeof body.updatedAt === 'string' ? { updatedAt: body.updatedAt } : {}),
    })
    return { session: sessionOf(threadId) ?? null }
  })

  add('PATCH', THREAD, (ctx) => {
    const threadId = param(ctx, THREAD)
    const body = bodyOf(ctx)
    store.patch(threadId, {
      ...(typeof body.title === 'string' ? { title: body.title } : {}),
      ...(typeof body.pinned === 'boolean' ? { pinned: body.pinned } : {}),
    })
    const session = sessionOf(threadId)
    return session ? { session } : failure(404, 'thread_not_found', 'Samtalen finnes ikke i demoen.')
  })

  add('DELETE', THREAD, (ctx) => {
    store.remove(param(ctx, THREAD))
    return { sessions: store.sessions(), retention: { zdr: false } }
  })

  add('GET', THREAD_MESSAGES, (ctx) => {
    const thread = store.get(param(ctx, THREAD_MESSAGES))
    if (!thread) return failure(404, 'thread_not_found', 'Samtalen finnes ikke i demoen.')
    return {
      messages: thread.messages.map((message) => ({
        id: message.id,
        role: message.role,
        content: message.content,
        created_at: message.created_at,
        model_used: message.model_used,
      })),
    }
  })

  add('GET', THREAD_TRANSCRIPT, () => failure(404, 'transcript_not_found', 'Ingen avskrift i demoen.'))

  add('GET', THREAD_EVENTS, () => ({ events: [], truncated: false, has_more: false }))

  add('GET', THREAD_CONTEXT, (ctx) => ({
    thread_id: param(ctx, THREAD_CONTEXT),
    estimated_tokens: 0,
    budget_tokens: 0,
    segments: [],
  }))

  // ── The answer and its controls (src/shared/api/chat-client.ts) ──────────
  add('POST', /^\/api\/v1\/chat\/stream$/, (ctx) => {
    const channel = engine.stream(ctx.body)
    return channel.response ?? failure(500, 'demo_error', 'Svaret kunne ikke startes i demoen.')
  })

  // The buffer of a finished or reloaded answer is gone: the product settles
  // the turn with the text it already has.
  add('GET', STREAM_RESUME, () => failure(404, 'stream_not_resumable', 'Svaret kan ikke gjenopptas i demoen.'))

  add('POST', INVOCATION_QUEUE, (ctx) => engine.queue(param(ctx, INVOCATION_QUEUE), textOf(bodyOf(ctx).content).trim()))

  add('POST', INVOCATION_CANCEL, (ctx) => ({
    ok: true,
    cancelled: engine.cancelInvocation(param(ctx, INVOCATION_CANCEL)),
  }))

  // ── Runs and approvals (src/shared/api/run-console-client.ts, orchestration-client.ts) ──
  add('GET', RUN_EVENTS, (ctx) => {
    const channel = engine.runEvents(param(ctx, RUN_EVENTS))
    return channel?.response ?? failure(404, 'run_not_found', 'Kjøringen finnes ikke i demoen.')
  })

  add('GET', RUN_REPLAY, (ctx) => ({ run_id: param(ctx, RUN_REPLAY), events: [], truncated: false }))

  add('GET', RUN_WATCHERS, (ctx) => ({ run_id: param(ctx, RUN_WATCHERS), watching: false, watchers: [] }))

  add('GET', AGENT_RUN, (ctx) => {
    const run = engine.run(param(ctx, AGENT_RUN))
    return run ? { run } : failure(404, 'run_not_found', 'Kjøringen finnes ikke i demoen.')
  })

  add('POST', PLAN_APPROVAL, (ctx) => engine.planApproval(ctx.body))

  add('GET', RUN_APPROVALS, (ctx) => ({ approvals: engine.approvals(param(ctx, RUN_APPROVALS)) }))

  add('POST', APPROVAL_DECIDE, (ctx) => {
    const result = engine.decide(param(ctx, APPROVAL_DECIDE), textOf(bodyOf(ctx).decision))
    return result ?? failure(404, 'approval_not_found', 'Godkjenningen finnes ikke i demoen.')
  })

  add('POST', RUN_RESUME, (ctx) => {
    const runId = param(ctx, RUN_RESUME)
    return engine.resume(runId) ? { ok: true, run_id: runId } : failure(404, 'run_not_found', 'Kjøringen finnes ikke i demoen.')
  })

  add('POST', RUN_CANCEL, (ctx) => engine.cancelRun(param(ctx, RUN_CANCEL)) ?? failure(404, 'run_not_found', 'Kjøringen finnes ikke i demoen.'))

  add('GET', RUN_PLANS, () => ({ plans: [] }))

  add('GET', THREAD_TODOS, () => ({ todos: [] }))

  add('GET', THREAD_LINEAGE, () => ({ nodes: [], edges: [] }))

  add('GET', PROOF_BUNDLE, () => ({}))

  // ── Features the demo does not run ───────────────────────────────────────
  for (const method of ANY_METHOD) {
    add(method, SPEECH, () => failure(503, 'demo_unavailable', 'Tale er slått av i demoen.'))
  }
  for (const method of WRITE_METHODS) {
    add(method, CRON, () => failure(403, 'demo_unavailable', 'Planlegging er slått av i demoen.'))
  }

  return routes
}
