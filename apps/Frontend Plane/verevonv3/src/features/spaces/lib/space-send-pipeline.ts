import { createSignal } from 'solid-js'

import type { ChatArtifact } from '@/features/chat/components/chat-types'
import { roomArtifactFromEvent } from './room-artifacts'
import type { SubmitPrivacyTier, TurnPrivacyTierResolver } from '@/features/dashboard/home/submit-privacy-tier'
import type { AiModelSelection } from '@/shared/ai/model-selection'
import {
  cancelInvocation,
  resumeStream,
  streamChat,
  type ChatErrorEvent,
  type ChatStreamHandlers,
} from '@/shared/api/chat-client'
import { OPENAI_CODEX_SUBSCRIPTION_PROVIDER } from '@/shared/api/chatgpt-subscription-client'
import type { Skill } from '@/shared/api/skills-client'
import { updateSpaceThreadPresentation } from '@/shared/api/spaces-client'
import { beginOwnStream, endOwnStream } from './space-live-work'
import type { SpaceComposerModeId } from './space-composer-registry'
import {
  noteSendTransportLost,
  noteSendTransportOk,
  spaceSendIdempotencyEnabled,
} from './space-connectivity'

/**
 * Reliable send on the agent path (WS01-S8, F-t1-033). With the send key on, one
 * send is named by one `sendId` for its whole life, retries included:
 *
 * - `sendId` rides as `idempotency_key` (the gateway's retry identity of a send;
 *   a repeat of it never dispatches a second run and, with send receipts on, is
 *   one transcript row);
 * - a send that opens a NEW thread also carries `session_key: space-post-{sendId}`,
 *   so a repeat resolves to the thread the first one created instead of minting a
 *   second (Session Core's create-thread is get-or-create by session key). A reply
 *   keeps the thread id as its session key, as before;
 * - a stream that drops after the server announced it is resumed by request id
 *   (replay from the last frame seen, never from a guess), a bounded number of
 *   times, before the send is called failed;
 * - `managed_run_already_started` is the server saying "I already have this send":
 *   that is acceptance, not an error, and the room's projection shows the run.
 *
 * The id matches the `client_send_id` shape the human-post routes take
 * (`^[A-Za-z0-9_-]{16,64}$`), so one id can name a send on either path.
 */
const MAX_RESUMES = 3
const RESUME_BACKOFF_MS = [1000, 2000, 4000]

/** The stable name of one send: 32 hex characters, no dashes. */
export function newSpaceSendId(): string {
  try {
    return crypto.randomUUID().replace(/-/g, '')
  } catch {
    return `${Date.now().toString(36)}${Math.random().toString(36).slice(2, 14)}${Math.random().toString(36).slice(2, 14)}`.padEnd(16, '0')
  }
}

/** The session key of a send that opens a new thread. */
export function spaceSendSessionKey(sendId: string): string {
  return `space-post-${sendId}`
}

function waitOrAbort(ms: number, signal: AbortSignal): Promise<void> {
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

/**
 * The room composer's send pipeline (P-SPA2): everything between "the member
 * pressed Send" and "the exchange block shows the outcome", moved out of
 * `SpaceRoomComposer.tsx` unchanged so later slices extend a seam instead of
 * the component body.
 *
 * One strategy per composer mode (`space-composer-registry.ts`) says how the
 * message leaves the box; `ask` is today's behaviour, a model turn streamed
 * through `streamChat`. A slice that adds a mode adds one entry to
 * `SEND_STRATEGIES` (the `Record` over the mode ids makes a missing one a type
 * error). The pipeline owns the in-flight handles (abort controller, request
 * id, thread id), the exchange state, the own-stream marker and Stop.
 *
 * The behaviour a turn must keep is documented on the composer: a Stop aborts
 * the read AND cancels by `requestId` (only the cancel is recorded), a
 * generated title is written back through the room's presentation route only
 * for a thread this browser opened, and a `done` never overwrites a landed stop.
 */

export type SpaceExchange = {
  readonly userContent: string
  assistantContent: string
  /**
   * `stopped` is its own state, not an error and not done. A server-side stop
   * used to reach `onDone` and render a halted answer as a finished one; the
   * transport now emits `stopped` separately, and this composer keeps the
   * distinction all the way to the screen.
   */
  status: 'streaming' | 'done' | 'error' | 'stopped'
  errorMessage?: string
  /**
   * The gateway's error code, kept beside the message so a refusal the room
   * can act on — the spend hard stop — renders as what it is, with the way
   * out, rather than as a generic "the reply stopped".
   */
  errorCode?: string
  /** Whether the stop was recorded server-side, or only cut the local read. */
  stopRecorded?: boolean
  /**
   * The send can be repeated under its own key (`pipeline.retry`): set only when the
   * transport failed and the outcome is unknown, never for an answer the server gave.
   */
  retryable?: boolean
  /**
   * The server already had this send (`managed_run_already_started`): it is accepted,
   * its run is in the room's projection, and there is no stream to show here.
   */
  acceptedEarlier?: boolean
}

/** What the composer hands over when a message is sent. */
export interface SpaceSendInput {
  readonly content: string
  readonly mode: SpaceComposerModeId
  readonly model: AiModelSelection
  /** The Control `subject_id` of a mentioned, bound agent, when one was picked. */
  readonly mentionedAgentRef?: string
  readonly skills: readonly Skill[]
  /** Set when the message continues an existing room thread. */
  readonly replyThreadId?: string
  /**
   * The name of this send. Left out by the composer (the pipeline mints one);
   * `retry` passes the first attempt's id back so the repeat is the same send.
   */
  readonly sendId?: string
}

/**
 * Stages a listener may observe. `started`: the exchange block is up and the
 * box may be cleared. `accepted`: the server announced the turn (`connected`).
 * `settled`: the send ended, however it ended.
 */
export type SpaceSendStage = 'started' | 'accepted' | 'settled'

/** What a strategy gets for one send. */
export interface SpaceSendRun {
  readonly input: SpaceSendInput
  /** Set only while the send key is on (`spaceSendIdempotencyEnabled`); otherwise the send is keyless as before. */
  readonly sendId?: string
  readonly spaceRef: string
  readonly signal: AbortSignal
  /** The turn's privacy tier for a selection; may wait (bounded) for the catalogue. */
  readonly resolveTier: (model: string, provider: string | undefined) => Promise<SubmitPrivacyTier>
  /** The pipeline's transport handlers: a strategy that streams passes them on. */
  readonly handlers: ChatStreamHandlers
}

export type SpaceSendStrategy = (run: SpaceSendRun) => Promise<void>

/** `ask`: a model turn streamed into the room. */
const askStrategy: SpaceSendStrategy = async (run) => {
  const { input } = run
  const modelSelection = input.model
  // May wait (bounded) for a catalogue still loading. A Stop in that
  // window has already said so on the exchange; send nothing.
  const turnTier = await run.resolveTier(modelSelection.model, modelSelection.provider)
  if (run.signal.aborted) return
  await streamChat(
    {
      content: input.content,
      model: modelSelection.model,
      provider: modelSelection.provider === OPENAI_CODEX_SUBSCRIPTION_PROVIDER
        ? modelSelection.provider
        : undefined,
      subscriptionConnectionId: modelSelection.provider === OPENAI_CODEX_SUBSCRIPTION_PROVIDER
        ? modelSelection.subscriptionConnectionId
        : undefined,
      minPrivacyTier: turnTier.tier,
      spaceRef: run.spaceRef,
      mentionedAgentRef: input.mentionedAgentRef,
      threadId: input.replyThreadId,
      // The send's name (F-t1-033). `sendId` is undefined while the key is off,
      // which leaves both fields off the wire.
      clientMessageId: run.sendId,
      sessionKey: run.sendId && !input.replyThreadId ? spaceSendSessionKey(run.sendId) : undefined,
      // Picked skills ride as actions; chat-client puts their ids on the
      // wire as `skill_ids`, never as tool specs.
      actions: input.skills.length > 0
        ? input.skills.map((skill) => ({ id: skill.id, name: skill.name, kind: 'skill' as const }))
        : undefined,
    },
    run.handlers,
    run.signal,
  )
}

const SEND_STRATEGIES: Record<SpaceComposerModeId, SpaceSendStrategy> = {
  ask: askStrategy,
}

/** The composer as the pipeline sees it; props are read at call time. */
export interface SpaceSendPipelineHost {
  readonly spaceRef: string
  /** Called once a turn completes, so the caller can refetch the thread list. */
  readonly onExchangeSettled?: () => void
  readonly onClearReplyTarget?: () => void
  /**
   * A live artifact from this turn's own stream, with the thread it belongs to.
   * Already through the room's kind and size gate. The host holds it in memory
   * only; it is never written to browser storage.
   */
  readonly onArtifact?: (artifact: ChatArtifact, threadId: string) => void
}

export interface SpaceSendPipelineOptions {
  readonly host: SpaceSendPipelineHost
  readonly resolveTier: TurnPrivacyTierResolver['resolve']
  /** The room's own words for a stream error: known codes translated, unknown kept. */
  readonly describeError: (event: ChatErrorEvent) => string
  readonly onStage?: (stage: SpaceSendStage) => void
  /** Whether a send carries its key and resumes a dropped stream. Defaults to the build-time switch. */
  readonly sendIdempotency?: () => boolean
}

export interface SpaceSendPipeline {
  readonly exchange: () => SpaceExchange | undefined
  readonly submitting: () => boolean
  /** The announced request id of the turn in flight (or the last one). */
  readonly liveRequestId: () => string | undefined
  readonly send: (input: SpaceSendInput) => Promise<void>
  readonly stop: () => Promise<void>
  /**
   * Repeat the last send under its own key. Only when its transport failed (the
   * exchange says `retryable`): the server then either takes it once or answers
   * that it already has it, and either way the transcript holds it once.
   */
  readonly retry: () => Promise<void>
}

export function createSpaceSendPipeline(options: SpaceSendPipelineOptions): SpaceSendPipeline {
  const { host } = options
  const [submitting, setSubmitting] = createSignal(false)
  const [exchange, setExchange] = createSignal<SpaceExchange | undefined>(undefined)
  // The in-flight turn's transport handles. `requestId` arrives on the
  // `connected` frame and is the only thing the cancel endpoint accepts.
  let abortController: AbortController | undefined
  let liveRequestId: string | undefined
  // The thread the in-flight turn is in, from the `connected` frame — the id a
  // new post gets is minted server-side, so it is only known once the stream
  // announces it.
  let liveThreadId: string | undefined
  // The last send as it left the box, with its name, for `retry`.
  let lastSend: SpaceSendInput | undefined
  const idempotencyOn = () => (options.sendIdempotency ?? spaceSendIdempotencyEnabled)()

  async function send(raw: SpaceSendInput): Promise<void> {
    setSubmitting(true)
    const reliable = idempotencyOn()
    const sendId = reliable ? (raw.sendId ?? newSpaceSendId()) : undefined
    const input: SpaceSendInput = sendId ? { ...raw, sendId } : raw
    lastSend = input
    const replyThreadId = input.replyThreadId
    setExchange({ userContent: input.content, assistantContent: '', status: 'streaming' })
    options.onStage?.('started')
    const controller = new AbortController()
    abortController = controller
    liveRequestId = undefined
    liveThreadId = replyThreadId
    const openedNewThread = !replyThreadId
    beginOwnStream({ spaceRef: host.spaceRef, threadId: replyThreadId })

    // Reliability state of THIS send. `terminal`: the stream ended with an answer
    // (done, stopped, or a server error), so a dropped read afterwards is moot.
    // (Typed through `as` because the handlers below assign these out of sight of flow analysis.)
    let terminal = false as boolean
    let announced = false
    let transportError = undefined as ChatErrorEvent | undefined
    // The highest numbered frame applied, so a resume asks for the rest and a
    // frame the server sends again is dropped, never shown twice.
    let lastFrameId = undefined as string | undefined
    let appliedThrough = undefined as number | undefined
    // A resume with no cursor replays the whole answer: start the text over.
    let restartTextOnNext = false

    const handlers: ChatStreamHandlers = {
      acceptFrame: (id) => {
        if (!reliable || !id || !/^\d+$/.test(id)) return true
        const sequence = Number(id)
        if (appliedThrough !== undefined && sequence <= appliedThrough) return false
        appliedThrough = sequence
        return true
      },
      onFrameId: (id) => {
        if (reliable) lastFrameId = id
      },
      onConnected: (event) => {
        if (event.requestId?.trim()) liveRequestId = event.requestId.trim()
        if (event.threadId?.trim()) liveThreadId = event.threadId.trim()
        noteSendTransportOk()
        // A resumed stream announces itself again; the stage is reported once.
        if (!announced) options.onStage?.('accepted')
        announced = true
      },
      onTitle: ({ title }) => {
        // Only for a thread this browser opened: the title event fires on a
        // first exchange, and only the owner may write presentation. Writing
        // it back is what makes the room's other members see the same name
        // on their next poll instead of the raw first message.
        const name = title?.trim()
        const threadId = liveThreadId
        if (!openedNewThread || !name || !threadId) return
        const settled = host.onExchangeSettled
        void updateSpaceThreadPresentation(host.spaceRef, threadId, { title: name })
          .then(() => settled?.())
          .catch(() => undefined)
      },
      onArtifact: (event) => {
        const threadId = liveThreadId
        const artifact = roomArtifactFromEvent(event)
        if (!host.onArtifact || !threadId || !artifact) return
        host.onArtifact(artifact, threadId)
      },
      onMessage: ({ content: delta }) => {
        const restart = restartTextOnNext
        restartTextOnNext = false
        setExchange((current) => current && {
          ...current,
          assistantContent: (restart ? '' : current.assistantContent) + delta,
        })
      },
      onDone: () => {
        terminal = true
        noteSendTransportOk()
        // A stop that already landed must not be overwritten by a trailing
        // `done`: the server emits `stopped` and may still close the
        // stream normally afterwards.
        setExchange((current) =>
          current && current.status === 'streaming' ? { ...current, status: 'done' } : current,
        )
        host.onClearReplyTarget?.()
        host.onExchangeSettled?.()
      },
      onStopped: () => {
        terminal = true
        setExchange((current) =>
          current && { ...current, status: 'stopped', stopRecorded: true },
        )
      },
      onError: (event) => {
        // The server already has this send: accepted, not failed. There is no
        // stream of ours to show (the original request owns it); the room's
        // projection shows the run, so re-read it.
        if (event.code === 'managed_run_already_started') {
          terminal = true
          noteSendTransportOk()
          setExchange((current) => current && current.status === 'streaming'
            ? { ...current, status: 'done', acceptedEarlier: true }
            : current)
          host.onClearReplyTarget?.()
          host.onExchangeSettled?.()
          return
        }
        // A dropped read after the server announced the turn: its outcome is
        // unknown and the run may well be going. Hold the error; the loop below
        // resumes the stream before calling the send failed.
        if (reliable && event.code === 'connection_error' && liveRequestId && !terminal) {
          transportError = event
          return
        }
        terminal = true
        setExchange((current) => current && {
          ...current,
          status: 'error',
          errorCode: event.code,
          errorMessage: options.describeError(event),
          // A failure to reach the server at all (nothing announced) can be repeated
          // under the same key; an answer the server gave cannot.
          ...(reliable && event.code === 'connection_error' ? { retryable: true } : {}),
        })
        if (reliable && event.code === 'connection_error') noteSendTransportLost()
      },
    }

    try {
      await SEND_STRATEGIES[input.mode]({
        input,
        sendId,
        spaceRef: host.spaceRef,
        signal: controller.signal,
        resolveTier: options.resolveTier,
        handlers,
      })
      // Resume a stream that dropped after it was announced, a bounded number of
      // times with growing waits; the run keeps going server-side meanwhile.
      let resumes = 0
      while (transportError && !terminal && !controller.signal.aborted && liveRequestId && resumes < MAX_RESUMES) {
        const waitMs = RESUME_BACKOFF_MS[resumes] ?? RESUME_BACKOFF_MS[RESUME_BACKOFF_MS.length - 1] ?? 1000
        resumes += 1
        transportError = undefined
        await waitOrAbort(waitMs, controller.signal)
        if (controller.signal.aborted) break
        restartTextOnNext = lastFrameId === undefined
        await resumeStream(liveRequestId, handlers, controller.signal, lastFrameId)
      }
      if (transportError && !terminal && !controller.signal.aborted) {
        const lost = transportError
        terminal = true
        noteSendTransportLost()
        setExchange((current) => current && current.status === 'streaming'
          ? {
              ...current,
              status: 'error',
              errorCode: lost.code,
              errorMessage: options.describeError(lost),
              retryable: true,
            }
          : current)
      }
    } catch (error) {
      // An abort we caused is not a failure. Anything else still is.
      if (!(error instanceof DOMException && error.name === 'AbortError') && !controller.signal.aborted) {
        setExchange((current) => current && current.status === 'streaming'
          ? { ...current, status: 'error', errorMessage: error instanceof Error ? error.message : String(error) }
          : current)
      }
    } finally {
      if (abortController === controller) abortController = undefined
      endOwnStream()
      setSubmitting(false)
      options.onStage?.('settled')
    }
  }

  /**
   * Stop the turn in flight.
   *
   * Aborting the read is instant and local. Cancelling by `requestId` is what
   * records the stop: it flips the run to `cancelled` in Session Core, which is
   * what every other member's projection shows on its next poll. Without a
   * `requestId` there is nothing to cancel with, and the exchange says so
   * rather than claiming a stop it cannot prove.
   */
  async function stop(): Promise<void> {
    const controller = abortController
    const requestId = liveRequestId
    if (!controller) return
    controller.abort()
    setExchange((current) =>
      current && current.status === 'streaming'
        ? { ...current, status: 'stopped', stopRecorded: false }
        : current,
    )
    if (!requestId) return
    try {
      await cancelInvocation(requestId)
      setExchange((current) => current && current.status === 'stopped' ? { ...current, stopRecorded: true } : current)
    } catch {
      // Recorded state stays false: the run may well finish server-side, and
      // saying "stopped" without proof is the one thing this must not do.
    } finally {
      // Re-read the projection either way, so the post's status follows the
      // server rather than this browser's belief.
      host.onClearReplyTarget?.()
      host.onExchangeSettled?.()
    }
  }

  async function retry(): Promise<void> {
    const again = lastSend
    if (!again || submitting() || exchange()?.retryable !== true) return
    await send(again)
  }

  return { exchange, submitting, liveRequestId: () => liveRequestId, send, stop, retry }
}
