/**
 * The live responder. It answers `POST /api/v1/chat/stream` with a scenario
 * turn and keeps what the product reads back: the threads, the runs and their
 * event logs, the approvals and what a decision sends. It holds no timers of
 * its own: time and channels are injected, so the whole flow is testable with
 * a manual clock (responder.test.ts).
 */
import type { ChannelFactory, LiveChannel, Scheduler } from './channel'
import { lastAt, paced, queuedInput, stopped } from './frames'
import { matchScenario, titleFromQuestion } from './match'
import type { ThreadStore } from './thread-store'
import { fallbackTurn, type FallbackReason } from './scenarios/fallback'
import type { Decision, Gate, Scenario, StreamFrame, TurnContext, TurnScript } from './scenario-types'

/** Pending approvals keep their chat channel open. This many of the newest stay open. */
const MAX_OPEN_APPROVALS = 3
/** Idle time after which a pending approval's chat channel closes. */
const APPROVAL_IDLE_MS = 15 * 60 * 1000
/** A turn without an approval closes this long after its last frame. */
const CLOSE_AFTER_MS = 40
/** A decision closes its channel this long after its last frame. */
const DECISION_CLOSE_MS = 60
/** A mid-run message is acknowledged in the stream, 600 ms and 900 ms after it arrives. */
const QUEUE_RECEIPT_MS = 600
const QUEUE_ACK_MS = 900
const QUEUE_ACK_TEXT = '*Notert. Demoen svarer bare på de forhåndsdefinerte sakene, så denne meldingen blir ikke besvart.*'
/** The model id every answer reports (a fixture id from fixtures/fjordform.ts). */
const MODEL = 'nordisk-standard'

export type ApprovalState = 'APPROVAL_STATE_REQUESTED' | 'APPROVAL_STATE_GRANTED' | 'APPROVAL_STATE_DENIED'

/** One approval as `GET /api/v1/orchestration/runs/:run/approvals` lists it. */
export type WireApproval = {
  id: string
  run_id: string
  kind: string
  status: ApprovalState
  detail: string
}

/** One run as `GET /api/v1/agents/runs/:run` reports it. */
export type WireRun = {
  run_id: string
  thread_id: string
  status: 'running' | 'completed' | 'cancelled'
  mode: 'ask' | 'plan'
  goal: string
  checkpoint_index: number
  steps_completed: number
  input_tokens: number
  output_tokens: number
}

export type QueueOutcome =
  | { queued: true; pending: number; persisted: true }
  | { queued: false; resend_as_new_turn: true }

export type Responder = {
  /** Starts a turn for a `POST /api/v1/chat/stream` body and returns its chat channel. */
  stream(body: unknown): LiveChannel
  /** A run's event channel. Frames already past go out at once, in order; later ones on schedule. */
  runEvents(runId: string): LiveChannel | undefined
  run(runId: string): WireRun | undefined
  approvals(runId: string): WireApproval[]
  /** Records a decision and sends its frames on the turn's open channel. Idempotent once decided. */
  decide(approvalId: string, decision: string): { approval: WireApproval } | undefined
  /** Whether the run exists (the product resumes a decided run after the decision). */
  resume(runId: string): boolean
  cancelRun(runId: string): { run_id: string; cancelled: true; status: 'cancelled' } | undefined
  /** Stops a turn: its chat channel gets `stopped` and closes. */
  cancelInvocation(requestId: string): boolean
  /** Delivers a mid-run message if the turn is still streaming; otherwise the product resends it as a new turn. */
  queue(requestId: string, content: string): QueueOutcome
  /** The plan-gate answer. The demo has no planner, so the grant is acknowledged as written. */
  planApproval(body: unknown): Record<string, unknown>
  /** Closes every channel, forgets every turn, run and approval, and restores the seeded threads. */
  reset(): void
  /** Closes every channel and forgets every turn, run and approval. The threads stay. */
  dispose(): void
}

export type ResponderOptions = {
  store: ThreadStore
  scenarios: readonly Scenario[]
  scheduler: Scheduler
  openChannel: ChannelFactory
  /** The fictional wall clock, in epoch milliseconds. */
  wallNow: () => number
}

type Turn = {
  requestId: string
  threadId: string
  runId: string
  postAt: number
  /** When the `done` frame is due, relative to the POST. Infinity when there is none. */
  doneAt: number
  /** When the turn's last scheduled chat frame is due, relative to the POST. */
  endAt: number
  channel: LiveChannel
  /** A gated turn's channel is closed by its approval's idle timeout, not by a timer. */
  gated: boolean
}

type Approval = {
  id: string
  runId: string
  threadId: string
  /** The stored assistant message the outcome is appended to. */
  messageId: string
  temporary: boolean
  kind: string
  detail: string
  state: ApprovalState
  gate: Gate
  chat: LiveChannel
}

type Run = {
  runId: string
  threadId: string
  postAt: number
  goal: string
  mode: 'ask' | 'do'
  status: 'running' | 'completed' | 'cancelled'
  /** Every run frame, with its time in ms after the POST, in order. */
  log: Array<{ atMs: number; item: StreamFrame }>
  subscribers: Set<LiveChannel>
}

export function createResponder(options: ResponderOptions): Responder {
  const { store, scenarios, scheduler, openChannel, wallNow } = options
  const turns = new Map<string, Turn>()
  const runs = new Map<string, Run>()
  const approvals = new Map<string, Approval>()
  const temporaryThreads = new Set<string>()
  let serial = 0

  const isoNow = () => new Date(wallNow()).toISOString()
  const elapsedSince = (postAt: number) => scheduler.now() - postAt

  /** Logs a run frame and sends it to every subscriber, at its time (relative to the POST). */
  function appendRun(run: Run, atMs: number, item: StreamFrame): void {
    run.log.push({ atMs, item })
    const delay = Math.max(0, atMs - elapsedSince(run.postAt))
    for (const subscriber of run.subscribers) subscriber.sendAfter(delay, item)
  }

  /** Only the newest approvals keep their channel open; older ones close (the decision is still recorded). */
  function capOpenApprovals(): void {
    const open = [...approvals.values()].filter((approval) => approval.state === 'APPROVAL_STATE_REQUESTED' && approval.chat.isOpen)
    for (const approval of open.slice(0, Math.max(0, open.length - MAX_OPEN_APPROVALS))) approval.chat.close()
  }

  function startTurn(body: unknown): LiveChannel {
    const request = recordOf(body)
    const content = textOf(request.content)
    const postAt = scheduler.now()
    serial += 1
    const requestedThread = textOf(request.thread_id).trim()
    const temporary = request.zdr === true
    const planMode = request.plan_mode === true
    const threadId = requestedThread || `thr_demo_${serial}`
    const existing = temporary ? undefined : store.get(threadId)
    const firstTurn = !existing || existing.messages.length === 0

    const reason = fallbackReasonOf(request)
    const match = reason === undefined ? matchScenario(content, scenarios) : null
    const ctx: TurnContext = {
      requestId: `req_demo_${serial}`,
      threadId,
      runId: `run_demo_${serial}`,
      approvalId: `apr_demo_${serial}`,
      artifactId: `art_demo_${serial}`,
      planMode,
      model: MODEL,
      firstTurn,
    }
    const script: TurnScript = match
      ? match.scenario.turn(ctx)
      : fallbackTurn({ ctx, reason: reason ?? 'free-text', question: content, offers: scenarios })
    const messageId = `msg_demo_${serial}`

    if (temporary) {
      temporaryThreads.add(threadId)
    } else {
      const now = isoNow()
      const title = match ? match.scenario.title : titleFromQuestion(content)
      const thread = store.ensure(threadId, { title, now })
      if (firstTurn) store.patch(threadId, { title })
      const lastUser = thread.messages.filter((message) => message.role === 'user').at(-1)
      const regenerated = request.regenerated === true && lastUser?.content === content
      if (!regenerated) {
        store.append(threadId, { id: `msg_demo_${serial}_u`, role: 'user', content, created_at: now })
      }
      store.append(threadId, { id: messageId, role: 'assistant', content: script.answer, created_at: now, model_used: MODEL })
      store.patch(threadId, { preview: plainPreview(script.answer), updatedAt: now })
    }

    if (match) {
      const run: Run = {
        runId: ctx.runId,
        threadId,
        postAt,
        goal: content,
        // A Do case without «Utfør» is an Ask turn: its run is an ask run.
        mode: planMode && match.scenario.mode === 'do' ? 'do' : 'ask',
        status: 'running',
        log: [...script.run].sort((a, b) => a.at - b.at).map((item) => ({ atMs: item.at, item })),
        subscribers: new Set(),
      }
      runs.set(run.runId, run)
    }

    const ordered = [...script.chat].sort((a, b) => a.at - b.at)
    const gated = script.gate !== undefined
    const channel = openChannel(`chat ${ctx.requestId}`, gated ? { idleCloseMs: APPROVAL_IDLE_MS } : {})
    for (const item of ordered) channel.sendAfter(item.at, item)
    const doneFrame = ordered.find((item) => item.event === 'done')
    const endAt = lastAt(ordered)
    turns.set(ctx.requestId, {
      requestId: ctx.requestId,
      threadId,
      runId: ctx.runId,
      postAt,
      doneAt: doneFrame?.at ?? Number.POSITIVE_INFINITY,
      endAt,
      channel,
      gated,
    })

    if (script.gate) {
      approvals.set(ctx.approvalId, {
        id: ctx.approvalId,
        runId: ctx.runId,
        threadId,
        messageId,
        temporary,
        kind: script.gate.kind,
        detail: script.gate.detail,
        state: 'APPROVAL_STATE_REQUESTED',
        gate: script.gate,
        chat: channel,
      })
      capOpenApprovals()
    } else {
      channel.closeAfter(endAt + CLOSE_AFTER_MS)
    }

    // Run frames are scheduled per subscriber from the log, so a panel that
    // connects later still sees them all, in order (see runEvents).
    return channel
  }

  return {
    stream(body) {
      return startTurn(body)
    },

    runEvents(runId) {
      const run = runs.get(runId)
      if (!run) return undefined
      const channel = openChannel(`run ${runId}`)
      const elapsed = elapsedSince(run.postAt)
      for (const entry of run.log) {
        const delay = entry.atMs - elapsed
        if (delay <= 0) channel.send(entry.item)
        else channel.sendAfter(delay, entry.item)
      }
      run.subscribers.add(channel)
      channel.onClose(() => {
        run.subscribers.delete(channel)
      })
      return channel
    },

    run(runId) {
      const run = runs.get(runId)
      if (!run) return undefined
      return {
        run_id: run.runId,
        thread_id: run.threadId,
        status: run.status,
        mode: run.mode === 'do' ? 'plan' : 'ask',
        goal: run.goal,
        checkpoint_index: 0,
        steps_completed: 0,
        input_tokens: 0,
        output_tokens: 0,
      }
    },

    approvals(runId) {
      return [...approvals.values()].filter((approval) => approval.runId === runId).map(wireApproval)
    },

    decide(approvalId, decision) {
      const approval = approvals.get(approvalId)
      if (!approval) return undefined
      if (approval.state === 'APPROVAL_STATE_REQUESTED') {
        const approved = decision === 'approve'
        approval.state = approved ? 'APPROVAL_STATE_GRANTED' : 'APPROVAL_STATE_DENIED'
        const outcome: Decision = approved ? approval.gate.approve : approval.gate.reject
        if (approval.chat.isOpen) {
          for (const item of outcome.chat) approval.chat.sendAfter(item.at, item)
          approval.chat.closeAfter(lastAt(outcome.chat) + DECISION_CLOSE_MS)
        }
        const run = runs.get(approval.runId)
        if (run) {
          const sinceStart = elapsedSince(run.postAt)
          for (const item of outcome.run) appendRun(run, sinceStart + item.at, item)
          run.status = outcome.runOutcome
        }
        if (!approval.temporary) store.extend(approval.threadId, approval.messageId, `\n\n${outcome.text}`)
      }
      return { approval: wireApproval(approval) }
    },

    resume(runId) {
      return runs.has(runId)
    },

    cancelRun(runId) {
      const run = runs.get(runId)
      if (!run) return undefined
      run.status = 'cancelled'
      for (const channel of [...run.subscribers]) channel.close()
      for (const approval of approvals.values()) {
        if (approval.runId !== runId || approval.state !== 'APPROVAL_STATE_REQUESTED') continue
        approval.state = 'APPROVAL_STATE_DENIED'
        approval.chat.close()
      }
      return { run_id: runId, cancelled: true, status: 'cancelled' }
    },

    cancelInvocation(requestId) {
      const turn = turns.get(requestId)
      if (!turn) return false
      if (turn.channel.isOpen) {
        turn.channel.send(stopped(0, { requestId, reason: 'cancelled' }))
        turn.channel.close()
      }
      return true
    },

    queue(requestId, content) {
      const turn = turns.get(requestId)
      const streaming = turn !== undefined && turn.channel.isOpen && elapsedSince(turn.postAt) < turn.doneAt
      if (!turn || !streaming) return { queued: false, resend_as_new_turn: true }
      const elapsed = elapsedSince(turn.postAt)
      turn.channel.sendAfter(QUEUE_RECEIPT_MS, queuedInput(QUEUE_RECEIPT_MS, content))
      const receipt = paced(QUEUE_ACK_MS, QUEUE_ACK_TEXT, requestId)
      for (const item of receipt) turn.channel.sendAfter(item.at, item)
      // A turn without a gate closes on a timer. Keep it open until both the
      // rest of its answer and the receipt have gone out; never earlier than before.
      if (!turn.gated) {
        const receiptEnd = elapsed + lastAt(receipt, QUEUE_ACK_MS)
        const closeAt = Math.max(turn.endAt, receiptEnd) + CLOSE_AFTER_MS
        turn.channel.closeAfter(closeAt - elapsed)
      }
      return { queued: true, pending: 1, persisted: true }
    },

    planApproval(body) {
      const request = recordOf(body)
      if (textOf(request.decision) === 'reject') return { was_in_plan_mode: true, persisted: true }
      return { granted_rung: textOf(request.granted_rung) || 'read_only', was_in_plan_mode: true, persisted: true }
    },

    reset() {
      closeEverything()
      store.reset()
    },

    dispose() {
      closeEverything()
    },
  }

  function closeEverything(): void {
    for (const turn of turns.values()) turn.channel.close()
    for (const approval of approvals.values()) approval.chat.close()
    for (const run of runs.values()) for (const channel of [...run.subscribers]) channel.close()
    turns.clear()
    runs.clear()
    approvals.clear()
    temporaryThreads.clear()
  }
}

/** The reason a message is answered by the fallback rather than a scenario, if any. */
function fallbackReasonOf(request: Record<string, unknown>): FallbackReason | undefined {
  if (Array.isArray(request.attachments) && request.attachments.length > 0) return 'attachments'
  if (request.generate_image === true) return 'image'
  return undefined
}

function recordOf(value: unknown): Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {}
}

function textOf(value: unknown): string {
  return typeof value === 'string' ? value : ''
}

function wireApproval(approval: Approval): WireApproval {
  return {
    id: approval.id,
    run_id: approval.runId,
    kind: approval.kind,
    status: approval.state,
    detail: approval.detail,
  }
}

/** The thread list's preview: the answer as one line of plain text, at most 140 characters. */
export function plainPreview(markdown: string): string {
  const plain = markdown
    .replace(/\[([^\]]+)\]\([^)]*\)/g, '$1')
    .replace(/[*_#>`[\]]/g, ' ')
    .replace(/\s+/g, ' ')
    .trim()
  return plain.length > 140 ? `${plain.slice(0, 139).trimEnd()}…` : plain
}
