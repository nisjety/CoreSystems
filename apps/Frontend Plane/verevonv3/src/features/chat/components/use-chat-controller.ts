import {
  createEffect,
  createMemo,
  createSignal,
  onCleanup,
  onMount,
} from 'solid-js'
import {
  createStore,
  unwrap,
} from 'solid-js/store'
import {
  beginNewVersion,
  lastUserIndex,
  selectVersion,
  versionBadge,
  type ExchangeVersionState,
} from '@/features/chat/lib/chat-versions'
import {
  CHAT_ACTIVE_THREAD_CHANGED_EVENT,
  clearActiveChatThreadId,
  readActiveChatThreadId,
  readChatThreadHistory,
  readChatThreadTranscript,
  removeChatThreadHistoryItem,
  removeChatThreadTranscript,
  setActiveChatThreadId,
  upsertChatThreadHistory,
  upsertChatThreadTranscript,
  type ChatThreadHistoryInput,
  type ChatThreadTitleKind,
  type ChatThreadTranscriptStep,
  type ChatThreadTranscriptTurn,
} from '@/features/chat/lib/chat-thread-history'
import { isLocalRetentionAllowed } from '@/features/chat/lib/chat-retention'
import {
  withBrregLookupAction,
} from '@/features/chat/lib/brreg-action'
import {
  consumePendingChatLaunch,
} from '@/features/chat/lib/pending-chat-launch'
import {
  bindSupportChatThread,
} from '@/shared/chat/support-chat-thread'
import { readThreadDeepLink } from '@/features/chat/lib/chat-thread-deep-link'
import {
  readChatRunPanelCollapsed,
  writeChatRunPanelCollapsed,
} from '@/features/chat/lib/chat-run-watch'
import {
  type DashboardComposerSubmitPayload,
} from '@/features/dashboard/home/DashboardComposer'
import {
  decideApproval,
  listApprovals,
  resumeRun,
  type ApprovalDecision,
} from '@/shared/api/orchestration-client'
import {
  cancelInvocation,
  cheapDefaultModelId,
  deleteChatThread,
  describeFeedbackFailure,
  getChatThreadTranscript,
  getThreadMessages,
  listChatThreads,
  listModels,
  resumeStream,
  saveChatThreadSnapshot,
  streamChat,
  submitFeedback,
  VEREVON_BALANCE_MODE_ID,
  type ChatFeedbackRating,
} from '@/shared/api/chat-client'
import {
  ApiError,
} from '@/shared/api/http'
import {
  findArtifactById,
} from './chat-artifacts'
import {
  collectArtifactItems,
  collectEvidenceSources,
  collectLatestGrounding,
  createChatTitle,
  createPreview,
  formatToolArgs,
  hostname,
  normalizeGrounding,
  prettyModel,
  selectLatestImageArtifact,
} from './chat-media-markdown'
import {
  applyToolResult,
  buildTaskSteps,
  citationEvidence,
  composerToolIdForToolName,
  createId,
  createTurnStep,
  dedupeChatTurns,
  extractCitationsFromToolOutput,
  formatUsageSummary,
  humanizeToolName,
  mergeServerTurnsWithCachedMetadata,
  messageToTurn,
  missingSearchResultStep,
  normalizeArtifact,
  normalizeCitation,
  normalizeGeneratedFile,
  normalizeStep,
  normalizeToolCall,
  readBrowseWebPreference,
  searchCompletionDetail,
  searchQueryFromArgs,
  summarizeGrounding,
  summarizeToolResult,
  taskStepsToTranscript,
  toStreamAttachments,
  toolNameForResult,
  transcriptStepToTaskStep,
  transcriptTurnToChatTurn,
  turnsToTranscript,
  upsertArtifact,
  upsertCitation,
  upsertGeneratedFile,
  upsertStepEvidence,
  upsertToolCall,
  writeBrowseWebPreference,
} from './chat-normalizers'
import type {
  AgentTaskStep,
  ChatState,
  ChatTab,
  ChatTurn,
  Citation,
  SendOptions,
  TaskStepStatus,
} from './chat-types'

export function useChatController() {
  const [state, setState] = createStore<ChatState>({
    turns: [],
    taskSteps: [],
    status: 'idle',
    error: null,
    requestId: null,
    threadId: null,
    // Default to the Verevon Balance intent mode; the backend resolves it
    // server-side. `cheapDefaultModelId` reaffirms this on mount.
    activeModel: VEREVON_BALANCE_MODE_ID,
    branchCount: 0,
  })

  const [activeTab, setActiveTab] = createSignal<ChatTab>('chat')
  const [copiedTurnId, setCopiedTurnId] = createSignal<string | null>(null)
  const [launchMotion, setLaunchMotion] = createSignal(false)
  const [showScrollDown, setShowScrollDown] = createSignal(false)
  const [imageMode, setImageMode] = createSignal(false)
  const [planMode, setPlanMode] = createSignal(false)
  const [browseWeb, setBrowseWeb] = createSignal(readBrowseWebPreference())
  // Temporary chat (ChatGPT's "Temporary Chat" = Verevon's already-enforced
  // Zero Data Retention mode): `temporaryChat` is the composer TOGGLE state
  // for the NEXT send. `temporaryThreadIds` is the set of thread ids THIS
  // session has actually sent a ZDR turn in — read at send time, not
  // recomputed from the toggle later, so a toggle flipped after sending
  // cannot retroactively change what a thread already committed to. A
  // temporary thread must never be persisted (history/transcript/server
  // snapshot/title/follow-ups) — see `isTemporaryThread` and the guard at the
  // top of `writeThreadSnapshot`.
  const [temporaryChat, setTemporaryChat] = createSignal(false)
  const [temporaryThreadIds, setTemporaryThreadIds] = createSignal<ReadonlySet<string>>(new Set())
  // This is a non-authoritative UI routing hint only. The BFF strips it and
  // re-resolves current Control evidence on every scoped append; keeping it in
  // memory makes a newly-created scoped thread continue to send the selected
  // Space on later turns during this chat session without persisting a bearer.
  const scopedThreadRefs = new Map<string, string>()
  const [input, setInput] = createSignal('')
  /**
   * Quiet, non-blocking notice for a rating that did NOT persist.
   *
   * Kept out of `state.error` on purpose: that field renders the *answer* as
   * failed, and a rejected thumbs-up says nothing about the answer.
   */
  const [feedbackNotice, setFeedbackNotice] = createSignal<string | null>(null)
  /**
   * Edit/regenerate version navigation (chat-parity §8 / "#49 part 2") —
   * client-only, session-lifetime, guarded to the final exchange. See
   * `chat-versions.ts` for why: nothing here is persisted, so a nested version
   * tree (the first design's fatal flaw) is structurally impossible.
   */
  const [versionState, setVersionState] = createSignal<ExchangeVersionState | null>(null)
  /** Chat split view: whether the live agent panel is folded to its rail. */
  const [runPanelCollapsed, setRunPanelCollapsed] = createSignal(readChatRunPanelCollapsed())
  const toggleRunPanel = () => {
    const next = !runPanelCollapsed()
    setRunPanelCollapsed(next)
    writeChatRunPanelCollapsed(next)
  }
  let feedbackNoticeTimer: number | undefined
  let abortController: AbortController | undefined
  let messageListRef: HTMLDivElement | undefined
  const setMessageListRef = (el: HTMLDivElement) => {
    messageListRef = el
  }
  const autoFollow = { current: true }

  const hasMessages = () => state.turns.length > 0
  const isStreaming = () => state.status === 'streaming'
  const evidenceSources = createMemo(() => collectEvidenceSources(state.turns))
  const latestGrounding = createMemo(() => collectLatestGrounding(state.turns))
  const artifactItems = createMemo(() => collectArtifactItems(state.turns))
  const artifacts = createMemo(() => artifactItems().map((item) => item.artifact))
  const latestScreen = createMemo(() => selectLatestImageArtifact(state.turns))
  const title = () => createChatTitle(state.turns)
  /**
   * The run the live agent panel watches: the most recent assistant turn that
   * actually has a durable orchestration run id. Plain chat turns have none, so
   * this stays null and the panel never opens.
   */
  const liveRunId = createMemo(() => {
    for (let index = state.turns.length - 1; index >= 0; index -= 1) {
      const turn = state.turns[index]
      if (turn?.role === 'assistant' && turn.runId) return turn.runId
    }
    return null
  })
  let serverSnapshotTimer: number | undefined
  let pendingServerSnapshot: {
    preview?: string
    taskSteps: ChatThreadTranscriptStep[]
    threadId: string
    title?: string
    turns: ChatThreadTranscriptTurn[]
    updatedAt?: string
  } | null = null

  createEffect(() => {
    writeBrowseWebPreference(browseWeb())
  })

  const flushServerThreadSnapshot = async () => {
    if (serverSnapshotTimer !== undefined) {
      window.clearTimeout(serverSnapshotTimer)
      serverSnapshotTimer = undefined
    }
    const snapshot = pendingServerSnapshot
    pendingServerSnapshot = null
    if (!snapshot) return
    await saveChatThreadSnapshot(snapshot.threadId, {
      title: snapshot.title,
      preview: snapshot.preview,
      updatedAt: snapshot.updatedAt,
      turns: snapshot.turns,
      taskSteps: snapshot.taskSteps,
    }).catch(() => undefined)
  }

  const queueServerThreadSnapshot = (snapshot: NonNullable<typeof pendingServerSnapshot>) => {
    pendingServerSnapshot = snapshot
    if (serverSnapshotTimer !== undefined) window.clearTimeout(serverSnapshotTimer)
    serverSnapshotTimer = window.setTimeout(() => {
      void flushServerThreadSnapshot()
    }, 500)
  }

  onCleanup(() => {
    if (serverSnapshotTimer !== undefined) window.clearTimeout(serverSnapshotTimer)
  })

  /** True once this thread has actually sent a ZDR (temporary chat) turn. */
  const isTemporaryThread = (threadId: string | null | undefined): boolean =>
    Boolean(threadId) && temporaryThreadIds().has(threadId as string)

  const markThreadTemporary = (threadId: string) => {
    setTemporaryThreadIds((prev) => (prev.has(threadId) ? prev : new Set(prev).add(threadId)))
  }

  /** The provisional client-generated thread id is replaced by the server's real one; carry the temporary marking across that rename so the lock survives it. */
  const renameTemporaryThread = (fromId: string, toId: string) => {
    setTemporaryThreadIds((prev) => {
      if (!prev.has(fromId)) return prev
      const next = new Set(prev)
      next.delete(fromId)
      next.add(toId)
      return next
    })
  }

  const writeThreadSnapshot = (
    threadId: string,
    turns: ChatTurn[],
    overrides: Partial<ChatThreadHistoryInput> = {},
    taskSteps: AgentTaskStep[] = state.taskSteps,
    options: { persistServer?: boolean } = {},
  ) => {
    // Temporary (ZDR) chat: never persisted, full stop. This is the single
    // funnel every persistence call site in this file goes through
    // (history + transcript + server snapshot below), so gating here covers
    // all three without touching chat-thread-history.ts's storage functions.
    // Defensive: the backend already never emits a `title`/`follow_ups` event
    // for a ZDR turn, but a caller (e.g. `onTitle`) reaching here anyway must
    // still not write.
    if (isTemporaryThread(threadId)) return
    // …and the SERVER's posture, which this in-memory Set cannot represent.
    // `temporaryThreadIds` only knows about temporary chats started in THIS
    // tab; it says nothing about an org-wide Zero Data Retention policy, and it
    // is gone on reload. `chat-retention` carries the posture the gateway
    // stated on the last threads listing, so a ZDR workspace writes no local
    // copy even though the composer toggle was never touched.
    if (!isLocalRetentionAllowed()) return
    const firstUserTurn = turns.find((turn) => turn.role === 'user')
    const lastTurn = turns.at(-1)
    const stored = readChatThreadHistory().find((item) => item.threadId === threadId)
    // Title lock: once a thread carries an AI-generated title (from the
    // gateway's `title` SSE event), every later snapshot keeps it — the
    // periodic writes must not clobber it with the truncated first message.
    // Resolving it HERE (not only in the upsert) matters because the server
    // snapshot below persists this exact title.
    const generatedTitle =
      overrides.titleKind === 'generated' && overrides.title
        ? overrides.title
        : stored?.titleKind === 'generated'
          ? stored.title
          : undefined
    const title =
      generatedTitle ??
      overrides.title ??
      (firstUserTurn ? createPreview(firstUserTurn.content, 48) : createChatTitle(turns))
    const titleKind: ChatThreadTitleKind = generatedTitle ? 'generated' : 'preview'
    const preview = overrides.preview ?? lastTurn?.content
    // Activity timestamp, never write timestamp: when this snapshot carries no
    // usable message time (e.g. a selection self-heal over timestamp-less
    // server messages), keep the stored `updatedAt` instead of letting
    // normalizeTimestamp stamp "now" — a click must not re-date the thread.
    const updatedAt = overrides.updatedAt ?? (lastTurn?.createdAt || undefined) ?? stored?.updatedAt
    const transcriptTurns = turnsToTranscript(turns)
    const transcriptTaskSteps = taskStepsToTranscript(taskSteps)
    upsertChatThreadHistory({
      threadId,
      title,
      titleKind,
      preview,
      updatedAt,
    })
    upsertChatThreadTranscript({
      threadId,
      taskSteps: transcriptTaskSteps,
      turns: transcriptTurns,
      updatedAt,
    })
    if (options.persistServer !== false) {
      queueServerThreadSnapshot({
        threadId,
        title,
        preview,
        updatedAt,
        turns: transcriptTurns,
        taskSteps: transcriptTaskSteps,
      })
    }
  }

  /**
   * Server messages can arrive without timestamps (session-core stores none),
   * and `messageToTurn` keeps those EMPTY so a fetch never fabricates "now"
   * (which made every selection look like fresh activity). The cached-metadata
   * merge restores the real send time where a cached twin exists; this fills
   * whatever remains from the thread's genuine last-activity records, and only
   * a thread this device has never seen gets the current time (it really is
   * new here).
   */
  const fillMissingTurnTimestamps = (
    turns: ChatTurn[],
    threadId: string,
    cachedUpdatedAt?: string,
  ): ChatTurn[] => {
    if (turns.every((turn) => turn.createdAt)) return turns
    const fallback =
      cachedUpdatedAt ??
      readChatThreadHistory().find((item) => item.threadId === threadId)?.updatedAt ??
      new Date().toISOString()
    return turns.map((turn) => (turn.createdAt ? turn : { ...turn, createdAt: fallback }))
  }

  /**
   * Guards for thread switching.
   *
   * `hydratingThreadId`: from the moment a thread is selected until its own
   * turns are in state, `state.threadId` points at the NEW thread while
   * `state.turns` still holds the PREVIOUS thread's messages. The snapshot
   * effect below fires on that intermediate pair, so without this guard every
   * click in the history menu stamped the previous chat's content and title
   * onto the newly selected chat — after a few clicks the whole menu showed
   * one identical conversation.
   *
   * `threadLoadSequence`: selecting B then quickly C must not let B's slower
   * fetch land last and clobber C's view; each load only applies its results
   * if it is still the newest.
   */
  let hydratingThreadId: string | null = null
  let threadLoadSequence = 0

  const loadThread = async (threadId: string) => {
    // Switching threads must tear down the previously-selected thread's
    // in-flight stream/resume and reset the shared view machine. Left running,
    // that stream's terminal handlers keep mutating the store after
    // `state.threadId` has moved on — landing a fallback-retry answer, a stale
    // error banner, or a stuck 'streaming' status (which permanently disables
    // the composer) on the thread the user just opened. The abort is also what
    // makes the fallback-retry unreachable: an aborted SSE returns without
    // calling `onError` (see readSseStream), so the retry branch never fires.
    // Clearing status/error mirrors what the new-chat path (`resetChatState`)
    // already does; the thread-select path needs it too.
    abortController?.abort()
    abortController = undefined
    const seq = ++threadLoadSequence
    hydratingThreadId = threadId
    setState({ threadId, status: 'idle', error: null })
    // Versions are guarded by threadId anyway (chat-versions.ts), but clear
    // eagerly rather than leave stale siblings from the old thread reachable
    // until the next regenerate/edit happens to overwrite them.
    setVersionState(null)
    // Recover the non-secret routing hint from the server-owned listing so a
    // scoped thread remains appendable after a page reload. No authority is
    // cached: the BFF obtains a new Control decision for the actual content.
    const listed = await listChatThreads().catch(() => [])
    const scoped = listed.find((thread) => thread.threadId === threadId)?.spaceRef?.trim()
    if (scoped) scopedThreadRefs.set(threadId, scoped)
    const localCached = readChatThreadTranscript(threadId)
    const serverCached = await getChatThreadTranscript(threadId).catch(() => null)
    if (seq !== threadLoadSequence) return
    const cached = serverCached
      ? {
          threadId: serverCached.threadId,
          turns: serverCached.turns as ChatThreadTranscriptTurn[],
          taskSteps: serverCached.taskSteps as ChatThreadTranscriptStep[] | undefined,
          updatedAt: serverCached.updatedAt,
        }
      : localCached
    const cachedTurns = dedupeChatTurns(cached?.turns.map(transcriptTurnToChatTurn) ?? [])
    const cachedTaskSteps = cached?.taskSteps?.map(transcriptStepToTaskStep) ?? []
    try {
      const history = await getThreadMessages(threadId)
      if (seq !== threadLoadSequence) return
      const serverTurns = dedupeChatTurns(history.map(messageToTurn))
      const mergedTurns = serverTurns.length > 0
        ? mergeServerTurnsWithCachedMetadata(serverTurns, cachedTurns)
        : cachedTurns
      const turns = fillMissingTurnTimestamps(mergedTurns, threadId, cached?.updatedAt)
      // Clear the guard BEFORE the turns land: setState runs the snapshot
      // effect synchronously, and that very run is the one that must persist
      // this thread's real content (it also self-heals entries the old bug
      // already overwrote).
      hydratingThreadId = null
      setState({ turns, taskSteps: cachedTaskSteps })
      if (turns.length > 0) {
        writeThreadSnapshot(threadId, turns, {}, cachedTaskSteps, { persistServer: false })
      }
      maybeResumeStream(threadId, turns)
    } catch (error) {
      if (seq !== threadLoadSequence) return
      // A 404 (thread_not_found) means session-core no longer has this thread
      // — it was deleted, or the id is stale (e.g. carried over from another
      // device). model-gateway now returns 404 for that case rather than a 502
      // outage, so evict the ghost thread from the sidebar list + local caches
      // and deselect it, instead of leaving it selected with stale content and
      // refetching it on every mount. Any other failure (502/timeout/offline)
      // is treated as transient: keep rendering the cached transcript.
      if (error instanceof ApiError && error.status === 404) {
        hydratingThreadId = null
        removeChatThreadHistoryItem(threadId)
        if (readActiveChatThreadId() === threadId) {
          // Clears the stored active id and resets the chat view via the
          // CHAT_ACTIVE_THREAD_CHANGED_EVENT listener registered in onMount.
          clearActiveChatThreadId()
        } else if (state.threadId === threadId) {
          resetChatState()
        }
        return
      }
      const fallbackTurns = cachedTurns
      hydratingThreadId = null
      setState({ turns: fallbackTurns, taskSteps: cachedTaskSteps })
      if (fallbackTurns.length > 0) {
        writeThreadSnapshot(threadId, fallbackTurns, {}, cachedTaskSteps, { persistServer: false })
      }
      maybeResumeStream(threadId, fallbackTurns)
    }
  }

  /**
   * Resumable streams (chat-parity §3b): if the last assistant turn survived
   * into the persisted transcript still `status: 'waiting'`, the tab
   * closed/reloaded mid-answer — `ChatMessages` renders ANY turn with that
   * status as actively streaming, so left alone this is a permanently
   * "thinking" bubble that will never complete. Reattach to the still-
   * buffered SSE run instead.
   */
  const maybeResumeStream = (threadId: string, turns: ChatTurn[]) => {
    const lastTurn = turns.at(-1)
    if (lastTurn && lastTurn.role === 'assistant' && lastTurn.status === 'waiting') {
      void attemptResumeStream(threadId, lastTurn)
    }
  }

  /**
   * The resume endpoint (`model-gateway`'s `stream_buffer.rs`) replays
   * buffered content deltas from the run's start — this client never tracked
   * a `Last-Event-Id` cursor, so `resumeStream` is called without one — plus
   * the final `done` chunk if the run already finished. Nothing else: no
   * tool/citation/usage/title/follow-up replay, the buffer only ever held
   * plain text. The turn's content is reset when the FIRST replayed delta
   * arrives — not upfront — so the full replay does not duplicate onto the
   * cached partial text, while a resume that never yields a delta (404 past
   * the buffer TTL, dead connection) settles with that partial text intact.
   *
   * Best-effort: a 404 (`stream not resumable` — the run genuinely finished
   * outside the buffer's TTL, or never existed) or any other resume failure
   * settles the turn as `stopped`, same as a normal failed/finished stream —
   * never a stuck spinner. Guards against a thread switch landing mid-resume:
   * every mutation checks `state.threadId === threadId` first.
   */
  const attemptResumeStream = async (threadId: string, turn: ChatTurn) => {
    if (!turn.requestId || state.status === 'streaming' || isTemporaryThread(threadId)) return
    const assistantId = turn.id
    const isActiveThread = () => state.threadId === threadId
    const turnIndex = state.turns.findIndex((candidate) => candidate.id === assistantId)
    const precedingUser = turnIndex > 0 ? state.turns[turnIndex - 1] : undefined
    const turnTitle = createPreview(
      precedingUser?.role === 'user' ? precedingUser.content : turn.content,
      58,
    )

    // Keep the cached partial text until the replay actually starts: the
    // buffer replays from seq 0, so content is cleared on the FIRST delta
    // (below) to avoid duplication — but a resume that 404s (buffer expired)
    // or dies before any delta then settles with the partial text intact
    // instead of wiping a visible answer down to an empty stopped bubble.
    setState('turns', (t) => t.id === assistantId, { streaming: true, status: 'waiting' })

    const controller = new AbortController()
    abortController = controller
    setState('status', 'streaming')

    let settled = false
    let replayStarted = false
    const stopStreaming = (status?: ChatTurn['status']) => {
      if (!isActiveThread()) return
      setState('turns', (t) => t.id === assistantId, 'streaming', false)
      setState('turns', (t) => t.id === assistantId, 'status', status)
    }

    await resumeStream(
      turn.requestId,
      {
        onMessage: ({ content: delta }) => {
          if (!isActiveThread()) return
          if (!replayStarted) {
            // First replayed delta: the buffer replays from the start of the
            // answer, so drop the cached partial text now (and only now).
            replayStarted = true
            setState('turns', (t) => t.id === assistantId, 'content', '')
          }
          upsertTaskStep(createTurnStep(assistantId, turnTitle, 'answer', 'Compose response', 'Streaming answer text.', 'active'))
          setState('turns', (t) => t.id === assistantId, 'content', (prev) => prev + delta)
        },
        onDone: ({ modelUsed, outputTokens }) => {
          settled = true
          if (!isActiveThread()) return
          if (modelUsed) setState('turns', (t) => t.id === assistantId, 'modelUsed', modelUsed)
          if (outputTokens != null) setState('turns', (t) => t.id === assistantId, 'outputTokens', outputTokens)
          stopStreaming(undefined)
          markOpenSteps('done', 'Completed.', assistantId)
          setState('status', 'idle')
          writeThreadSnapshot(threadId, state.turns)
        },
        onError: () => {
          settled = true
          if (!isActiveThread()) return
          stopStreaming('stopped')
          markOpenSteps('stopped', 'The connection was lost before this answer finished.', assistantId)
          setState('status', 'idle')
          writeThreadSnapshot(threadId, state.turns)
        },
      },
      controller.signal,
    )

    if (!settled && isActiveThread()) {
      // The stream closed with no terminal event at all (e.g. the buffer was
      // already empty and nothing else arrived) — do not leave the bubble
      // spinning forever.
      stopStreaming('stopped')
      markOpenSteps('stopped', 'Connection closed before this answer finished.', assistantId)
      setState('status', 'idle')
      writeThreadSnapshot(threadId, state.turns)
    }
  }

  createEffect(() => {
    if (!state.threadId || state.turns.length === 0) return
    // While a thread switch is hydrating, `state.turns` still belongs to the
    // PREVIOUS thread — persisting that pair is the overwrite bug loadThread's
    // guard exists for. Live streaming is unaffected: hydratingThreadId is only
    // non-null inside loadThread.
    if (state.threadId === hydratingThreadId) return
    writeThreadSnapshot(state.threadId, state.turns, {}, state.taskSteps, { persistServer: false })
  })

  const resetChatState = () => {
    abortController?.abort()
    setState({
      turns: [],
      taskSteps: [],
      status: 'idle',
      error: null,
      requestId: null,
      threadId: null,
      activeModel: state.activeModel,
      branchCount: 0,
    })
    setInput('')
    setActiveTab('chat')
    setVersionState(null)
    // Each fresh chat starts with Temporary Chat off — a user re-enables it
    // deliberately per conversation rather than it silently staying on.
    setTemporaryChat(false)
  }

  onMount(() => {
    const handleActiveThreadChange = (event: Event) => {
      const threadId = (event as CustomEvent<{ threadId: string | null }>).detail?.threadId
      if (!threadId) {
        resetChatState()
        return
      }
      if (threadId && threadId !== state.threadId) void loadThread(threadId)
    }
    window.addEventListener(CHAT_ACTIVE_THREAD_CHANGED_EVENT, handleActiveThreadChange)
    onCleanup(() => window.removeEventListener(CHAT_ACTIVE_THREAD_CHANGED_EVENT, handleActiveThreadChange))

    const initializeChat = async () => {
      // A Space Activity link is URL-addressable across a browser restart. It
      // may choose the requested view but never grants it: `loadThread` still
      // goes through the owner-bound transcript endpoints, and a 404 clears
      // the local selection rather than retaining a cross-user ghost thread.
      const linkedThread = readThreadDeepLink(window.location.search)
      if (linkedThread) setActiveChatThreadId(linkedThread)
      const storedThread = linkedThread ?? readActiveChatThreadId()
      if (storedThread) await loadThread(storedThread)

      try {
        const available = await listModels()
        // Default to the Verevon Balance intent mode (cost-aware, resolved
        // server-side), never the first (possibly expensive) catalog entry.
        // cheapDefaultModelId is resilient: it always returns the balance mode id.
        const cheapId = cheapDefaultModelId(available)
        if (cheapId) setState('activeModel', cheapId)
      } catch {
        // The chat can still run with the gateway default model.
      }

      const pending = consumePendingChatLaunch()
      if (pending) {
        if (pending.startNewThread) {
          // A contextual handoff is a new Chat conversation by contract. The
          // selected support case becomes the first user turn, never an
          // accidental append to the previously active global thread.
          clearActiveChatThreadId()
          resetChatState()
        }
        if (pending.model) setState('activeModel', pending.model)
        setBrowseWeb(Boolean(pending.tools?.includes('search') || pending.tools?.includes('research')))
        const attachments = await toStreamAttachments(pending.attachments ?? [])
        triggerLaunchMotion()
        await sendContent(pending.text, pending.model, {
          attachments: attachments.length > 0 ? attachments : undefined,
          browseWeb: pending.tools?.includes('search') || pending.tools?.includes('research'),
          deepResearch: pending.tools?.includes('research'),
          displayAttachments: pending.attachments ?? [],
          generateImage: pending.tools?.includes('image'),
          tools: pending.tools ?? [],
          actions: (pending.actions ?? []).map((a) => ({ id: a.id, name: a.name, kind: a.kind })),
        })
        if (pending.supportHandoff && state.threadId) {
          bindSupportChatThread(pending.supportHandoff, state.threadId)
        }
      }
    }

    void initializeChat()
  })

  createEffect(() => {
    const lastTurn = state.turns[state.turns.length - 1]
    const streamSignal = `${lastTurn?.content.length ?? 0}:${lastTurn?.reasoning?.length ?? 0}:${lastTurn?.artifacts?.length ?? 0}`
    void streamSignal
    if (autoFollow.current && messageListRef) {
      messageListRef.scrollTo({ top: messageListRef.scrollHeight, behavior: isStreaming() ? 'auto' : 'smooth' })
    }
  })

  const triggerLaunchMotion = () => {
    requestAnimationFrame(() => {
      setLaunchMotion(true)
      setTimeout(() => setLaunchMotion(false), 1200)
    })
  }

  const handleScroll = () => {
    if (!messageListRef) return
    const dist = messageListRef.scrollHeight - messageListRef.scrollTop - messageListRef.clientHeight
    autoFollow.current = dist < 80
    setShowScrollDown(dist > 160)
  }

  const scrollToBottom = (behavior: ScrollBehavior = 'smooth') => {
    if (!messageListRef) return
    messageListRef.scrollTo({ top: messageListRef.scrollHeight, behavior })
    autoFollow.current = true
    setShowScrollDown(false)
  }

  // ── Agentic human-in-the-loop approvals ─────────────────────────────────
  // An agentic run that hits a risky tool pauses and the chat stream emits a
  // `paused` step carrying the run id. We fetch that run's pending approvals so
  // the assistant turn can render Approve/Reject; deciding + resuming unblocks
  // the still-open run stream so the agent continues.
  const setTurnRunId = (turnId: string, runId: string) => {
    setState('turns', (turn) => turn.id === turnId, 'runId', runId)
  }
  const refreshTurnApprovals = async (turnId: string) => {
    const runId = state.turns.find((turn) => turn.id === turnId)?.runId
    if (!runId) return
    try {
      const approvals = await listApprovals(runId)
      setState(
        'turns',
        (turn) => turn.id === turnId,
        'pendingApprovals',
        approvals.filter((approval) => (approval.status ?? 'PENDING').toUpperCase() === 'PENDING'),
      )
    } catch {
      // Transient list failure — keep the existing pending state.
    }
  }
  const handleApprovalDecision = async (
    turnId: string,
    approvalId: string,
    decision: ApprovalDecision,
  ) => {
    const runId = state.turns.find((turn) => turn.id === turnId)?.runId
    // Optimistically drop the decided approval so the card resolves instantly.
    setState('turns', (turn) => turn.id === turnId, 'pendingApprovals', (prev) =>
      (prev ?? []).filter((approval) => approval.id !== approvalId),
    )
    try {
      await decideApproval(approvalId, decision)
      // Recording the decision unblocks execution-core; resume advances the run
      // (the denial, if any, is recorded so the agent routes around the tool).
      if (runId) await resumeRun(runId)
    } catch {
      // Re-sync from the source of truth if the decision/resume call failed.
      void refreshTurnApprovals(turnId)
    }
  }

  const sendContent = async (rawContent: string, modelOverride?: string, options: SendOptions = {}) => {
    const content = rawContent.trim()
    if (!content || state.status === 'streaming') return

    let activeThreadId = state.threadId ?? createId('thread')
    // Temporary chat locks in at the first send of a thread: once ANY
    // message has gone out under this thread id it is marked temporary for
    // the rest of the session (see `isTemporaryThread`), independent of
    // whatever the composer toggle does afterward.
    const isNewThread = !state.threadId
    // The URL can select a Space for a *new* chat, but it cannot supply any
    // authority. The BFF strips this selection after exchanging it with Control
    // for the signed, effect-bound creation decision.
    const selectedNewSpaceRef = isNewThread && typeof window !== 'undefined'
      ? new URLSearchParams(window.location.search).get('space_ref')?.trim() || undefined
      : undefined
    const requestedSpaceRef = selectedNewSpaceRef ?? scopedThreadRefs.get(activeThreadId)
    if (isNewThread) {
      setState('threadId', activeThreadId)
      if (options.zdr) markThreadTemporary(activeThreadId)
      // A temporary thread's id must never become the persisted "active
      // thread" pointer — that pointer is itself a form of persistence
      // (survives reload), which a no-history/no-memory session must not.
      if (!options.zdr) setActiveChatThreadId(activeThreadId)
    }

    const submittedAt = options.createdAt ?? new Date().toISOString()
    const model = modelOverride ?? state.activeModel
    const assistantId = createId('asst')
    const turnTitle = createPreview(content, 58)
    const stepId = (id: string) => `${assistantId}:${id}`
    const tools = options.tools ?? []
    const displayAttachments = options.displayAttachments ?? []
    const appendUser = options.appendUser !== false

    const userTurn: ChatTurn | null = appendUser
      ? {
          id: createId('user'),
          role: 'user',
          content,
          createdAt: submittedAt,
          streaming: false,
          model,
          tools,
          attachments: displayAttachments,
        }
      : null
    const assistantTurn: ChatTurn = {
      id: assistantId,
      role: 'assistant',
      content: '',
      createdAt: new Date().toISOString(),
      streaming: true,
      status: 'waiting',
      model,
      tools,
      attachments: [],
    }
    const nextTurns = [
      ...state.turns,
      ...(userTurn ? [userTurn] : []),
      assistantTurn,
    ]

    setActiveTab('chat')
    setState('turns', nextTurns)
    writeThreadSnapshot(activeThreadId, nextTurns, { preview: content, updatedAt: submittedAt })
    setState('taskSteps', (steps) => [
      ...steps,
      ...buildTaskSteps(content, tools, appendUser ? 'submit' : 'regenerate', assistantId, options.actions ?? []),
    ])
    setInput('')

    const controller = new AbortController()
    abortController = controller
    setState('requestId', null)
    setState('status', 'streaming')
    setState('error', null)

    // Once the user switches to another thread mid-flight this send no longer
    // owns the shared status/error machine, so its terminal handlers must not
    // write global state onto the now-visible thread. loadThread aborts us on
    // switch, but an aborted SSE resolves as a normal close (readSseStream
    // swallows AbortError), so the finaliser below still runs — this keeps its
    // global writes scoped to the thread this send belongs to. `activeThreadId`
    // tracks the server's real id after the onConnected swap, so it stays the
    // canonical owner check for the whole turn.
    const ownsMachine = () => state.threadId === activeThreadId

    let settled = false
    const captureRequestId = (requestId?: string) => {
      if (!requestId) return
      setState('requestId', requestId)
      setState('turns', (turn) => turn.id === assistantId, 'requestId', requestId)
    }
    const stopStreaming = (status?: ChatTurn['status']) => {
      setState('turns', (turn) => turn.id === assistantId, 'streaming', false)
      setState('turns', (turn) => turn.id === assistantId, 'status', status)
    }

    try {
      await streamChat(
        {
          content,
          model,
          // A scoped first turn lets Session Core mint the durable thread ID.
          // The local provisional ID remains only a UI correlation key until
          // `onConnected` replaces it; sending it as a thread ID would make
          // the BFF correctly treat the request as an existing-thread write.
          threadId: selectedNewSpaceRef ? undefined : activeThreadId,
          sessionKey: activeThreadId,
          spaceRef: requestedSpaceRef,
          browseWeb: options.browseWeb,
          deepResearch: options.deepResearch,
          generateImage: options.generateImage,
          attachments: options.attachments,
          actions: options.actions,
          planMode: planMode(),
          zdr: options.zdr,
          regenerated: options.regenerated,
          editResubmit: options.editResubmit,
        },
        {
          onConnected: ({ requestId, threadId: serverThreadId, model: connectedModel, runId }) => {
            captureRequestId(requestId)
            // An agentic / plan-mode turn learns its durable orchestration run
            // id here — this is what lets the live agent panel attach to
            // `GET /api/v1/runs/:run_id/events` from the very first step
            // instead of only once the run pauses for an approval.
            if (runId) setTurnRunId(assistantId, runId)
            if (serverThreadId) {
              const priorThreadId = activeThreadId
              if (serverThreadId !== activeThreadId) {
                const provisionalThreadId = activeThreadId
                removeChatThreadHistoryItem(provisionalThreadId)
                removeChatThreadTranscript(provisionalThreadId)
                void deleteChatThread(provisionalThreadId).catch(() => undefined)
                // The provisional id may have been the one just marked
                // temporary above — carry that marking to the server's real
                // id so the lock and the persistence guard both survive the
                // swap. (In practice a ZDR turn's `connected` event never
                // carries a `thread_id` at all — ZDR creates no session/
                // thread server-side — so this is defensive, not the normal
                // path.)
                renameTemporaryThread(provisionalThreadId, serverThreadId)
              }
              activeThreadId = serverThreadId
              if (requestedSpaceRef) {
                scopedThreadRefs.delete(priorThreadId)
                scopedThreadRefs.set(serverThreadId, requestedSpaceRef)
              }
              setState('threadId', serverThreadId)
              if (!isTemporaryThread(serverThreadId)) setActiveChatThreadId(serverThreadId)
              writeThreadSnapshot(serverThreadId, state.turns, { preview: content, updatedAt: submittedAt })
            }
            if (connectedModel) {
              setState('turns', (turn) => turn.id === assistantId, 'modelUsed', connectedModel)
            }
            markStepDone(stepId('connect'), 'Connected to the live agent stream.')
            if (connectedModel) {
              upsertTaskStep(createTurnStep(assistantId, turnTitle, 'model', 'Model selected', prettyModel(connectedModel), 'done'))
            }
          },
          onMessage: ({ content: delta, requestId }) => {
            captureRequestId(requestId)
            upsertTaskStep(createTurnStep(assistantId, turnTitle, 'answer', 'Compose response', 'Streaming answer text.', 'active'))
            setState('turns', (turn) => turn.id === assistantId, 'content', (prev) => prev + delta)
          },
          onArtifact: (event) => {
            const artifact = normalizeArtifact(event)
            if (!artifact) return
            // The same artifact id can come back many turns later (the model
            // rewrites a document it produced earlier). Hand the earlier
            // carrier's revisions along so the version history survives the
            // move to this turn instead of restarting at one entry.
            const carried = findArtifactById(state.turns.map((turn) => turn.artifacts), artifact.id)
            setState('turns', (turn) => turn.id === assistantId, 'artifacts', (prev) => (
              upsertArtifact(prev ?? [], artifact, carried)
            ))
          },
          onAttachment: (event) => {
            const file = normalizeGeneratedFile(event)
            if (!file) return
            setState('turns', (turn) => turn.id === assistantId, 'files', (prev) => upsertGeneratedFile(prev ?? [], file))
          },
          onCitation: (event) => {
            const citation = normalizeCitation(event)
            if (!citation) return
            addAssistantCitation(assistantId, turnTitle, citation)
          },
          onGrounding: ({ value }) => {
            const grounding = normalizeGrounding(value)
            if (!grounding) return
            setState('turns', (turn) => turn.id === assistantId, 'grounding', grounding)
            upsertTaskStep(createTurnStep(assistantId, turnTitle, 'grounding', 'Knowledge grounding', summarizeGrounding(grounding), 'done'))
          },
          onReasoning: ({ delta }) => {
            if (!delta) return
            upsertTaskStep(createTurnStep(assistantId, turnTitle, 'reasoning', 'Reasoning trace', 'Received model reasoning tokens.', 'active'))
            setState('turns', (turn) => turn.id === assistantId, 'reasoning', (prev = '') => prev + delta)
          },
          onStep: (event) => {
            const step = normalizeStep(event, assistantId, turnTitle)
            if (step) upsertTaskStep(step)
            // Agentic HITL: the raw step carries the orchestration status before
            // it is coerced to a task-status. A `paused` run is awaiting human
            // approval; approval/resume steps mean the gate resolved.
            const rawStatus = (event.status ?? '').toLowerCase()
            if (rawStatus === 'paused' && event.id) {
              setTurnRunId(assistantId, event.id)
              void refreshTurnApprovals(assistantId)
            } else if (event.title === 'Approval' || event.title === 'Resumed') {
              void refreshTurnApprovals(assistantId)
            }
          },
          onToolCall: (event) => {
            const call = normalizeToolCall(event)
            if (!call) return
            setState('turns', (turn) => turn.id === assistantId, 'toolCalls', (prev) => upsertToolCall(prev ?? [], call))
            markComposerToolStarted(assistantId, call.name, call.args)
            upsertTaskStep({
              id: stepId(`tool-${call.id}`),
              title: `Tool: ${humanizeToolName(call.name)}`,
              detail: formatToolArgs(call.args) || 'Tool call running.',
              status: 'active',
              createdAt: new Date().toISOString(),
              turnId: assistantId,
              turnTitle,
            })
          },
          onToolResult: (event) => {
            if (!event.id) return
            const toolName = toolNameForResult(state.turns.find((turn) => turn.id === assistantId)?.toolCalls ?? [], event.id)
            setState('turns', (turn) => turn.id === assistantId, 'toolCalls', (prev) => applyToolResult(prev ?? [], event))
            const citations = extractCitationsFromToolOutput(event.output ?? '')
            for (const citation of citations) {
              addAssistantCitation(assistantId, turnTitle, citation)
            }
            markComposerToolCompleted(assistantId, toolName, event.error, event.output, citations.length)
            upsertTaskStep({
              id: stepId(`tool-${event.id}`),
              title: `Tool: ${humanizeToolName(toolName ?? 'tool')}`,
              detail: summarizeToolResult(event),
              status: event.error ? 'error' : 'done',
              createdAt: new Date().toISOString(),
              turnId: assistantId,
              turnTitle,
            })
          },
          onUsage: (usage) => {
            setState('turns', (turn) => turn.id === assistantId, {
              inputTokens: usage.inputTokens,
              outputTokens: usage.outputTokens,
              latencyMs: usage.latencyMs,
              costUsd: usage.costUsd,
              confidence: usage.confidence,
            })
            upsertTaskStep(createTurnStep(assistantId, turnTitle, 'usage', 'Usage recorded', formatUsageSummary(usage), 'done'))
          },
          onTitle: ({ title }) => {
            // AI-generated thread title (first exchange only, server-side).
            // Persisting with titleKind 'generated' locks it: later periodic
            // snapshots resolve against the stored item and keep this title
            // instead of reverting to the truncated first message.
            if (!title.trim()) return
            writeThreadSnapshot(state.threadId ?? activeThreadId, state.turns, {
              title,
              titleKind: 'generated',
            })
          },
          onFollowUps: ({ suggestions }) => {
            // The backend never emits this for a ZDR turn, but the guard is
            // defensive here too — a temporary chat must never RECEIVE
            // follow-up chips either, not just never persist them.
            if (suggestions.length === 0 || isTemporaryThread(state.threadId ?? activeThreadId)) return
            setState('turns', (turn) => turn.id === assistantId, 'followUps', suggestions.slice(0, 3))
          },
          onDone: ({ requestId, modelUsed, outputTokens }) => {
            settled = true
            captureRequestId(requestId)
            if (modelUsed) setState('turns', (turn) => turn.id === assistantId, 'modelUsed', modelUsed)
            if (outputTokens != null) setState('turns', (turn) => turn.id === assistantId, 'outputTokens', outputTokens)
            stopStreaming(undefined)
            markOpenSteps('done', 'Completed.', assistantId)
            addAnswerVerificationStep(assistantId, turnTitle, tools.includes('search') || tools.includes('research'))
            // Skip the shared machine + snapshot if the user has since opened
            // another thread — state.turns is now that thread's, so a snapshot
            // here would write the wrong turns and the status flip would clobber
            // the visible thread.
            if (ownsMachine()) {
              setState('status', 'idle')
              writeThreadSnapshot(state.threadId ?? activeThreadId, state.turns)
            }
          },
          onError: ({ message }) => {
            // Thread switched away mid-stream: loadThread already aborted us and
            // reset the shared machine for the new thread, so finalise nothing
            // here (a global error/status write or a fallback retry would land
            // on the wrong thread).
            if (!ownsMachine()) {
              settled = true
              return
            }
            // Graceful model fallback: a pinned model (or Verevon intent mode)
            // whose provider is unavailable fails the whole turn. Retry once with
            // an empty model → inference-core resolves Verevon Balance / a working
            // provider. Only when a model was set (`model` non-empty) — the retry
            // runs with model "" so it can never re-trigger this branch — and
            // never on a user-aborted stream.
            if (model && !controller.signal.aborted) {
              settled = true
              markOpenSteps('stopped', 'Provider unavailable. Retrying with fallback model.', assistantId)
              setState('turns', (turns) => turns.filter((turn) => turn.id !== assistantId))
              setState('status', 'idle')
              void sendContent(content, '', { ...options, appendUser: false })
              return
            }
            settled = true
            setState('error', message)
            setState('status', 'error')
            setState('turns', (turn) => turn.id === assistantId, 'content', (prev) => prev || message)
            stopStreaming('error')
            markOpenSteps('error', message, assistantId)
            writeThreadSnapshot(state.threadId ?? activeThreadId, state.turns)
          },
        },
        controller.signal,
      )

      if (!settled) {
        stopStreaming(undefined)
        markOpenSteps('done', 'Completed.', assistantId)
        addAnswerVerificationStep(assistantId, turnTitle, tools.includes('search') || tools.includes('research'))
        // Same ownership guard as onDone: an aborted stream (e.g. a thread
        // switch) lands here with `!settled`, and must not flip the visible
        // thread's status or snapshot the wrong turns.
        if (ownsMachine()) {
          setState('status', 'idle')
          writeThreadSnapshot(state.threadId ?? activeThreadId, state.turns)
        }
      }
    } catch {
      stopStreaming(controller.signal.aborted ? 'stopped' : 'error')
      // If the user switched threads, this send no longer owns the shared
      // machine — settle its own turn's flag above but never write global
      // status/error or a snapshot onto the now-visible thread.
      if (!ownsMachine()) return
      if (controller.signal.aborted) {
        markOpenSteps('stopped', 'Stopped by the user.', assistantId)
        setState('status', 'idle')
        writeThreadSnapshot(state.threadId ?? activeThreadId, state.turns)
        return
      }
      setState('status', 'error')
      setState('error', 'Stream interrupted')
      markOpenSteps('error', 'Stream interrupted', assistantId)
      writeThreadSnapshot(state.threadId ?? activeThreadId, state.turns)
    }
  }

  const handleComposerSubmit = async (payload: DashboardComposerSubmitPayload) => {
    if (!hasMessages()) triggerLaunchMotion()
    const attachments = await toStreamAttachments(payload.attachments)
    const model = payload.model ?? state.activeModel
    const actions = withBrregLookupAction(
      payload.actions.map((a) => ({ id: a.id, name: a.name, kind: a.kind })),
      payload.text,
    )
    setState('activeModel', model)
    void sendContent(payload.text, model, {
      attachments: attachments.length > 0 ? attachments : undefined,
      browseWeb: payload.tools.includes('search') || payload.tools.includes('research'),
      deepResearch: payload.tools.includes('research'),
      displayAttachments: payload.attachments,
      generateImage: payload.tools.includes('image'),
      tools: payload.tools,
      actions,
      zdr: payload.zdr,
    })
  }

  const handleStop = () => {
    abortController?.abort()
    if (state.requestId) {
      void cancelInvocation(state.requestId).catch(() => undefined)
    }
    setState('turns', (turn) => turn.streaming, 'streaming', false)
    setState('turns', (turn) => turn.status === 'waiting', 'status', 'stopped')
    markOpenSteps('stopped', 'Stopped by the user.')
    setState('status', 'idle')
    if (state.threadId) writeThreadSnapshot(state.threadId, state.turns)
  }

  const addAssistantCitation = (turnId: string, turnTitle: string, citation: Citation) => {
    setState('turns', (turn) => turn.id === turnId, 'citations', (prev) => upsertCitation(prev ?? [], citation))
    appendSearchEvidence(turnId, citation)
    upsertTaskStep(createTurnStep(
      turnId,
      turnTitle,
      `source-${citation.id}`,
      'Source found',
      `${citation.title || hostname(citation.url)} · ${hostname(citation.url)}`,
      'done',
    ))
  }

  const appendSearchEvidence = (turnId: string, citation: Citation) => {
    updateTaskStep(`${turnId}:tool-search`, (step) => {
      const evidence = upsertStepEvidence(step.evidence ?? [], citationEvidence(citation))
      return {
        ...step,
        detail: `Web search captured ${evidence.length} source${evidence.length === 1 ? '' : 's'}.`,
        evidence,
        status: 'done',
      }
    })
  }

  const addAnswerVerificationStep = (turnId: string, turnTitle: string, searchRequested: boolean) => {
    if (!searchRequested) return
    const citations = state.turns.find((turn) => turn.id === turnId)?.citations ?? []
    // Search being available no longer means a search must have run: the
    // backend searches only when the query needs fresh data. Add the
    // verification step only when web sources actually grounded the answer.
    if (citations.length === 0) return
    upsertTaskStep({
      id: `${turnId}:verification`,
      title: 'Answer verification',
      detail: `Checked against ${citations.length} web source${citations.length === 1 ? '' : 's'} shown in Kilder.`,
      evidence: citations.map(citationEvidence),
      status: 'done',
      createdAt: new Date().toISOString(),
      turnId,
      turnTitle,
    })
  }

  const markComposerToolStarted = (turnId: string, toolName: string, args?: unknown) => {
    const composerTool = composerToolIdForToolName(toolName)
    if (!composerTool) return
    const query = composerTool === 'search' ? searchQueryFromArgs(args) : undefined
    updateTaskStep(`${turnId}:tool-${composerTool}`, (step) => ({
      ...step,
      detail: query ? `Searching web for "${query}".` : `${humanizeToolName(toolName)} started by the Model Plane.`,
      expandedDetail: formatToolArgs(args),
      status: 'active',
    }))
  }

  const markComposerToolCompleted = (
    turnId: string,
    toolName: string | undefined,
    error?: string,
    output?: string,
    sourceCount = 0,
  ) => {
    const composerTool = composerToolIdForToolName(toolName)
    if (!composerTool) return
    updateTaskStep(`${turnId}:tool-${composerTool}`, (step) => ({
      ...step,
      detail: searchCompletionDetail(toolName ?? composerTool, Boolean(error), sourceCount, output),
      expandedDetail: output || step.expandedDetail,
      status: error ? 'error' : 'done',
    }))
  }

  const copyTurn = async (turn: ChatTurn) => {
    await navigator.clipboard.writeText(turn.content).catch(() => undefined)
    setCopiedTurnId(turn.id)
    window.setTimeout(() => setCopiedTurnId(null), 1200)
  }

  const dismissFeedbackNotice = () => {
    if (feedbackNoticeTimer !== undefined) window.clearTimeout(feedbackNoticeTimer)
    feedbackNoticeTimer = undefined
    setFeedbackNotice(null)
  }

  const showFeedbackNotice = (message: string) => {
    if (feedbackNoticeTimer !== undefined) window.clearTimeout(feedbackNoticeTimer)
    setFeedbackNotice(message)
    feedbackNoticeTimer = window.setTimeout(() => {
      feedbackNoticeTimer = undefined
      setFeedbackNotice(null)
    }, 6000)
  }

  onCleanup(() => {
    if (feedbackNoticeTimer !== undefined) window.clearTimeout(feedbackNoticeTimer)
  })

  /**
   * Rate one turn. The failure path is the point of this function: the POST used
   * to be fired with `.catch(() => undefined)`, so a rejected rating (every
   * rating, until the contract was fixed) left the thumb lit while nothing was
   * recorded. A rating that does not persist must say so.
   *
   * Resolves `true` only when the rating actually persisted, so the caller can
   * roll back its optimistic highlight. Deliberately reports rather than
   * rethrows: the notice belongs here, in one place, and a caller that ignores
   * the result still gets the message.
   */
  const submitTurnFeedback = async (
    turnId: string,
    rating: ChatFeedbackRating,
  ): Promise<boolean> => {
    const turn = state.turns.find((candidate) => candidate.id === turnId)
    if (!turn?.requestId) {
      showFeedbackNotice('Denne meldingen mangler en referanse å vurdere. Vurderingen ble ikke lagret.')
      return false
    }
    dismissFeedbackNotice()
    try {
      await submitFeedback(turn.requestId, rating, { runId: turn.runId })
      return true
    } catch (error: unknown) {
      showFeedbackNotice(describeFeedbackFailure(error))
      return false
    }
  }

  const regenerateLatest = () => {
    if (isStreaming()) return
    const anchorIndex = lastUserIndex(state.turns)
    if (anchorIndex < 0) return
    const lastUser = state.turns[anchorIndex]
    if (!lastUser) return
    // Snapshot the outgoing exchange as a version BEFORE truncating it away —
    // see chat-versions.ts. Must run before the slice below, which is itself
    // the fix for a real bug this uncovered: `sendContent` with
    // `appendUser: false` only ever APPENDS the new assistant turn, so
    // without this truncation the old answer stayed visible forever and a
    // second, separate answer piled up underneath it.
    setVersionState((prev) => beginNewVersion(prev, unwrap(state.turns), state.threadId))
    setState('turns', (turns) => turns.slice(0, anchorIndex + 1))
    setState('branchCount', (count) => count + 1)
    void sendContent(lastUser.content, lastUser.model, {
      appendUser: false,
      browseWeb: lastUser.tools.includes('search') || lastUser.tools.includes('research'),
      deepResearch: lastUser.tools.includes('research'),
      displayAttachments: lastUser.attachments,
      generateImage: lastUser.tools.includes('image'),
      tools: lastUser.tools,
      // Regenerating within an already-temporary thread must keep sending
      // ZDR — the thread-level lock, not the (possibly since-toggled)
      // composer state, decides.
      zdr: isTemporaryThread(state.threadId),
      // Tells the server this is a replacement, not a new question. It is the
      // only way model-gateway can know: the text is identical. This is what
      // activates SignalKind::Regenerate in the implicit-feedback loop.
      regenerated: true,
    })
  }

  const editAndResubmit = async (turnId: string, text: string) => {
    const index = state.turns.findIndex((turn) => turn.id === turnId)
    const original = state.turns[index]
    const next = text.trim()
    if (!original || original.role !== 'user' || !next) return
    abortController?.abort()
    const attachments = await toStreamAttachments(original.attachments)
    if (index === lastUserIndex(state.turns)) {
      // Editing the FINAL exchange: snapshot it as a version before it's
      // truncated away, same as regenerateLatest. Editing an EARLIER turn
      // truncates everything after it (below) and replaces it wholesale —
      // that path is deliberately NOT versioned (chat-versions.ts: nesting
      // would be possible past this point, which the design rules out).
      setVersionState((prev) => beginNewVersion(prev, unwrap(state.turns), state.threadId))
    } else {
      setVersionState(null)
    }
    setState('turns', (turns) => turns.slice(0, index))
    setState('status', 'idle')
    await sendContent(next, original.model, {
      attachments: attachments.length > 0 ? attachments : undefined,
      browseWeb: original.tools.includes('search') || original.tools.includes('research'),
      deepResearch: original.tools.includes('research'),
      displayAttachments: original.attachments,
      generateImage: original.tools.includes('image'),
      tools: original.tools,
      // Same thread-level ZDR lock as `regenerateLatest`.
      zdr: isTemporaryThread(state.threadId),
      // The edit happened in the composer and never reached the server as a
      // distinct action, so it has to be declared. Activates
      // SignalKind::EditResubmit.
      editResubmit: true,
    })
  }

  const branchAt = (turnId: string) => {
    const index = state.turns.findIndex((turn) => turn.id === turnId)
    if (index < 0) return
    abortController?.abort()
    const sourceWasTemporary = isTemporaryThread(state.threadId)
    const nextThreadId = createId('thread')
    const branchTurns = state.turns.slice(0, index + 1).map((turn) => ({ ...turn, id: createId(turn.role) }))
    setState('turns', branchTurns)
    setState('threadId', nextThreadId)
    setState('status', 'idle')
    setState('requestId', null)
    setState('branchCount', 0)
    setState('taskSteps', [])
    setVersionState(null)
    if (sourceWasTemporary) {
      // The branch copies a temporary thread's own content into a new thread
      // id — it must inherit the ZDR marking, or the calls below would
      // persist exactly the content Temporary Chat exists to keep out of
      // storage (history, transcript, AND the active-thread pointer).
      markThreadTemporary(nextThreadId)
    } else {
      writeThreadSnapshot(nextThreadId, branchTurns)
      setActiveChatThreadId(nextThreadId)
    }
    setActiveTab('chat')
  }

  /** Derived n/N badge for the final exchange, or `null` when it must not render. */
  const finalExchangeVersion = createMemo(() => versionBadge(versionState(), state.turns, state.threadId))

  /** Swap the displayed version of the final exchange to logical position `target` (0-based). */
  const selectExchangeVersion = (target: number) => {
    const result = selectVersion(versionState(), unwrap(state.turns), state.threadId, target)
    if (!result) return
    setVersionState(result.state)
    setState('turns', result.turns)
  }

  const startNewChat = () => {
    clearActiveChatThreadId()
    resetChatState()
  }

  const updateTaskStep = (id: string, update: (step: AgentTaskStep) => AgentTaskStep) => {
    setState('taskSteps', (steps) => steps.map((step) => (
      step.id === id ? update(step) : step
    )))
  }

  const markStepDone = (id: string, detail: string) => {
    updateTaskStep(id, (step) => ({ ...step, status: 'done' as const, detail }))
  }

  const upsertTaskStep = (step: AgentTaskStep) => {
    setState('taskSteps', (steps) => {
      const index = steps.findIndex((item) => item.id === step.id)
      if (index < 0) return [...steps, step]
      return steps.map((item, itemIndex) => itemIndex === index ? { ...item, ...step } : item)
    })
  }

  const markOpenSteps = (status: TaskStepStatus, detail: string, turnId?: string) => {
    setState('taskSteps', (steps) => steps.map((step) => (
      (!turnId || step.turnId === turnId) && (step.status === 'active' || step.status === 'waiting')
        ? missingSearchResultStep(step, status) ?? { ...step, status, detail }
        : step
    )))
  }

  return {
    hasMessages,
    isStreaming,
    evidenceSources,
    latestGrounding,
    artifactItems,
    artifacts,
    latestScreen,
    liveRunId,
    runPanelCollapsed,
    toggleRunPanel,
    title,
    handleScroll,
    scrollToBottom,
    handleApprovalDecision,
    handleComposerSubmit,
    handleStop,
    copyTurn,
    submitTurnFeedback,
    feedbackNotice,
    dismissFeedbackNotice,
    regenerateLatest,
    editAndResubmit,
    branchAt,
    finalExchangeVersion,
    selectExchangeVersion,
    startNewChat,
    state,
    activeTab,
    setActiveTab,
    copiedTurnId,
    launchMotion,
    showScrollDown,
    imageMode,
    setImageMode,
    planMode,
    setPlanMode,
    browseWeb,
    setBrowseWeb,
    temporaryChat,
    setTemporaryChat,
    isActiveThreadTemporary: () => isTemporaryThread(state.threadId),
    temporaryChatLocked: () => isTemporaryThread(state.threadId),
    input,
    setInput,
    setMessageListRef,
  }
}
