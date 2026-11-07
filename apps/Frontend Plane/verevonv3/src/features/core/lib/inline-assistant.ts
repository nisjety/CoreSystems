import type { DashboardComposerSubmitPayload } from '@/features/dashboard/home/DashboardComposer'
import { appendInlinedAttachments, inlineTextAttachments, toStreamAttachments } from '@/features/chat/components/chat-normalizers'
import { withBrregLookupAction } from '@/features/chat/lib/brreg-action'
import { cancelInvocation, streamChat, type ChatInvokeRequest, type ChatStreamHandlers } from '@/shared/api/chat-client'
import { cancelRun } from '@/shared/api/orchestration-client'

export type InlineAssistantTurn = { role: 'user' | 'assistant'; text: string }
export type InlineAssistantState = {
  turns: InlineAssistantTurn[]
  status: 'idle' | 'sending' | 'streaming' | 'stopping' | 'stopped' | 'error'
  error: string | null
  activity: string | null
  threadId: string | null
  /** The run produced a card or source the compact surface cannot render. */
  needsFullChat: boolean
}

type Dependencies = {
  stream: typeof streamChat
  cancelInvocation: typeof cancelInvocation
  cancelRun: typeof cancelRun
}

type ActiveRun = { abort: AbortController; scope: string; requestId?: string; runId?: string; zdr: boolean; stopRequested: boolean; timer?: ReturnType<typeof setTimeout> }

const defaults: Dependencies = { stream: streamChat, cancelInvocation, cancelRun }
const freshState = (): InlineAssistantState => ({ turns: [], status: 'idle', error: null, activity: null, threadId: null, needsFullChat: false })

/**
 * One in-memory chat owned by the shell. It has no active-thread listeners or
 * pending-launch consumer and is never a Space grant. The caller must call
 * reset when the authenticated user or organization changes.
 */
export function createInlineAssistantController(options: {
  scope: () => string | null
  onChange: (state: InlineAssistantState) => void
  tr?: (no: string, en: string) => string
  dependencies?: Dependencies
}) {
  const api = options.dependencies ?? defaults
  const tr = options.tr ?? ((_no: string, en: string) => en)
  let state = freshState()
  let serial = 0
  let preparing = false
  let disposed = false
  let active: ActiveRun | null = null
  let previousZdr: boolean | null = null
  const publish = (patch: Partial<InlineAssistantState>) => {
    state = { ...state, ...patch }
    if (!disposed) options.onChange(state)
  }
  const id = (prefix: string) => `${prefix}-${crypto.randomUUID()}`
  let sessionKey = id('assistant')

  const cancelKnown = (run: ActiveRun) => {
    const requests: Promise<unknown>[] = []
    if (run.requestId) requests.push(api.cancelInvocation(run.requestId))
    if (run.runId && !run.zdr) requests.push(api.cancelRun(run.runId))
    if (!requests.length) return
    void Promise.allSettled(requests).then((results) => {
      if (active !== run || options.scope() !== run.scope) return
      const failed = results.some((result) => result.status === 'rejected')
      publish(failed
        ? { status: 'error', error: tr('Kunne ikke bekrefte at kjøringen ble stoppet.', 'Could not confirm that the run stopped.') }
        : { status: 'stopped', activity: null })
      active = null
    })
  }

  const stop = () => {
    const run = active
    if (!run || run.stopRequested) return
    run.stopRequested = true
    publish({ status: 'stopping', activity: null })
    if (run.requestId || run.runId) {
      run.abort.abort()
      cancelKnown(run)
    } else {
      // Keep the connection briefly for its ID so Stop can reach the server.
      run.timer = setTimeout(() => {
        if (active !== run) return
        run.abort.abort()
        active = null
        publish({ status: 'error', error: tr('Kunne ikke bekrefte at kjøringen ble stoppet.', 'Could not confirm that the run stopped.') })
      }, 10_000)
    }
  }

  const reset = () => {
    serial += 1
    const run = active
    active = null
    if (run) {
      if (run.timer) clearTimeout(run.timer)
      run.stopRequested = true
      if (run.requestId || run.runId) {
        run.abort.abort()
        cancelKnown(run)
      } else {
        // Keep the stream briefly so a late connection can still be cancelled.
        run.timer = setTimeout(() => run.abort.abort(), 10_000)
      }
    }
    previousZdr = null
    sessionKey = id('assistant')
    publish(freshState())
  }

  const dispose = () => {
    disposed = true
    serial += 1
    const run = active
    active = null
    if (run) {
      run.stopRequested = true
      if (run.timer) clearTimeout(run.timer)
      run.abort.abort()
      cancelKnown(run)
    }
    state = freshState()
  }

  async function send(payload: DashboardComposerSubmitPayload, context: { spaceRef?: string; planMode?: boolean } = {}) {
    const scope = options.scope()
    if (disposed) throw new Error(tr('Assistenten er ikke lenger tilgjengelig.', 'The assistant is no longer available.'))
    if (!scope) throw new Error(tr('Ingen innlogget arbeidsflate er tilgjengelig.', 'No authenticated workspace is available.'))
    if (active || preparing) throw new Error(tr('Et svar er allerede underveis.', 'An answer is already in progress.'))
    if (context.planMode || payload.agentMode || payload.actions.length || payload.tools.length || payload.attachments.length) {
      throw new Error(tr('Åpne Chat for å bruke verktøy, vedlegg eller agentmodus.', 'Open Chat to use tools, attachments or agent mode.'))
    }
    // The existing Chat controller does not consume this payload field. Refuse
    // rather than silently dropping an explicitly requested browser context.
    if (payload.browserContext) throw new Error(tr('Nettleserkontekst er ikke tilgjengelig her. Åpne Chat for å fortsette.', 'Browser context is not available in the inline assistant. Open Chat to continue.'))
    const generation = serial
    preparing = true
    let attachments: Awaited<ReturnType<typeof toStreamAttachments>>
    let inlined: Awaited<ReturnType<typeof inlineTextAttachments>>
    try {
      attachments = await toStreamAttachments(payload.attachments)
      inlined = await inlineTextAttachments(payload.attachments)
    } finally {
      preparing = false
    }
    if (generation !== serial || options.scope() !== scope) throw new Error(tr('Arbeidsflaten ble endret. Meldingen ble ikke sendt.', 'The workspace changed. Your message was not sent.'))
    const zdr = payload.zdr === true
    const newPrivacy = previousZdr !== null && previousZdr !== zdr
    if (newPrivacy) sessionKey = id('assistant')
    const threadId = newPrivacy || zdr ? undefined : state.threadId ?? undefined
    const userTurns = newPrivacy ? [] : state.turns
    previousZdr = zdr
    const content = inlined.length ? appendInlinedAttachments(payload.text, inlined) : payload.text
    const request: ChatInvokeRequest = {
      content,
      model: payload.model,
      provider: payload.provider,
      subscriptionConnectionId: payload.subscriptionConnectionId,
      threadId,
      sessionKey: threadId ?? sessionKey,
      spaceRef: context.spaceRef,
      sourceScope: payload.sourceScope,
      retrievalScope: payload.retrievalScope,
      connectorScope: payload.connectorScope,
      agentMode: payload.agentMode,
      browseWeb: payload.tools.includes('search') || payload.tools.includes('research'),
      deepResearch: payload.tools.includes('research'),
      generateImage: payload.tools.includes('image'),
      attachments: attachments.length ? attachments : undefined,
      actions: withBrregLookupAction(payload.actions.map(({ id, name, kind }) => ({ id, name, kind })), payload.text),
      effort: payload.effort,
      verbosity: payload.tone,
      minPrivacyTier: payload.minPrivacyTier,
      planMode: context.planMode,
      zdr,
      clientMessageId: zdr ? undefined : id('msg'),
    }
    const run: ActiveRun = { abort: new AbortController(), scope, zdr, stopRequested: false }
    active = run
    let terminal = false
    const current = () => active === run && generation === serial && options.scope() === scope
    publish({ turns: [...userTurns, { role: 'user', text: payload.text }, { role: 'assistant', text: '' }], status: 'sending', error: null, activity: null, threadId: threadId ?? null, needsFullChat: newPrivacy ? false : state.needsFullChat })
    const handlers: ChatStreamHandlers = {
      onUiEvent: (event) => {
        if (current() && (event.type === 'approval.requested' || event.type === 'run.paused')) publish({ needsFullChat: true })
      },
      onConnected: (event) => {
        run.requestId = event.requestId
        run.runId = event.runId
        if (run.stopRequested) {
          if (run.requestId || run.runId) {
            if (run.timer) clearTimeout(run.timer)
            run.abort.abort()
            cancelKnown(run)
          }
          return
        }
        if (!current()) return
        publish({ status: 'streaming', threadId: zdr ? null : event.threadId ?? state.threadId })
      },
      onMessage: ({ content: delta }) => {
        if (!current() || run.stopRequested) return
        const turns = state.turns.slice()
        const last = turns[turns.length - 1]
        if (last?.role === 'assistant') turns[turns.length - 1] = { ...last, text: last.text + delta }
        publish({ turns })
      },
      onStep: (step) => { if (current() && !run.stopRequested) publish({ activity: step.title ?? step.detail ?? null }) },
      onCitation: () => { if (current()) publish({ needsFullChat: true }) },
      onArtifact: () => { if (current()) publish({ needsFullChat: true }) },
      onAttachment: () => { if (current()) publish({ needsFullChat: true }) },
      onPlanProposed: () => { if (current()) publish({ needsFullChat: true }) },
      onUserQuestion: () => { if (current()) publish({ needsFullChat: true }) },
      onDone: (event) => {
        if (!current() || run.stopRequested) return
        terminal = true
        publish(event.stopReason === 'stream_incomplete' ? { status: 'error', error: tr('Svaret ble ikke fullført.', 'The answer did not finish.'), activity: null } : { status: 'idle', activity: null })
      },
      onStopped: () => { if (current()) { terminal = true; if (run.timer) clearTimeout(run.timer); active = null; publish({ status: 'stopped', activity: null }) } },
      onError: (event) => { if (current() && !run.stopRequested) { terminal = true; publish({ status: 'error', error: event.message, activity: null }) } },
    }
    // Return once preparation and dispatch have started. Waiting for the whole
    // answer here would make DashboardComposer clear a newer draft when the
    // stream finally ends; async failures remain visible in this state.
    void (async () => {
      try {
        await api.stream(request, handlers, run.abort.signal)
        if (current() && !terminal && !run.stopRequested) publish({ status: 'error', error: tr('Forbindelsen ble brutt før svaret var ferdig.', 'The connection closed before the answer finished.'), activity: null })
      } catch (error) {
        if (current() && !run.stopRequested) publish({ status: 'error', error: error instanceof Error ? error.message : tr('Kunne ikke sende meldingen.', 'Could not send the message.'), activity: null })
      } finally {
        if (current() && !run.stopRequested) active = null
      }
    })()
  }

  return { send, stop, reset, dispose, state: () => state }
}
