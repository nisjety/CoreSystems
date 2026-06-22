import {
  createEffect,
  createMemo,
  createSignal,
  onCleanup,
  onMount,
} from 'solid-js'
import {
  createStore,
} from 'solid-js/store'
import {
  CHAT_ACTIVE_THREAD_CHANGED_EVENT,
  clearActiveChatThreadId,
  readActiveChatThreadId,
  readChatThreadTranscript,
  removeChatThreadHistoryItem,
  removeChatThreadTranscript,
  setActiveChatThreadId,
  upsertChatThreadHistory,
  upsertChatThreadTranscript,
  type ChatThreadHistoryInput,
  type ChatThreadTranscriptStep,
  type ChatThreadTranscriptTurn,
} from '@/features/chat/lib/chat-thread-history'
import {
  withBrregLookupAction,
} from '@/features/chat/lib/brreg-action'
import {
  consumePendingChatLaunch,
} from '@/features/chat/lib/pending-chat-launch'
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
  getChatThreadTranscript,
  getThreadMessages,
  listModels,
  saveChatThreadSnapshot,
  streamChat,
  VELION_BALANCE_MODE_ID,
} from '@/shared/api/chat-client'
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
    // Default to the Velion Balance intent mode; the backend resolves it
    // server-side. `cheapDefaultModelId` reaffirms this on mount.
    activeModel: VELION_BALANCE_MODE_ID,
    branchCount: 0,
  })

  const [activeTab, setActiveTab] = createSignal<ChatTab>('chat')
  const [copiedTurnId, setCopiedTurnId] = createSignal<string | null>(null)
  const [launchMotion, setLaunchMotion] = createSignal(false)
  const [showScrollDown, setShowScrollDown] = createSignal(false)
  const [imageMode, setImageMode] = createSignal(false)
  const [planMode, setPlanMode] = createSignal(false)
  const [browseWeb, setBrowseWeb] = createSignal(readBrowseWebPreference())
  const [input, setInput] = createSignal('')
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

  const writeThreadSnapshot = (
    threadId: string,
    turns: ChatTurn[],
    overrides: Partial<ChatThreadHistoryInput> = {},
    taskSteps: AgentTaskStep[] = state.taskSteps,
    options: { persistServer?: boolean } = {},
  ) => {
    const firstUserTurn = turns.find((turn) => turn.role === 'user')
    const lastTurn = turns.at(-1)
    const title = overrides.title ?? (firstUserTurn ? createPreview(firstUserTurn.content, 48) : createChatTitle(turns))
    const preview = overrides.preview ?? lastTurn?.content
    const updatedAt = overrides.updatedAt ?? lastTurn?.createdAt
    const transcriptTurns = turnsToTranscript(turns)
    const transcriptTaskSteps = taskStepsToTranscript(taskSteps)
    upsertChatThreadHistory({
      threadId,
      title,
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

  const loadThread = async (threadId: string) => {
    setState('threadId', threadId)
    const localCached = readChatThreadTranscript(threadId)
    const serverCached = await getChatThreadTranscript(threadId).catch(() => null)
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
      const serverTurns = dedupeChatTurns(history.map(messageToTurn))
      const turns = serverTurns.length > 0
        ? mergeServerTurnsWithCachedMetadata(serverTurns, cachedTurns)
        : cachedTurns
      setState({ turns, taskSteps: cachedTaskSteps })
      if (turns.length > 0) {
        writeThreadSnapshot(threadId, turns, {}, cachedTaskSteps, { persistServer: false })
      }
    } catch {
      const fallbackTurns = cachedTurns
      setState({ turns: fallbackTurns, taskSteps: cachedTaskSteps })
      if (fallbackTurns.length > 0) {
        writeThreadSnapshot(threadId, fallbackTurns, {}, cachedTaskSteps, { persistServer: false })
      }
    }
  }

  createEffect(() => {
    if (!state.threadId || state.turns.length === 0) return
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
      const storedThread = readActiveChatThreadId()
      if (storedThread) await loadThread(storedThread)

      try {
        const available = await listModels()
        // Default to the Velion Balance intent mode (cost-aware, resolved
        // server-side), never the first (possibly expensive) catalog entry.
        // cheapDefaultModelId is resilient: it always returns the balance mode id.
        const cheapId = cheapDefaultModelId(available)
        if (cheapId) setState('activeModel', cheapId)
      } catch {
        // The chat can still run with the gateway default model.
      }

      const pending = consumePendingChatLaunch()
      if (pending) {
        if (pending.model) setState('activeModel', pending.model)
        setBrowseWeb(Boolean(pending.tools?.includes('search') || pending.tools?.includes('research')))
        const attachments = await toStreamAttachments(pending.attachments ?? [])
        triggerLaunchMotion()
        await sendContent(pending.text, pending.model, {
          attachments: attachments.length > 0 ? attachments : undefined,
          browseWeb: pending.tools?.includes('search') || pending.tools?.includes('research'),
          displayAttachments: pending.attachments ?? [],
          generateImage: pending.tools?.includes('image'),
          tools: pending.tools ?? [],
          actions: (pending.actions ?? []).map((a) => ({ id: a.id, name: a.name, kind: a.kind })),
        })
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
    if (!state.threadId) {
      setState('threadId', activeThreadId)
      setActiveChatThreadId(activeThreadId)
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
          threadId: activeThreadId,
          sessionKey: activeThreadId,
          browseWeb: options.browseWeb,
          generateImage: options.generateImage,
          attachments: options.attachments,
          actions: options.actions,
          planMode: planMode(),
        },
        {
          onConnected: ({ requestId, threadId: serverThreadId, model: connectedModel }) => {
            captureRequestId(requestId)
            if (serverThreadId) {
              if (serverThreadId !== activeThreadId) {
                const provisionalThreadId = activeThreadId
                removeChatThreadHistoryItem(provisionalThreadId)
                removeChatThreadTranscript(provisionalThreadId)
                void deleteChatThread(provisionalThreadId).catch(() => undefined)
              }
              activeThreadId = serverThreadId
              setState('threadId', serverThreadId)
              setActiveChatThreadId(serverThreadId)
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
            setState('turns', (turn) => turn.id === assistantId, 'artifacts', (prev) => upsertArtifact(prev ?? [], artifact))
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
          onDone: ({ requestId, modelUsed, outputTokens }) => {
            settled = true
            captureRequestId(requestId)
            if (modelUsed) setState('turns', (turn) => turn.id === assistantId, 'modelUsed', modelUsed)
            if (outputTokens != null) setState('turns', (turn) => turn.id === assistantId, 'outputTokens', outputTokens)
            stopStreaming(undefined)
            markOpenSteps('done', 'Completed.', assistantId)
            addAnswerVerificationStep(assistantId, turnTitle, tools.includes('search') || tools.includes('research'))
            setState('status', 'idle')
            writeThreadSnapshot(state.threadId ?? activeThreadId, state.turns)
          },
          onError: ({ message }) => {
            // Graceful model fallback: a pinned model (or Velion intent mode)
            // whose provider is unavailable fails the whole turn. Retry once with
            // an empty model → inference-core resolves Velion Balance / a working
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
        setState('status', 'idle')
        writeThreadSnapshot(state.threadId ?? activeThreadId, state.turns)
      }
    } catch {
      stopStreaming(controller.signal.aborted ? 'stopped' : 'error')
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
      displayAttachments: payload.attachments,
      generateImage: payload.tools.includes('image'),
      tools: payload.tools,
      actions,
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
    upsertTaskStep({
      id: `${turnId}:verification`,
      title: 'Answer verification',
      detail: citations.length > 0
        ? `Checked against ${citations.length} web source${citations.length === 1 ? '' : 's'} shown in Kilder.`
        : 'Could not verify: no web source event was received for this answer.',
      expandedDetail: citations.length > 0
        ? undefined
        : 'Search was active for this answer, but the stream did not include a web_search tool result or citation event. The answer may still contain the model response, but it should be treated as unverified until the backend emits searchable source evidence.',
      evidence: citations.map(citationEvidence),
      status: citations.length > 0 ? 'done' : 'stopped',
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

  const regenerateLatest = () => {
    if (isStreaming()) return
    const lastUser = [...state.turns].reverse().find((turn) => turn.role === 'user')
    if (!lastUser) return
    setState('branchCount', (count) => count + 1)
    void sendContent(lastUser.content, lastUser.model, {
      appendUser: false,
      browseWeb: lastUser.tools.includes('search') || lastUser.tools.includes('research'),
      displayAttachments: lastUser.attachments,
      generateImage: lastUser.tools.includes('image'),
      tools: lastUser.tools,
    })
  }

  const editAndResubmit = async (turnId: string, text: string) => {
    const index = state.turns.findIndex((turn) => turn.id === turnId)
    const original = state.turns[index]
    const next = text.trim()
    if (!original || original.role !== 'user' || !next) return
    abortController?.abort()
    const attachments = await toStreamAttachments(original.attachments)
    setState('turns', (turns) => turns.slice(0, index))
    setState('status', 'idle')
    await sendContent(next, original.model, {
      attachments: attachments.length > 0 ? attachments : undefined,
      browseWeb: original.tools.includes('search') || original.tools.includes('research'),
      displayAttachments: original.attachments,
      generateImage: original.tools.includes('image'),
      tools: original.tools,
    })
  }

  const branchAt = (turnId: string) => {
    const index = state.turns.findIndex((turn) => turn.id === turnId)
    if (index < 0) return
    abortController?.abort()
    const nextThreadId = createId('thread')
    const branchTurns = state.turns.slice(0, index + 1).map((turn) => ({ ...turn, id: createId(turn.role) }))
    setState('turns', branchTurns)
    setState('threadId', nextThreadId)
    setState('status', 'idle')
    setState('requestId', null)
    setState('branchCount', 0)
    setState('taskSteps', [])
    writeThreadSnapshot(nextThreadId, branchTurns)
    setActiveChatThreadId(nextThreadId)
    setActiveTab('chat')
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
    title,
    handleScroll,
    scrollToBottom,
    handleApprovalDecision,
    handleComposerSubmit,
    handleStop,
    copyTurn,
    regenerateLatest,
    editAndResubmit,
    branchAt,
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
    input,
    setInput,
    setMessageListRef,
  }
}
