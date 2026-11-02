import { createSignal } from 'solid-js'

import { streamChat, type ChatInvokeRequest } from '@/shared/api/chat-client'
import { beginOwnStream, endOwnStream } from './space-live-work'
import { newSpaceSendId } from './space-send-pipeline'

/**
 * The wake runner (WS08-S6). A human post that names one to three bound agents is stored first;
 * each `expected` agent is then woken from that stored post, one at a time, in the order the post
 * named them: one wake is one `/chat/stream` request carrying `wake:{message_id, client_wake_id}`,
 * and one run. The request's `content` is the stored post's text (the trigger the gateway signs
 * and Session Core checks by digest); the browser names the post and the agent and nothing else.
 *
 * - A thread has one live run at a time, so `thread_run_active` is not a failure: the wake waits
 *   `BUSY_RETRY_MS` and asks again, up to `BUSY_RETRIES` times, then says so.
 * - `client_wake_id` is minted once per (post, agent) and reused by every retry, so a replayed
 *   wake is one run; `managed_run_already_started` is the server saying it has it: done.
 * - The runner is module-level on purpose: leaving the room does not cancel the wakes still
 *   queued. Closing the tab does, and those agents simply stay `expected` on the server; the
 *   poster's browser shows "Wake now" for the ones this session interrupted.
 * - Authority is not here. The BFF mints the wake decision and strips forged fields, model-gateway
 *   checks the digest, Session Core verifies the signature, the owner gate and the trigger.
 */

export const BUSY_RETRY_MS = 3000
export const BUSY_RETRIES = 10

export type WakeState =
  /** Waiting for the agents named before it in the same post. */
  | 'queued'
  /** The thread has a live run; asking again shortly. */
  | 'waiting'
  | 'answering'
  | 'done'
  /** Recorded but not allowed to answer (`respond_to`, mention policy, paused, not the thread's owner). */
  | 'refused'
  | 'failed'
  /** This session stopped before it got to it; "Wake now" starts it. */
  | 'interrupted'

export interface WakeEntry {
  readonly messageId: string
  readonly threadId: string
  readonly spaceRef: string
  /** The stored post's text: the wake's trigger. */
  readonly content: string
  readonly agentSubjectId: string
  readonly agentName: string
  readonly ordinal: number
  readonly clientWakeId: string
  readonly state: WakeState
  /** The refusal reason (`respond_to_denied`, ...) or the error code behind `refused` / `failed`. */
  readonly code?: string
  /** Request hints kept so "Wake now" can run after the composer is gone. */
  readonly model?: ChatInvokeRequest['model']
  readonly minPrivacyTier?: ChatInvokeRequest['minPrivacyTier']
  readonly locale?: ChatInvokeRequest['locale']
}

export type WakePlanEntry = Pick<WakeEntry, 'agentSubjectId' | 'agentName' | 'ordinal'> & {
  /** Recorded as refused by the server: shown, never run. */
  readonly refusedReason?: string
}

export interface WakePlan {
  readonly messageId: string
  readonly threadId: string
  readonly spaceRef: string
  readonly content: string
  readonly agents: readonly WakePlanEntry[]
  readonly model?: ChatInvokeRequest['model']
  readonly minPrivacyTier?: ChatInvokeRequest['minPrivacyTier']
  readonly locale?: ChatInvokeRequest['locale']
}

/** Codes that mean "this agent may not answer this post": shown as a reason, not retried. */
const REFUSAL_CODES: ReadonlySet<string> = new Set([
  'agent_respond_to_denied',
  'agent_mention_not_enabled',
  'agent_invocation_blocked',
  'mentioned_agent_not_bound',
  'wake_owner_required',
])

/** Codes where asking again can succeed: shown with Retry. */
export const RETRYABLE_WAKE_CODES: ReadonlySet<string> = new Set([
  'space_decision_unavailable',
  'connection_error',
  'session_unavailable',
  'delegated_auth_unavailable',
])

export interface WakeBoard {
  readonly forMessage: (messageId: string) => readonly WakeEntry[]
  readonly all: () => readonly WakeEntry[]
  readonly set: (messageId: string, entries: readonly WakeEntry[]) => void
  readonly patch: (messageId: string, agentSubjectId: string, patch: Partial<WakeEntry>) => void
}

/**
 * What the timeline reads: in memory, per post, never stored. The data sits in a plain variable
 * with a version signal beside it, so the runner reads its own writes at once (Solid applies a
 * signal write at the next flush) while a component reading inside JSX still re-runs.
 */
export function createWakeBoard(): WakeBoard {
  let data: Readonly<Record<string, readonly WakeEntry[]>> = {}
  const [version, setVersion] = createSignal(0)
  const commit = (next: Readonly<Record<string, readonly WakeEntry[]>>) => {
    data = next
    setVersion((current) => current + 1)
  }
  return {
    forMessage: (messageId) => {
      version()
      return data[messageId] ?? []
    },
    all: () => {
      version()
      return Object.values(data).flat()
    },
    set: (messageId, next) => commit({ ...data, [messageId]: next }),
    patch: (messageId, agentSubjectId, patch) => {
      const list = data[messageId]
      if (!list) return
      commit({
        ...data,
        [messageId]: list.map((entry) => (entry.agentSubjectId === agentSubjectId ? { ...entry, ...patch } : entry)),
      })
    },
  }
}

/** The result of one wake request: no code is a finished run; a code is the typed reason it stopped. */
export interface WakeStreamResult {
  readonly code?: string
}

export interface WakeSequencerDeps {
  readonly board: WakeBoard
  readonly streamWake: (entry: WakeEntry, signal: AbortSignal) => Promise<WakeStreamResult>
  readonly sleep?: (ms: number, signal: AbortSignal) => Promise<void>
  readonly newWakeId?: () => string
}

export interface WakeSequencer {
  /** Record the plan on the board and run it. Refused agents are shown and skipped. */
  readonly start: (plan: WakePlan) => void
  /** Run whatever this session left `interrupted` for the post ("Wake now"). */
  readonly wakeNow: (messageId: string) => void
  /** Run one failed wake again under its own `clientWakeId`. */
  readonly retry: (messageId: string, agentSubjectId: string) => void
  /** Called after each wake settles and when a post's sequence ends, so the room re-reads. */
  readonly onSettled: (listener: () => void) => () => void
}

function defaultSleep(ms: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    if (signal.aborted) return resolve()
    const timer = setTimeout(done, ms)
    function done() {
      clearTimeout(timer)
      signal.removeEventListener('abort', done)
      resolve()
    }
    signal.addEventListener('abort', done)
  })
}

export function createWakeSequencer(deps: WakeSequencerDeps): WakeSequencer {
  const { board } = deps
  const sleep = deps.sleep ?? defaultSleep
  const newWakeId = deps.newWakeId ?? newSpaceSendId
  const running = new Map<string, AbortController>()
  const listeners = new Set<() => void>()
  const notify = () => {
    for (const listener of listeners) {
      try {
        listener()
      } catch {
        // A listener that throws must not stop the next wake.
      }
    }
  }

  async function runOne(entry: WakeEntry, signal: AbortSignal): Promise<'stop' | 'next'> {
    const patch = (next: Partial<WakeEntry>) => board.patch(entry.messageId, entry.agentSubjectId, next)
    for (let attempt = 0; attempt <= BUSY_RETRIES; attempt += 1) {
      patch({ state: 'answering', code: undefined })
      const result = await deps.streamWake(entry, signal)
      if (signal.aborted) {
        patch({ state: 'interrupted' })
        return 'stop'
      }
      const code = result.code
      if (!code || code === 'managed_run_already_started') {
        patch({ state: 'done', code: undefined })
        return 'next'
      }
      if (code === 'thread_run_active') {
        if (attempt === BUSY_RETRIES) {
          patch({ state: 'failed', code })
          return 'next'
        }
        patch({ state: 'waiting', code })
        await sleep(BUSY_RETRY_MS, signal)
        if (signal.aborted) {
          patch({ state: 'interrupted' })
          return 'stop'
        }
        continue
      }
      patch({ state: REFUSAL_CODES.has(code) ? 'refused' : 'failed', code })
      return 'next'
    }
    return 'next'
  }

  async function run(messageId: string): Promise<void> {
    if (running.has(messageId)) return
    const controller = new AbortController()
    running.set(messageId, controller)
    try {
      for (;;) {
        // Re-read the board each round: a retry or "Wake now" may have queued more behind us.
        const next = [...board.forMessage(messageId)]
          .sort((a, b) => a.ordinal - b.ordinal)
          .find((entry) => entry.state === 'queued')
        if (!next) break
        const outcome = await runOne(next, controller.signal)
        notify()
        if (outcome === 'stop') {
          // Whatever is still queued stays for "Wake now"; the server still holds those as expected.
          for (const entry of board.forMessage(messageId)) {
            if (entry.state === 'queued') board.patch(messageId, entry.agentSubjectId, { state: 'interrupted' })
          }
          break
        }
      }
    } finally {
      running.delete(messageId)
      notify()
    }
  }

  return {
    start(plan) {
      const entries: WakeEntry[] = [...plan.agents]
        .sort((a, b) => a.ordinal - b.ordinal)
        .map((agent) => ({
          messageId: plan.messageId,
          threadId: plan.threadId,
          spaceRef: plan.spaceRef,
          content: plan.content,
          agentSubjectId: agent.agentSubjectId,
          agentName: agent.agentName,
          ordinal: agent.ordinal,
          clientWakeId: newWakeId(),
          state: agent.refusedReason ? ('refused' as const) : ('queued' as const),
          ...(agent.refusedReason ? { code: agent.refusedReason } : {}),
          ...(plan.model ? { model: plan.model } : {}),
          ...(plan.minPrivacyTier ? { minPrivacyTier: plan.minPrivacyTier } : {}),
          ...(plan.locale ? { locale: plan.locale } : {}),
        }))
      board.set(plan.messageId, entries)
      void run(plan.messageId)
    },
    wakeNow(messageId) {
      for (const entry of board.forMessage(messageId)) {
        if (entry.state === 'interrupted') board.patch(messageId, entry.agentSubjectId, { state: 'queued' })
      }
      void run(messageId)
    },
    retry(messageId, agentSubjectId) {
      const entry = board.forMessage(messageId).find((item) => item.agentSubjectId === agentSubjectId)
      if (!entry || entry.state !== 'failed') return
      board.patch(messageId, agentSubjectId, { state: 'queued', code: undefined })
      void run(messageId)
    },
    onSettled(listener) {
      listeners.add(listener)
      return () => {
        listeners.delete(listener)
      }
    },
  }
}

/** Opens one wake stream and reports how it ended. The answer itself reaches the room through its projection. */
async function streamWakeRequest(entry: WakeEntry, signal: AbortSignal): Promise<WakeStreamResult> {
  let code: string | undefined
  beginOwnStream({ spaceRef: entry.spaceRef, threadId: entry.threadId })
  try {
    await streamChat(
      {
        content: entry.content,
        spaceRef: entry.spaceRef,
        threadId: entry.threadId,
        mentionedAgentRef: entry.agentSubjectId,
        wake: { messageId: entry.messageId, clientWakeId: entry.clientWakeId },
        ...(entry.model ? { model: entry.model } : {}),
        ...(entry.minPrivacyTier ? { minPrivacyTier: entry.minPrivacyTier } : {}),
        ...(entry.locale ? { locale: entry.locale } : {}),
      },
      {
        // A refusal before the run starts, or a run that failed: the typed code is the whole story.
        onError: (event) => {
          code = event.code
        },
      },
      signal,
    )
  } catch (error) {
    if (!(error instanceof DOMException && error.name === 'AbortError') && !signal.aborted) code = 'connection_error'
  } finally {
    endOwnStream()
  }
  return code ? { code } : {}
}

/** The board the timeline reads and the runner every composer shares. */
export const spaceWakeBoard: WakeBoard = createWakeBoard()
export const spaceWakeSequencer: WakeSequencer = createWakeSequencer({
  board: spaceWakeBoard,
  streamWake: streamWakeRequest,
})
