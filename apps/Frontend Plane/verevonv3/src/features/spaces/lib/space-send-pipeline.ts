import { createSignal } from 'solid-js'

import type { ChatArtifact } from '@/features/chat/components/chat-types'
import { roomArtifactFromEvent } from './room-artifacts'
import type { SubmitPrivacyTier, TurnPrivacyTierResolver } from '@/features/dashboard/home/submit-privacy-tier'
import type { AiModelSelection } from '@/shared/ai/model-selection'
import {
  cancelInvocation,
  streamChat,
  type ChatErrorEvent,
  type ChatStreamHandlers,
} from '@/shared/api/chat-client'
import { OPENAI_CODEX_SUBSCRIPTION_PROVIDER } from '@/shared/api/chatgpt-subscription-client'
import type { Skill } from '@/shared/api/skills-client'
import { updateSpaceThreadPresentation } from '@/shared/api/spaces-client'
import { beginOwnStream, endOwnStream } from './space-live-work'
import type { SpaceComposerModeId } from './space-composer-registry'

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
}

export interface SpaceSendPipeline {
  readonly exchange: () => SpaceExchange | undefined
  readonly submitting: () => boolean
  /** The announced request id of the turn in flight (or the last one). */
  readonly liveRequestId: () => string | undefined
  readonly send: (input: SpaceSendInput) => Promise<void>
  readonly stop: () => Promise<void>
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

  async function send(input: SpaceSendInput): Promise<void> {
    setSubmitting(true)
    const replyThreadId = input.replyThreadId
    setExchange({ userContent: input.content, assistantContent: '', status: 'streaming' })
    options.onStage?.('started')
    const controller = new AbortController()
    abortController = controller
    liveRequestId = undefined
    liveThreadId = replyThreadId
    const openedNewThread = !replyThreadId
    beginOwnStream({ spaceRef: host.spaceRef, threadId: replyThreadId })

    const handlers: ChatStreamHandlers = {
      onConnected: (event) => {
        if (event.requestId?.trim()) liveRequestId = event.requestId.trim()
        if (event.threadId?.trim()) liveThreadId = event.threadId.trim()
        options.onStage?.('accepted')
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
        setExchange((current) => current && { ...current, assistantContent: current.assistantContent + delta })
      },
      onDone: () => {
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
        setExchange((current) =>
          current && { ...current, status: 'stopped', stopRecorded: true },
        )
      },
      onError: (event) => {
        setExchange((current) => current && {
          ...current,
          status: 'error',
          errorCode: event.code,
          errorMessage: options.describeError(event),
        })
      },
    }

    try {
      await SEND_STRATEGIES[input.mode]({
        input,
        spaceRef: host.spaceRef,
        signal: controller.signal,
        resolveTier: options.resolveTier,
        handlers,
      })
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

  return { exchange, submitting, liveRequestId: () => liveRequestId, send, stop }
}
