import {
  createEffect,
  createMemo,
  createSignal,
  For,
  type JSX,
  Match,
  onCleanup,
  onMount,
  Show,
  Switch,
} from 'solid-js'
import { createStore } from 'solid-js/store'
import {
  AlertCircle,
  ArrowDown,
  Brain,
  Briefcase,
  Check,
  CheckCircle2,
  ChevronRight,
  Clock3,
  Code2,
  Copy,
  FileCode2,
  GraduationCap,
  Laptop,
  Link2,
  ListChecks,
  MessageSquare,
  MessageSquarePlus,
  MoreHorizontal,
  Palette,
  Paperclip,
  PenLine,
  Pencil,
  Presentation,
  RefreshCw,
  Sparkles,
  Square,
  ThumbsDown,
  ThumbsUp,
  Volume2,
  WandSparkles,
  Wrench,
  X,
  type LucideProps,
} from 'lucide-solid'
import { consumePendingChatLaunch } from '@/features/chat/lib/pending-chat-launch'
import { DashboardComposer, type DashboardComposerSubmitPayload } from '@/features/dashboard/home/DashboardComposer'
import {
  decideApproval,
  listApprovals,
  resumeRun,
  type Approval,
  type ApprovalDecision,
} from '@/shared/api/orchestration-client'
import {
  cancelInvocation,
  cheapDefaultModelId,
  getThreadMessages,
  listModels,
  streamChat,
  submitFeedback,
  VELION_BALANCE_MODE_ID,
  type ChatAction,
  type ChatMessage,
} from '@/shared/api/chat-client'
import { blobToDataUrl, parseDataUrl } from '@/shared/lib/blob-data'
import { readClientValue, removeClientValue, writeClientValue } from '@/shared/session/client-storage'

// ── Types ─────────────────────────────────────────────────────────────────────

type ComposerToolId = DashboardComposerSubmitPayload['tools'][number]
type ComposerAttachment = DashboardComposerSubmitPayload['attachments'][number]
type ChatTab = 'chat' | 'sources' | 'artifacts' | 'steps'
type TaskStepStatus = 'done' | 'active' | 'waiting' | 'error' | 'stopped'
type IconComponent = (props: LucideProps) => JSX.Element

type ChatArtifact = {
  id: string
  kind: string
  content: string
  title: string
  version: number
}

type ChatToolCall = {
  id: string
  name: string
  args?: unknown
  status?: string
  output?: string
  error?: string
}

type Citation = {
  id: string
  title: string
  url: string
  snippet: string
}

type GeneratedFile = {
  id: string
  name: string
  mime: string
  size: number
  url: string
}

type ChatGroundingSource = {
  id: string
  kind: 'knowledge'
  title: string
  snippet: string
  provider: string
  sourceType: string
  documentId: string
  href: string
  score: number
}

type ChatGroundingFact = {
  knowledgeId: string
  documentId: string
  text: string
  score: number
  sourceTitle: string
  sourceType: string
  provider: string
  chunkIndex: number
}

type ChatGroundingGraphNode = {
  id: string
  label: string
  kind: string
}

type ChatGroundingGraph = {
  traceId?: string
  communitySummaries: string[]
  edgeCount: number
  nodes: ChatGroundingGraphNode[]
}

type ChatKnowledgeGrounding = {
  mode: 'retrieve' | 'hybrid'
  query: string
  traceId?: string
  lowConfidence: boolean
  factCount: number
  sourceCount: number
  facts: ChatGroundingFact[]
  sources: ChatGroundingSource[]
  graph?: ChatGroundingGraph
}

type ChatTurn = {
  id: string
  role: 'user' | 'assistant'
  content: string
  createdAt: string
  streaming: boolean
  tools: ComposerToolId[]
  attachments: ComposerAttachment[]
  status?: 'waiting' | 'stopped' | 'error'
  model?: string
  requestId?: string
  modelUsed?: string
  inputTokens?: number
  outputTokens?: number
  latencyMs?: number
  costUsd?: number
  confidence?: number
  reasoning?: string
  citations?: Citation[]
  toolCalls?: ChatToolCall[]
  artifacts?: ChatArtifact[]
  files?: GeneratedFile[]
  grounding?: ChatKnowledgeGrounding
  /** Orchestration run id (captured from a `paused` step) — drives approvals. */
  runId?: string
  /** Pending human-approval requests gating this agentic run's next tool. */
  pendingApprovals?: Approval[]
}

type AgentTaskStep = {
  id: string
  title: string
  detail: string
  status: TaskStepStatus
  createdAt: string
}

type ChatStatus = 'idle' | 'streaming' | 'error'

type ChatState = {
  turns: ChatTurn[]
  taskSteps: AgentTaskStep[]
  status: ChatStatus
  error: string | null
  requestId: string | null
  threadId: string | null
  activeModel: string
  branchCount: number
}

type StreamAttachment = {
  data_base64: string
  kind: 'image'
  mime_type: string
}

type SendOptions = {
  actions?: ChatAction[]
  attachments?: StreamAttachment[]
  browseWeb?: boolean
  createdAt?: string
  displayAttachments?: ComposerAttachment[]
  generateImage?: boolean
  appendUser?: boolean
  tools?: ComposerToolId[]
}

type EvidenceSource = (Citation & { kind: 'web' }) | ChatGroundingSource

type MarkdownBlock =
  | { kind: 'code'; lang: string; text: string }
  | { kind: 'heading'; level: 1 | 2 | 3; text: string }
  | { kind: 'hr' }
  | { kind: 'list'; ordered: boolean; items: string[] }
  | { kind: 'paragraph'; text: string }
  | { kind: 'quote'; text: string }

const chatThreadKey = 'velion.chat.threadId'

// ── Prompt chips ──────────────────────────────────────────────────────────────

const PRIMARY_PROMPTS = [
  { label: 'Create slides', prompt: 'Create a concise slide outline for a customer support leadership update.', icon: Presentation },
  { label: 'Build website', prompt: 'Build a focused website plan for a high-converting support automation page.', icon: Code2 },
  { label: 'Develop apps', prompt: 'Plan a desktop app workflow for agents managing customer conversations.', icon: Laptop },
  { label: 'Design', prompt: 'Design a refined support workflow with clear states, handoffs, and escalation paths.', icon: Palette },
  { label: 'Write', prompt: 'Write a polished customer reply that is concise, helpful, and on-brand.', icon: PenLine },
] as const

const OVERFLOW_PROMPTS = [
  { label: 'Learn', prompt: 'Teach me the most important concepts behind customer support automation.', icon: GraduationCap },
  { label: 'Code', prompt: 'Help me implement a clean customer support automation feature with tests.', icon: Code2 },
  { label: 'Career chat', prompt: 'Help me prepare for a career conversation about customer experience leadership.', icon: Briefcase },
  { label: "Velion's choice", prompt: 'Choose the highest-impact next task for improving our support operations.', icon: WandSparkles },
] as const

const TOOL_LABELS: Record<ComposerToolId, string> = {
  image: 'Create image',
  reason: 'Reason',
  research: 'Deep research',
  search: 'Search',
}

const PROSE_ARTIFACT_KINDS = new Set(['markdown', 'md', 'doc', 'text', 'report', 'prose'])

// ── Page ──────────────────────────────────────────────────────────────────────

export default function ChatPage() {
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
  const [input, setInput] = createSignal('')
  let abortController: AbortController | undefined
  let messageListRef!: HTMLDivElement
  const autoFollow = { current: true }

  const hasMessages = () => state.turns.length > 0
  const isStreaming = () => state.status === 'streaming'
  const evidenceSources = createMemo(() => collectEvidenceSources(state.turns))
  const latestGrounding = createMemo(() => collectLatestGrounding(state.turns))
  const artifacts = createMemo(() => collectArtifacts(state.turns))
  const latestScreen = createMemo(() => selectLatestImageArtifact(state.turns))
  const title = () => createChatTitle(state.turns)

  onMount(async () => {
    const storedThread = readClientValue(chatThreadKey)
    if (storedThread) {
      setState('threadId', storedThread)
      try {
        const history = await getThreadMessages(storedThread)
        setState('turns', history.map(messageToTurn))
      } catch {
        removeClientValue(chatThreadKey)
        setState('threadId', null)
      }
    }

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
    const dist = messageListRef.scrollHeight - messageListRef.scrollTop - messageListRef.clientHeight
    autoFollow.current = dist < 80
    setShowScrollDown(dist > 160)
  }

  const scrollToBottom = (behavior: ScrollBehavior = 'smooth') => {
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

    let threadId = state.threadId
    if (!threadId) {
      threadId = createId('thread')
      setState('threadId', threadId)
      writeClientValue(chatThreadKey, threadId)
    }

    const submittedAt = options.createdAt ?? new Date().toISOString()
    const model = modelOverride ?? state.activeModel
    const assistantId = createId('asst')
    const tools = options.tools ?? []
    const displayAttachments = options.displayAttachments ?? []
    const appendUser = options.appendUser !== false

    setActiveTab('chat')
    setState('turns', (turns) => [
      ...turns,
      ...(appendUser
        ? [{
            id: createId('user'),
            role: 'user' as const,
            content,
            createdAt: submittedAt,
            streaming: false,
            model,
            tools,
            attachments: displayAttachments,
          }]
        : []),
      {
        id: assistantId,
        role: 'assistant' as const,
        content: '',
        createdAt: new Date().toISOString(),
        streaming: true,
        status: 'waiting' as const,
        model,
        tools,
        attachments: [],
      },
    ])
    setState('taskSteps', buildTaskSteps(content, tools, appendUser ? 'submit' : 'regenerate'))
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
          threadId,
          sessionKey: threadId,
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
              setState('threadId', serverThreadId)
              writeClientValue(chatThreadKey, serverThreadId)
            }
            if (connectedModel) {
              setState('turns', (turn) => turn.id === assistantId, 'modelUsed', connectedModel)
            }
            markStepDone('connect', 'Connected to the live agent stream.')
          },
          onMessage: ({ content: delta, requestId }) => {
            captureRequestId(requestId)
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
            setState('turns', (turn) => turn.id === assistantId, 'citations', (prev) => upsertCitation(prev ?? [], citation))
          },
          onGrounding: ({ value }) => {
            const grounding = normalizeGrounding(value)
            if (!grounding) return
            setState('turns', (turn) => turn.id === assistantId, 'grounding', grounding)
          },
          onReasoning: ({ delta }) => {
            if (!delta) return
            setState('turns', (turn) => turn.id === assistantId, 'reasoning', (prev = '') => prev + delta)
          },
          onStep: (event) => {
            const step = normalizeStep(event)
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
            upsertTaskStep({
              id: `tool-${call.id}`,
              title: call.name,
              detail: 'Tool call running.',
              status: 'active',
              createdAt: new Date().toISOString(),
            })
          },
          onToolResult: (event) => {
            if (!event.id) return
            setState('turns', (turn) => turn.id === assistantId, 'toolCalls', (prev) => applyToolResult(prev ?? [], event))
            upsertTaskStep({
              id: `tool-${event.id}`,
              title: 'Tool result',
              detail: event.error ?? event.output ?? 'Tool call completed.',
              status: event.error ? 'error' : 'done',
              createdAt: new Date().toISOString(),
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
          },
          onDone: ({ requestId, modelUsed, outputTokens }) => {
            settled = true
            captureRequestId(requestId)
            if (modelUsed) setState('turns', (turn) => turn.id === assistantId, 'modelUsed', modelUsed)
            if (outputTokens != null) setState('turns', (turn) => turn.id === assistantId, 'outputTokens', outputTokens)
            stopStreaming(undefined)
            markOpenSteps('done', 'Completed.')
            setState('status', 'idle')
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
              setState('turns', (turns) => turns.filter((turn) => turn.id !== assistantId))
              void sendContent(content, '', { ...options, appendUser: false })
              return
            }
            settled = true
            setState('error', message)
            setState('status', 'error')
            setState('turns', (turn) => turn.id === assistantId, 'content', (prev) => prev || message)
            stopStreaming('error')
            markOpenSteps('error', message)
          },
        },
        controller.signal,
      )

      if (!settled) {
        stopStreaming(undefined)
        markOpenSteps('done', 'Completed.')
        setState('status', 'idle')
      }
    } catch {
      stopStreaming(controller.signal.aborted ? 'stopped' : 'error')
      if (controller.signal.aborted) {
        markOpenSteps('stopped', 'Stopped by the user.')
        setState('status', 'idle')
        return
      }
      setState('status', 'error')
      setState('error', 'Stream interrupted')
      markOpenSteps('error', 'Stream interrupted')
    }
  }

  const handleComposerSubmit = async (payload: DashboardComposerSubmitPayload) => {
    if (!hasMessages()) triggerLaunchMotion()
    const attachments = await toStreamAttachments(payload.attachments)
    const model = payload.model ?? state.activeModel
    setState('activeModel', model)
    void sendContent(payload.text, model, {
      attachments: attachments.length > 0 ? attachments : undefined,
      browseWeb: payload.tools.includes('search') || payload.tools.includes('research'),
      displayAttachments: payload.attachments,
      generateImage: payload.tools.includes('image'),
      tools: payload.tools,
      actions: payload.actions.map((a) => ({ id: a.id, name: a.name, kind: a.kind })),
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
    setState('turns', state.turns.slice(0, index + 1).map((turn) => ({ ...turn, id: createId(turn.role) })))
    setState('threadId', nextThreadId)
    setState('status', 'idle')
    setState('requestId', null)
    setState('branchCount', 0)
    setState('taskSteps', [])
    writeClientValue(chatThreadKey, nextThreadId)
    setActiveTab('chat')
  }

  const startNewChat = () => {
    abortController?.abort()
    removeClientValue(chatThreadKey)
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

  const markStepDone = (id: string, detail: string) => {
    setState('taskSteps', (steps) => steps.map((step) => (
      step.id === id ? { ...step, status: 'done' as const, detail } : step
    )))
  }

  const upsertTaskStep = (step: AgentTaskStep) => {
    setState('taskSteps', (steps) => {
      const index = steps.findIndex((item) => item.id === step.id)
      if (index < 0) return [...steps, step]
      return steps.map((item, itemIndex) => itemIndex === index ? { ...item, ...step } : item)
    })
  }

  const markOpenSteps = (status: TaskStepStatus, detail: string) => {
    setState('taskSteps', (steps) => steps.map((step) => (
      step.status === 'active' || step.status === 'waiting'
        ? { ...step, status, detail }
        : step
    )))
  }

  const composer = () => (
    <DashboardComposer
      imageMode={imageMode()}
      message={input()}
      onImageModeChange={setImageMode}
      onMessageChange={setInput}
      onPlanModeChange={setPlanMode}
      onStop={handleStop}
      onSubmit={handleComposerSubmit}
      planMode={planMode()}
      showTurnReceipt={false}
      submitting={isStreaming()}
    />
  )

  return (
    <div class={`velion-chat-page${launchMotion() ? ' velion-chat-page-launch' : ''}`}>
      <Show when={launchMotion()}>
        <div class="velion-chat-launch-wash" aria-hidden="true" />
      </Show>

      <section class="velion-chat-section" aria-label="Velion chat workspace">
        <Show when={hasMessages()}>
          <ChatHeader
            branchCount={state.branchCount}
            messageCount={state.turns.length}
            title={title()}
            onNewChat={startNewChat}
            onRegenerate={regenerateLatest}
          />
          <ChatTabs
            active={activeTab()}
            artifactCount={artifacts().length}
            sourceCount={evidenceSources().length}
            stepCount={state.taskSteps.length}
            onChange={setActiveTab}
          />
        </Show>

        <Switch>
          <Match when={!hasMessages()}>
            <EmptyChatState onSelectPrompt={setInput}>{composer()}</EmptyChatState>
          </Match>
          <Match when={activeTab() === 'chat'}>
            <div ref={messageListRef} class="velion-chat-message-list" onScroll={handleScroll}>
              <div class="velion-chat-thread">
                <For each={state.turns}>
                  {(turn, index) => (
                    <>
                      <Show when={shouldShowDateDivider(state.turns[index() - 1], turn)}>
                        <DateDivider value={turn.createdAt} />
                      </Show>
                      <MessageBlock
                        copied={copiedTurnId() === turn.id}
                        message={turn}
                        onBranch={() => branchAt(turn.id)}
                        onCopy={() => void copyTurn(turn)}
                        onEdit={(text) => void editAndResubmit(turn.id, text)}
                        onRegenerate={regenerateLatest}
                        onFeedback={(rating) => {
                          if (turn.requestId) void submitFeedback(turn.requestId, rating).catch(() => undefined)
                        }}
                        onApprovalDecision={(approvalId, decision) =>
                          void handleApprovalDecision(turn.id, approvalId, decision)
                        }
                      />
                    </>
                  )}
                </For>
                <Show when={state.error && state.status === 'error'}>
                  <div class="velion-chat-error" role="alert">{state.error}</div>
                </Show>
              </div>
            </div>
          </Match>
          <Match when={activeTab() === 'sources'}>
            <SourcesPanel grounding={latestGrounding()} sources={evidenceSources()} />
          </Match>
          <Match when={activeTab() === 'artifacts'}>
            <ArtifactsPanel artifacts={artifacts()} />
          </Match>
          <Match when={activeTab() === 'steps'}>
            <StepsPanel steps={state.taskSteps} screen={latestScreen()} onStopTask={handleStop} />
          </Match>
        </Switch>

        <Show when={hasMessages() && activeTab() === 'chat'}>
          <div class="velion-chat-composer-dock">
            <Show when={showScrollDown()}>
              <button
                type="button"
                class="velion-chat-scroll-down"
                aria-label="Scroll to bottom"
                onClick={() => scrollToBottom()}
              >
                <ArrowDown size={16} />
              </button>
            </Show>
            <div class="velion-chat-composer-dock__inner">
              <Show when={isStreaming()}>
                <div class="velion-chat-stop-wrap">
                  <button type="button" class="velion-chat-stop-btn" onClick={handleStop}>
                    <Square size={12} />
                    Stopp svar
                  </button>
                </div>
              </Show>
              {composer()}
            </div>
          </div>
        </Show>
      </section>
    </div>
  )
}

// ── Header / tabs / panels ───────────────────────────────────────────────────

function ChatHeader(props: {
  branchCount: number
  messageCount: number
  title: string
  onNewChat: () => void
  onRegenerate: () => void
}) {
  return (
    <header class="velion-chat-header">
      <div class="velion-chat-header__copy">
        <h1>{props.title}</h1>
        <p>{props.messageCount} messages · {props.branchCount} regenerations</p>
      </div>
      <div class="velion-chat-header__actions">
        <button type="button" class="velion-chat-header-button" aria-label="Regenerate latest response" onClick={() => props.onRegenerate()}>
          <RefreshCw size={15} />
        </button>
        <button type="button" class="velion-chat-header-button velion-chat-header-button--primary" aria-label="New chat" onClick={() => props.onNewChat()}>
          <MessageSquarePlus size={15} />
        </button>
      </div>
    </header>
  )
}

function ChatTabs(props: {
  active: ChatTab
  artifactCount: number
  sourceCount: number
  stepCount: number
  onChange: (tab: ChatTab) => void
}) {
  const tabs = (): Array<{ id: ChatTab; label: string; icon: IconComponent; count: number }> => [
    { id: 'chat', label: 'Chat', icon: MessageSquare, count: 0 },
    { id: 'sources', label: 'Kilder', icon: Link2, count: props.sourceCount },
    { id: 'artifacts', label: 'Artefakter', icon: FileCode2, count: props.artifactCount },
    { id: 'steps', label: 'Steg', icon: ListChecks, count: props.stepCount },
  ]

  return (
    <div class="velion-chat-tabs" role="tablist" aria-label="Chat workspace views">
      <For each={tabs()}>
        {(item) => {
          const Icon = item.icon
          const selected = () => props.active === item.id
          return (
            <button
              type="button"
              role="tab"
              aria-selected={selected()}
              classList={{ 'velion-chat-tab': true, 'velion-chat-tab--active': selected() }}
              onClick={() => props.onChange(item.id)}
            >
              <Icon size={14} />
              <span>{item.label}</span>
              <Show when={item.count > 0}>
                <em>{item.count}</em>
              </Show>
            </button>
          )
        }}
      </For>
    </div>
  )
}

function EmptyPanel(props: { icon: JSX.Element; title: string; subtitle: string }) {
  return (
    <div class="velion-chat-empty-panel">
      <div>
        <span class="velion-chat-empty-panel__icon">{props.icon}</span>
        <h2>{props.title}</h2>
        <p>{props.subtitle}</p>
      </div>
    </div>
  )
}

function SourcesPanel(props: { grounding?: ChatKnowledgeGrounding | null; sources: EvidenceSource[] }) {
  return (
    <Show
      when={props.sources.length > 0 || props.grounding}
      fallback={(
        <EmptyPanel
          icon={<Link2 size={20} />}
          title="Ingen kilder ennå"
          subtitle="Interne kunnskapskilder og websøk dukker opp her når Velion bruker dem i svaret."
        />
      )}
    >
      <div class="velion-chat-panel">
        <div class="velion-chat-panel__inner">
          <Show when={props.grounding}>
            {(grounding) => <GroundingOverviewCard grounding={grounding()} />}
          </Show>
          <For each={props.sources}>
            {(source, index) => (
              source.kind === 'knowledge'
                ? <KnowledgeSourceCard source={source} index={index() + 1} />
                : <WebSourceCard source={source} index={index() + 1} />
            )}
          </For>
        </div>
      </div>
    </Show>
  )
}

function GroundingOverviewCard(props: { grounding: ChatKnowledgeGrounding }) {
  return (
    <section class="velion-chat-source-summary">
      <div class="velion-chat-source-summary__badges">
        <span><Sparkles size={14} /> Internal knowledge grounding</span>
        <Show when={props.grounding.lowConfidence}>
          <em>Low confidence</em>
        </Show>
      </div>
      <div class="velion-chat-source-summary__metrics">
        <Metric label="Sources" value={String(props.grounding.sourceCount)} />
        <Metric label="Facts" value={String(props.grounding.factCount)} />
        <Metric label="Graph nodes" value={String(props.grounding.graph?.nodes.length ?? 0)} />
      </div>
      <Show when={props.grounding.graph}>
        {(graph) => <GroundingGraphSummary graph={graph()} traceId={props.grounding.traceId} />}
      </Show>
    </section>
  )
}

function Metric(props: { label: string; value: string }) {
  return (
    <div class="velion-chat-metric">
      <span>{props.label}</span>
      <strong>{props.value}</strong>
    </div>
  )
}

function GroundingGraphSummary(props: { compact?: boolean; graph: ChatGroundingGraph; traceId?: string }) {
  return (
    <div classList={{ 'velion-chat-graph-summary': true, 'velion-chat-graph-summary--compact': props.compact }}>
      <div>
        <strong>Graph evidence</strong>
        <Show when={props.traceId}><span>Trace {props.traceId}</span></Show>
      </div>
      <For each={props.graph.communitySummaries}>
        {(summary) => <p>{summary}</p>}
      </For>
      <Show when={props.graph.nodes.length > 0}>
        <div class="velion-chat-graph-summary__nodes">
          <For each={props.graph.nodes}>
            {(node) => <span>{node.label}</span>}
          </For>
        </div>
      </Show>
    </div>
  )
}

function KnowledgeSourceCard(props: { source: ChatGroundingSource; index: number }) {
  return (
    <article class="velion-chat-source-card">
      <div class="velion-chat-source-card__meta">
        <span>{props.index}</span>
        <em>{props.source.provider} · {props.source.sourceType}</em>
        <strong>Score {props.source.score.toFixed(2)}</strong>
      </div>
      <h2>{props.source.title}</h2>
      <p>{props.source.snippet}</p>
      <a href={props.source.href}>Open knowledge <ChevronRight size={14} /></a>
    </article>
  )
}

function WebSourceCard(props: { source: Citation & { kind: 'web' }; index: number }) {
  return (
    <a class="velion-chat-source-card" href={props.source.url} target="_blank" rel="noopener noreferrer">
      <div class="velion-chat-source-card__meta">
        <span>{props.index}</span>
        <em>{hostname(props.source.url)}</em>
      </div>
      <h2>{props.source.title || props.source.url}</h2>
      <Show when={props.source.snippet}><p>{props.source.snippet}</p></Show>
    </a>
  )
}

function ArtifactsPanel(props: { artifacts: ChatArtifact[] }) {
  return (
    <Show
      when={props.artifacts.length > 0}
      fallback={(
        <EmptyPanel
          icon={<FileCode2 size={20} />}
          title="Ingen artefakter ennå"
          subtitle="Dokumenter, kode, bilder og andre artefakter Velion lager dukker opp her."
        />
      )}
    >
      <div class="velion-chat-panel">
        <div class="velion-chat-panel__inner velion-chat-panel__inner--wide">
          <For each={props.artifacts}>
            {(artifact) => <ArtifactCard artifact={artifact} />}
          </For>
        </div>
      </div>
    </Show>
  )
}

function ArtifactCard(props: { artifact: ChatArtifact }) {
  const [open, setOpen] = createSignal(true)
  const kind = () => props.artifact.kind.toLowerCase()
  const isImage = () => kind() === 'image'
  const isProse = () => PROSE_ARTIFACT_KINDS.has(kind())

  return (
    <article class="velion-chat-artifact-card">
      <button type="button" aria-expanded={open()} onClick={() => setOpen((value) => !value)}>
        <FileCode2 size={16} />
        <span>{props.artifact.title || props.artifact.kind}</span>
        <em>{props.artifact.kind}</em>
        <Show when={props.artifact.version > 0}>
          <small>v{props.artifact.version}</small>
        </Show>
        <ChevronRight size={16} classList={{ 'velion-chat-rotate': open() }} />
      </button>
      <Show when={open()}>
        <div class="velion-chat-artifact-card__body">
          <Show
            when={isImage()}
            fallback={isProse()
              ? <ChatMarkdown content={props.artifact.content} />
              : <pre>{props.artifact.content}</pre>}
          >
            <img src={imageArtifactSrc(props.artifact.content)} alt={props.artifact.title || 'Generert bilde'} />
          </Show>
        </div>
      </Show>
    </article>
  )
}

function StepsPanel(props: { steps: AgentTaskStep[]; screen?: ChatArtifact | null; onStopTask: () => void }) {
  const activeTask = () => props.steps.some((step) => step.status === 'active' || step.status === 'waiting')
  return (
    <Show
      when={props.steps.length > 0 || props.screen}
      fallback={(
        <EmptyPanel
          icon={<ListChecks size={20} />}
          title="Ingen steg ennå"
          subtitle="Agentens arbeidssteg vises her mens en oppgave kjører."
        />
      )}
    >
      <div class="velion-chat-panel">
        <div class="velion-chat-panel__inner">
          <div class="velion-chat-steps-header">
            <div>
              <h2>Agent activity</h2>
              <p>Live oppgavestatus</p>
            </div>
            <button type="button" disabled={!activeTask()} onClick={() => props.onStopTask()}>
              <Square size={12} />
              Stopp
            </button>
          </div>
          <Show when={props.screen}>
            {(screen) => (
              <figure class="velion-chat-agent-screen">
                <img src={imageArtifactSrc(screen().content)} alt={screen().title || 'Agent screen'} />
                <figcaption>{screen().title || 'Live screen'}</figcaption>
              </figure>
            )}
          </Show>
          <div class="velion-chat-step-list">
            <For each={props.steps}>
              {(step, index) => <TaskStep step={step} isLast={index() === props.steps.length - 1} />}
            </For>
          </div>
        </div>
      </div>
    </Show>
  )
}

// ── Message components ───────────────────────────────────────────────────────

function MessageBlock(props: {
  copied: boolean
  message: ChatTurn
  onBranch: () => void
  onCopy: () => void
  onEdit: (text: string) => void
  onFeedback: (rating: 'positive' | 'negative') => void
  onRegenerate: () => void
  onApprovalDecision: (approvalId: string, decision: ApprovalDecision) => void
}) {
  return (
    <Show when={props.message.role === 'assistant'} fallback={<UserMessage {...props} />}>
      <AssistantMessage {...props} />
    </Show>
  )
}

function AssistantMessage(props: {
  copied: boolean
  message: ChatTurn
  onBranch: () => void
  onCopy: () => void
  onFeedback: (rating: 'positive' | 'negative') => void
  onRegenerate: () => void
  onApprovalDecision: (approvalId: string, decision: ApprovalDecision) => void
}) {
  const [reaction, setReaction] = createSignal<'up' | 'down' | null>(null)
  const waiting = () => props.message.status === 'waiting'
  const errored = () => props.message.status === 'error'
  const stopped = () => props.message.status === 'stopped'
  const emptyWaiting = () => waiting() && !props.message.content && !props.message.reasoning
  const artifactCount = () => props.message.artifacts?.length ?? 0

  return (
    <article class="velion-chat-message velion-chat-message--assistant">
      <div class="velion-chat-message__avatar">
        <Sparkles size={14} strokeWidth={1.8} />
      </div>
      <div class="velion-chat-message__body">
        <div class="velion-chat-message__heading">
          <span>Velion</span>
          <time>{formatRelative(props.message.createdAt)}</time>
        </div>
        <Show when={props.message.reasoning}>
          {(reasoning) => <ReasoningTrace text={reasoning()} streaming={waiting()} />}
        </Show>
        <Show
          when={!emptyWaiting()}
          fallback={<ThinkingDots />}
        >
          <Show
            when={!errored()}
            fallback={<ErrorNotice message={props.message.content || 'Stream error'} onRetry={props.onRegenerate} />}
          >
            <div classList={{ 'velion-chat-streaming': waiting() }}>
              <Show when={props.message.content}>
                <ChatMarkdown content={props.message.content} />
              </Show>
              <Show when={stopped()}>
                <span class="velion-chat-status-chip"><Square size={12} /> Stoppet</span>
              </Show>
            </div>
          </Show>
        </Show>
        <Show when={props.message.grounding}>
          {(grounding) => <GroundingInlineSummary grounding={grounding()} />}
        </Show>
        <ToolChips tools={props.message.tools} />
        <AttachmentChips attachments={props.message.attachments} tone="assistant" />
        <Show when={(props.message.toolCalls?.length ?? 0) > 0}>
          <ToolCallList calls={props.message.toolCalls ?? []} />
        </Show>
        <Show when={(props.message.pendingApprovals?.length ?? 0) > 0}>
          <ApprovalRequests
            approvals={props.message.pendingApprovals ?? []}
            onDecide={props.onApprovalDecision}
          />
        </Show>
        <Show when={(props.message.files?.length ?? 0) > 0}>
          <GeneratedFiles files={props.message.files ?? []} />
        </Show>
        <Show when={artifactCount() > 0}>
          <div class="velion-chat-artifact-chips">
            <For each={props.message.artifacts ?? []}>
              {(artifact) => (
                <span>
                  <FileCode2 size={12} />
                  {artifact.title || artifact.kind}
                </span>
              )}
            </For>
          </div>
        </Show>
        <Show when={!waiting() && !errored()}>
          <div class="velion-chat-message-actions">
            <MessageAction label={props.copied ? 'Copied' : 'Copy'} onClick={props.onCopy}>
              {props.copied ? <Check size={14} /> : <Copy size={14} />}
            </MessageAction>
            <MessageAction
              active={reaction() === 'up'}
              label="Good response"
              onClick={() => {
                setReaction((current) => current === 'up' ? null : 'up')
                props.onFeedback('positive')
              }}
            >
              <ThumbsUp size={14} />
            </MessageAction>
            <MessageAction
              active={reaction() === 'down'}
              label="Bad response"
              onClick={() => {
                setReaction((current) => current === 'down' ? null : 'down')
                props.onFeedback('negative')
              }}
            >
              <ThumbsDown size={14} />
            </MessageAction>
            <MessageAction label="Regenerate" onClick={props.onRegenerate}>
              <RefreshCw size={14} />
            </MessageAction>
            <MessageMenu
              items={[
                { label: 'Fortsett i ny chat', icon: <MessageSquarePlus size={16} />, onClick: props.onBranch },
                { label: 'Les høyt', icon: <Volume2 size={16} />, onClick: () => readAloud(props.message.content) },
              ]}
            />
            <ReasoningPopover message={props.message} />
          </div>
        </Show>
      </div>
    </article>
  )
}

function UserMessage(props: {
  copied: boolean
  message: ChatTurn
  onBranch: () => void
  onCopy: () => void
  onEdit: (text: string) => void
}) {
  const [editing, setEditing] = createSignal(false)
  const [draft, setDraft] = createSignal('')

  createEffect(() => {
    if (!editing()) setDraft(props.message.content)
  })

  const startEditing = () => {
    setDraft(props.message.content)
    setEditing(true)
  }

  const submitEdit = () => {
    const next = draft().trim()
    if (!next) return
    setEditing(false)
    props.onEdit(next)
  }

  return (
    <article class="velion-chat-message velion-chat-message--user">
      <div class="velion-chat-user-meta">
        <span>Meg</span>
        <time>{formatRelative(props.message.createdAt)}</time>
      </div>
      <Show
        when={!editing()}
        fallback={(
          <div class="velion-chat-edit-box">
            <textarea
              autofocus
              value={draft()}
              rows={Math.min(10, Math.max(2, draft().split('\n').length))}
              onInput={(event) => setDraft(event.currentTarget.value)}
              onKeyDown={(event) => {
                if (event.key === 'Enter' && (event.metaKey || event.ctrlKey)) {
                  event.preventDefault()
                  submitEdit()
                }
                if (event.key === 'Escape') {
                  setEditing(false)
                  setDraft(props.message.content)
                }
              }}
            />
            <div>
              <button type="button" onClick={() => setEditing(false)}>Avbryt</button>
              <button type="button" disabled={!draft().trim()} onClick={submitEdit}>Send på nytt</button>
            </div>
          </div>
        )}
      >
        <>
          <div class="velion-chat-bubble">
            <span class="velion-chat-bubble__text">{props.message.content}</span>
            <ToolChips tools={props.message.tools} />
            <AttachmentChips attachments={props.message.attachments} tone="user" />
          </div>
          <div class="velion-chat-message-actions velion-chat-message-actions--user">
            <MessageAction label="Rediger" onClick={startEditing}>
              <Pencil size={14} />
            </MessageAction>
            <MessageAction label={props.copied ? 'Copied' : 'Copy'} onClick={props.onCopy}>
              {props.copied ? <Check size={14} /> : <Copy size={14} />}
            </MessageAction>
            <MessageMenu
              align="end"
              items={[{ label: 'Fortsett i ny chat', icon: <MessageSquarePlus size={16} />, onClick: props.onBranch }]}
            />
          </div>
        </>
      </Show>
    </article>
  )
}

function ChatMarkdown(props: { content: string }) {
  return (
    <div class="velion-chat-markdown">
      <For each={parseMarkdownBlocks(props.content)}>
        {(block) => <MarkdownBlockView block={block} />}
      </For>
    </div>
  )
}

function MarkdownBlockView(props: { block: MarkdownBlock }) {
  return (
    <Switch>
      <Match when={props.block.kind === 'heading'}>
        <DynamicHeading block={props.block as Extract<MarkdownBlock, { kind: 'heading' }>} />
      </Match>
      <Match when={props.block.kind === 'code'}>
        <pre><code>{(props.block as Extract<MarkdownBlock, { kind: 'code' }>).text}</code></pre>
      </Match>
      <Match when={props.block.kind === 'list'}>
        <MarkdownList block={props.block as Extract<MarkdownBlock, { kind: 'list' }>} />
      </Match>
      <Match when={props.block.kind === 'quote'}>
        <blockquote>{parseInline((props.block as Extract<MarkdownBlock, { kind: 'quote' }>).text)}</blockquote>
      </Match>
      <Match when={props.block.kind === 'hr'}>
        <hr />
      </Match>
      <Match when={props.block.kind === 'paragraph'}>
        <p>{parseInline((props.block as Extract<MarkdownBlock, { kind: 'paragraph' }>).text)}</p>
      </Match>
    </Switch>
  )
}

function DynamicHeading(props: { block: Extract<MarkdownBlock, { kind: 'heading' }> }) {
  return (
    <Switch fallback={<h3>{parseInline(props.block.text)}</h3>}>
      <Match when={props.block.level === 1}>
        <h1>{parseInline(props.block.text)}</h1>
      </Match>
      <Match when={props.block.level === 2}>
        <h2>{parseInline(props.block.text)}</h2>
      </Match>
    </Switch>
  )
}

function MarkdownList(props: { block: Extract<MarkdownBlock, { kind: 'list' }> }) {
  return (
    <Show
      when={props.block.ordered}
      fallback={<ul><For each={props.block.items}>{(item) => <li>{parseInline(item)}</li>}</For></ul>}
    >
      <ol><For each={props.block.items}>{(item) => <li>{parseInline(item)}</li>}</For></ol>
    </Show>
  )
}

function ReasoningTrace(props: { text: string; streaming: boolean }) {
  const [open, setOpen] = createSignal(false)
  const expanded = () => open() || props.streaming
  const trimmed = () => props.text.trim()
  return (
    <Show when={trimmed()}>
      <div class="velion-chat-reasoning">
        <button type="button" aria-expanded={expanded()} onClick={() => setOpen((value) => !value)}>
          <Brain size={14} />
          {props.streaming ? 'Tenker ...' : 'Tenkte'}
          <ChevronRight size={14} classList={{ 'velion-chat-rotate': expanded() }} />
        </button>
        <Show when={expanded()}>
          <p>{trimmed()}</p>
        </Show>
      </div>
    </Show>
  )
}

function ReasoningPopover(props: { message: ChatTurn }) {
  const [open, setOpen] = createSignal(false)
  const [tab, setTab] = createSignal('general')
  let ref!: HTMLDivElement
  const model = () => props.message.modelUsed ?? props.message.model
  const hasMetrics = () => Boolean(
    model()
    || props.message.inputTokens != null
    || props.message.outputTokens != null
    || props.message.latencyMs != null
    || props.message.confidence != null
    || props.message.costUsd != null
    || props.message.reasoning
    || props.message.grounding
    || (props.message.citations?.length ?? 0) > 0
    || (props.message.toolCalls?.length ?? 0) > 0,
  )
  const tabs = () => [
    { id: 'general', label: 'Oversikt' },
    ...(props.message.reasoning ? [{ id: 'insight', label: 'Innsikt' }] : []),
    ...((props.message.toolCalls?.length ?? 0) > 0 ? [{ id: 'tools', label: 'Verktøy' }] : []),
    ...(props.message.grounding || (props.message.citations?.length ?? 0) > 0 ? [{ id: 'sources', label: 'Kilder' }] : []),
  ]

  createEffect(() => {
    if (!open()) return
    const onPointer = (event: PointerEvent) => {
      if (ref && !ref.contains(event.target as Node)) setOpen(false)
    }
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setOpen(false)
    }
    document.addEventListener('pointerdown', onPointer)
    document.addEventListener('keydown', onKey)
    onCleanup(() => {
      document.removeEventListener('pointerdown', onPointer)
      document.removeEventListener('keydown', onKey)
    })
  })

  return (
    <Show when={hasMetrics()}>
      <div ref={ref} class="velion-chat-reasoning-popover">
        <button type="button" aria-expanded={open()} onClick={() => setOpen((value) => !value)}>
          <Sparkles size={12} />
          <Show when={model()}><span>{prettyModel(model() ?? '')}</span></Show>
          <Show when={props.message.outputTokens}><em>{props.message.outputTokens} tokens</em></Show>
        </button>
        <Show when={open()}>
          <div class="velion-chat-reasoning-popover__panel">
            <div class="velion-chat-reasoning-popover__head">
              <strong>Reasoning</strong>
              <button type="button" aria-label="Lukk" onClick={() => setOpen(false)}>
                <X size={14} />
              </button>
            </div>
            <Show when={tabs().length > 1}>
              <div class="velion-chat-reasoning-popover__tabs">
                <For each={tabs()}>
                  {(item) => (
                    <button type="button" classList={{ 'is-active': tab() === item.id }} onClick={() => setTab(item.id)}>
                      {item.label}
                    </button>
                  )}
                </For>
              </div>
            </Show>
            <Switch>
              <Match when={tab() === 'insight'}>
                <p class="velion-chat-reasoning-popover__copy">{props.message.reasoning}</p>
              </Match>
              <Match when={tab() === 'tools'}>
                <div class="velion-chat-reasoning-popover__stack">
                  <For each={props.message.toolCalls ?? []}>
                    {(call) => <span><Wrench size={13} /> {call.name}</span>}
                  </For>
                </div>
              </Match>
              <Match when={tab() === 'sources'}>
                <div class="velion-chat-reasoning-popover__stack">
                  <For each={props.message.grounding?.sources ?? []}>
                    {(source) => <span>{source.title}</span>}
                  </For>
                  <For each={props.message.citations ?? []}>
                    {(citation) => <a href={citation.url} target="_blank" rel="noopener noreferrer">{citation.title || citation.url}</a>}
                  </For>
                </div>
              </Match>
              <Match when={true}>
                <dl>
                  <Show when={model()}><MetricRow label="Modell" value={prettyModel(model() ?? '')} /></Show>
                  <Show when={props.message.inputTokens != null}><MetricRow label="Input" value={`${props.message.inputTokens} tokens`} /></Show>
                  <Show when={props.message.outputTokens != null}><MetricRow label="Output" value={`${props.message.outputTokens} tokens`} /></Show>
                  <Show when={props.message.latencyMs != null}><MetricRow label="Total tid" value={formatLatency(props.message.latencyMs ?? 0)} /></Show>
                  <Show when={props.message.confidence != null}><MetricRow label="Sikkerhet" value={`${Math.round((props.message.confidence ?? 0) * 100)}%`} /></Show>
                  <Show when={props.message.costUsd != null}><MetricRow label="Kostnad" value={`$${(props.message.costUsd ?? 0).toFixed(4)}`} /></Show>
                </dl>
              </Match>
            </Switch>
          </div>
        </Show>
      </div>
    </Show>
  )
}

function MetricRow(props: { label: string; value: string }) {
  return (
    <div>
      <dt>{props.label}</dt>
      <dd>{props.value}</dd>
    </div>
  )
}

function ToolCallList(props: { calls: ChatToolCall[] }) {
  return (
    <div class="velion-chat-tool-list">
      <For each={props.calls}>
        {(call) => <ToolCallCard call={call} />}
      </For>
    </div>
  )
}

function ApprovalRequests(props: {
  approvals: Approval[]
  onDecide: (approvalId: string, decision: ApprovalDecision) => void
}) {
  return (
    <div class="velion-chat-approvals" role="group" aria-label="Godkjenninger">
      <For each={props.approvals}>
        {(approval) => (
          <div class="velion-chat-approval">
            <div class="velion-chat-approval__head">
              <span class="velion-chat-approval__badge">Godkjenning</span>
              <span class="velion-chat-approval__kind">
                {approval.kind ?? 'Agenten venter på godkjenning før neste steg'}
              </span>
            </div>
            <Show when={approval.detail}>
              <p class="velion-chat-approval__detail">{approval.detail}</p>
            </Show>
            <div class="velion-chat-approval__actions">
              <button
                type="button"
                class="velion-chat-approval__approve"
                onClick={() => props.onDecide(approval.id, 'approve')}
              >
                Godkjenn
              </button>
              <button
                type="button"
                class="velion-chat-approval__reject"
                onClick={() => props.onDecide(approval.id, 'reject')}
              >
                Avvis
              </button>
            </div>
          </div>
        )}
      </For>
    </div>
  )
}

function ToolCallCard(props: { call: ChatToolCall }) {
  const [open, setOpen] = createSignal(false)
  const failed = () => Boolean(props.call.error) || props.call.status === 'error'
  const running = () => !props.call.status || props.call.status === 'running'
  const statusLabel = () => failed() ? 'feilet' : running() ? 'kjører ...' : 'fullført'
  const args = () => formatToolArgs(props.call.args)

  return (
    <div class="velion-chat-tool-call">
      <button type="button" aria-expanded={open()} onClick={() => setOpen((value) => !value)}>
        <Wrench size={14} />
        <span>{props.call.name}</span>
        <em classList={{ 'is-error': failed(), 'is-running': running() }}>{statusLabel()}</em>
        <ChevronRight size={14} classList={{ 'velion-chat-rotate': open() }} />
      </button>
      <Show when={open() && (args() || props.call.output || props.call.error)}>
        <div>
          <Show when={args()}><pre>{args()}</pre></Show>
          <Show when={props.call.output}><pre>{props.call.output}</pre></Show>
          <Show when={props.call.error}><p>{props.call.error}</p></Show>
        </div>
      </Show>
    </div>
  )
}

function ToolChips(props: { tools: ComposerToolId[] }) {
  return (
    <Show when={props.tools.length > 0}>
      <div class="velion-chat-tool-chips">
        <For each={props.tools}>
          {(tool) => <span>{TOOL_LABELS[tool]}</span>}
        </For>
      </div>
    </Show>
  )
}

function AttachmentChips(props: { attachments: ComposerAttachment[]; tone: 'assistant' | 'user' }) {
  return (
    <Show when={props.attachments.length > 0}>
      <div class="velion-chat-attachments">
        <For each={props.attachments}>
          {(attachment) => <AttachmentItem attachment={attachment} tone={props.tone} />}
        </For>
      </div>
    </Show>
  )
}

function AttachmentItem(props: { attachment: ComposerAttachment; tone: 'assistant' | 'user' }) {
  const [failed, setFailed] = createSignal(false)
  const isImage = () => Boolean(props.attachment.url) && props.attachment.type.startsWith('image/') && !failed()

  return (
    <Show
      when={isImage() && props.attachment.url}
      fallback={<span class={`velion-chat-attachment velion-chat-attachment--${props.tone}`}>{props.attachment.name}</span>}
    >
      {(url) => (
        <span class="velion-chat-attachment-image">
          <img src={url()} alt={props.attachment.name} onError={() => setFailed(true)} />
        </span>
      )}
    </Show>
  )
}

function GeneratedFiles(props: { files: GeneratedFile[] }) {
  return (
    <div class="velion-chat-generated-files">
      <For each={props.files}>
        {(file) => (
          <a href={file.url} target="_blank" rel="noopener noreferrer">
            <Paperclip size={12} />
            {file.name}
            <Show when={file.size > 0}><span>{formatBytes(file.size)}</span></Show>
          </a>
        )}
      </For>
    </div>
  )
}

function GroundingInlineSummary(props: { grounding: ChatKnowledgeGrounding }) {
  return (
    <div class="velion-chat-grounding-inline">
      <span><Sparkles size={12} /> {props.grounding.sourceCount} internal source{props.grounding.sourceCount === 1 ? '' : 's'}</span>
      <span>{props.grounding.factCount} fact{props.grounding.factCount === 1 ? '' : 's'}</span>
      <Show when={props.grounding.graph?.nodes.length}>
        {(count) => <span>{count()} graph node{count() === 1 ? '' : 's'}</span>}
      </Show>
      <Show when={props.grounding.lowConfidence}>
        <em>Low confidence</em>
      </Show>
    </div>
  )
}

function ErrorNotice(props: { message: string; onRetry: () => void }) {
  return (
    <div class="velion-chat-error-notice">
      <p><AlertCircle size={16} /> {props.message}</p>
      <button type="button" onClick={() => props.onRetry()}>
        <RefreshCw size={14} />
        Prøv igjen
      </button>
    </div>
  )
}

function MessageAction(props: { active?: boolean; children: JSX.Element; label: string; onClick: () => void }) {
  return (
    <button
      type="button"
      title={props.label}
      aria-label={props.label}
      aria-pressed={props.active}
      classList={{ 'velion-chat-action-button': true, 'velion-chat-action-button--active': Boolean(props.active) }}
      onClick={() => props.onClick()}
    >
      {props.children}
    </button>
  )
}

function MessageMenu(props: { align?: 'start' | 'end'; items: Array<{ label: string; icon: JSX.Element; onClick: () => void }> }) {
  const [open, setOpen] = createSignal(false)
  let ref!: HTMLDivElement

  createEffect(() => {
    if (!open()) return
    const onPointer = (event: PointerEvent) => {
      if (ref && !ref.contains(event.target as Node)) setOpen(false)
    }
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setOpen(false)
    }
    document.addEventListener('pointerdown', onPointer)
    document.addEventListener('keydown', onKey)
    onCleanup(() => {
      document.removeEventListener('pointerdown', onPointer)
      document.removeEventListener('keydown', onKey)
    })
  })

  return (
    <div ref={ref} class="velion-chat-menu">
      <button type="button" aria-label="Flere handlinger" aria-expanded={open()} onClick={() => setOpen((value) => !value)}>
        <MoreHorizontal size={14} />
      </button>
      <Show when={open()}>
        <div classList={{ 'velion-chat-menu__panel': true, 'velion-chat-menu__panel--end': props.align === 'end' }}>
          <For each={props.items}>
            {(item) => (
              <button
                type="button"
                onClick={() => {
                  item.onClick()
                  setOpen(false)
                }}
              >
                {item.icon}
                {item.label}
              </button>
            )}
          </For>
        </div>
      </Show>
    </div>
  )
}

function DateDivider(props: { value: string }) {
  return (
    <div class="velion-chat-divider">
      <span />
      <time>{formatDayLabel(props.value)}</time>
    </div>
  )
}

function ThinkingDots() {
  return (
    <span class="velion-chat-thinking">
      <span class="velion-thinking-dots" aria-hidden="true">
        <span />
        <span />
        <span />
      </span>
      Tenker
    </span>
  )
}

function TaskStep(props: { isLast: boolean; step: AgentTaskStep }) {
  const icon = () => getTaskStepIcon(props.step.status)
  return (
    <div class="velion-chat-step">
      <Show when={!props.isLast}>
        <span class="velion-chat-step__line" />
      </Show>
      <span class={`velion-chat-step__icon ${icon().className}`}>{icon().node}</span>
      <div>
        <p>
          <strong>{props.step.title}</strong>
          <time>{formatTime(props.step.createdAt)}</time>
        </p>
        <span>{props.step.detail}</span>
      </div>
    </div>
  )
}

// ── Empty state ───────────────────────────────────────────────────────────────

function EmptyChatState(props: { children: JSX.Element; onSelectPrompt: (prompt: string) => void }) {
  const [moreOpen, setMoreOpen] = createSignal(false)
  let moreRef!: HTMLDivElement

  createEffect(() => {
    if (!moreOpen()) return
    const onPointer = (e: PointerEvent) => { if (!moreRef?.contains(e.target as Node)) setMoreOpen(false) }
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') setMoreOpen(false) }
    document.addEventListener('pointerdown', onPointer)
    document.addEventListener('keydown', onKey)
    onCleanup(() => {
      document.removeEventListener('pointerdown', onPointer)
      document.removeEventListener('keydown', onKey)
    })
  })

  const select = (prompt: string) => {
    props.onSelectPrompt(prompt)
    setMoreOpen(false)
  }

  return (
    <div class="velion-chat-empty">
      <div class="velion-chat-empty__inner">
        <div class="velion-chat-empty__heading">
          <Sparkles size={28} strokeWidth={1.75} />
          <h1>Hva kan jeg hjelpe med?</h1>
        </div>
        <div class="velion-chat-empty__composer">{props.children}</div>
        <div class="velion-chat-empty__prompts">
          <For each={PRIMARY_PROMPTS}>
            {({ label, prompt, icon: Icon }) => (
              <button
                type="button"
                class="velion-quick-chip"
                aria-label={`Use quick prompt: ${label}`}
                onClick={() => select(prompt)}
              >
                <Icon size={16} strokeWidth={1.9} />
                <span>{label}</span>
              </button>
            )}
          </For>
          <div ref={moreRef} class="velion-quick-chip-more">
            <button
              type="button"
              class="velion-quick-chip"
              aria-expanded={moreOpen()}
              onClick={() => setMoreOpen((o) => !o)}
            >
              <MoreHorizontal size={16} strokeWidth={1.9} />
              <span>More</span>
            </button>
            <Show when={moreOpen()}>
              <div class="velion-popover velion-quick-chip-menu" role="menu">
                <For each={OVERFLOW_PROMPTS}>
                  {({ label, prompt, icon: Icon }) => (
                    <button type="button" role="menuitem" class="velion-menu-item" onClick={() => select(prompt)}>
                      <Icon size={16} strokeWidth={1.9} />
                      <span>{label}</span>
                    </button>
                  )}
                </For>
              </div>
            </Show>
          </div>
        </div>
      </div>
    </div>
  )
}

// ── Helpers ───────────────────────────────────────────────────────────────────

function messageToTurn(msg: ChatMessage): ChatTurn {
  return {
    id: msg.id,
    role: msg.role,
    content: msg.content,
    createdAt: msg.createdAt || new Date().toISOString(),
    streaming: false,
    model: msg.model,
    tools: [],
    attachments: [],
  }
}

async function toStreamAttachments(
  attachments: DashboardComposerSubmitPayload['attachments'],
): Promise<StreamAttachment[]> {
  const out: StreamAttachment[] = []
  for (const attachment of attachments) {
    if (!attachment.url || !attachment.type.startsWith('image/')) continue
    try {
      let dataUrl = attachment.url
      if (!dataUrl.startsWith('data:')) {
        const response = await fetch(dataUrl)
        dataUrl = await blobToDataUrl(await response.blob())
      }
      const parsed = parseDataUrl(dataUrl)
      if (parsed) out.push({ kind: 'image', data_base64: parsed.base64, mime_type: parsed.mime || attachment.type })
    } catch {
      // skip unreadable attachments
    }
  }
  return out
}

function createId(prefix: string) {
  if (typeof crypto !== 'undefined' && 'randomUUID' in crypto) {
    return `${prefix}-${crypto.randomUUID()}`
  }
  return `${prefix}-${Date.now()}-${Math.random().toString(36).slice(2, 10)}`
}

function buildTaskSteps(content: string, tools: ComposerToolId[], mode: 'submit' | 'regenerate'): AgentTaskStep[] {
  const now = new Date().toISOString()
  const intro = mode === 'regenerate' ? 'Regenerating latest response.' : `Preparing "${createPreview(content)}".`
  return [
    { id: 'connect', title: 'Connect stream', detail: intro, status: 'active', createdAt: now },
    ...tools.map((tool) => ({
      id: `tool-${tool}`,
      title: TOOL_LABELS[tool],
      detail: `${TOOL_LABELS[tool]} is enabled for this turn.`,
      status: 'waiting' as const,
      createdAt: now,
    })),
    { id: 'answer', title: 'Compose response', detail: 'Waiting for model output.', status: 'waiting', createdAt: now },
  ]
}

function normalizeArtifact(event: { id?: string; kind?: string; title?: string; content?: string; version?: number }): ChatArtifact | null {
  if (!event.content) return null
  const kind = event.kind ?? inferArtifactKind(event.content)
  return {
    id: event.id ?? createId('artifact'),
    kind,
    title: event.title ?? artifactTitle(kind),
    content: event.content,
    version: event.version ?? 0,
  }
}

function normalizeGeneratedFile(event: {
  id?: string
  name?: string
  mime?: string
  type?: string
  url?: string
  size?: number
}): GeneratedFile | null {
  if (!event.url || !event.name) return null
  return {
    id: event.id ?? event.url,
    name: event.name,
    mime: event.mime ?? event.type ?? 'application/octet-stream',
    size: event.size ?? 0,
    url: event.url,
  }
}

function normalizeCitation(event: { id?: string; title?: string; url?: string; snippet?: string }): Citation | null {
  if (!event.url) return null
  return {
    id: event.id ?? event.url,
    title: event.title ?? hostname(event.url),
    url: event.url,
    snippet: event.snippet ?? '',
  }
}

function normalizeStep(event: { id?: string; title?: string; detail?: string; status?: string }): AgentTaskStep | null {
  if (!event.id && !event.title) return null
  return {
    id: event.id ?? createId('step'),
    title: event.title ?? 'Agent step',
    detail: event.detail ?? event.status ?? 'Updated.',
    status: normalizeTaskStatus(event.status),
    createdAt: new Date().toISOString(),
  }
}

function normalizeToolCall(event: { id?: string; name?: string; args?: unknown }): ChatToolCall | null {
  if (!event.id && !event.name) return null
  return {
    id: event.id ?? event.name ?? createId('tool'),
    name: event.name ?? 'Tool call',
    args: event.args,
    status: 'running',
  }
}

function applyToolResult(calls: ChatToolCall[], event: { id?: string; output?: string; error?: string; status?: string }): ChatToolCall[] {
  const index = calls.findIndex((call) => call.id === event.id)
  if (index < 0) {
    return [
      ...calls,
      {
        id: event.id ?? createId('tool'),
        name: 'Tool result',
        output: event.output,
        error: event.error,
        status: event.status ?? (event.error ? 'error' : 'done'),
      },
    ]
  }
  return calls.map((call, itemIndex) => itemIndex === index
    ? { ...call, output: event.output, error: event.error, status: event.status ?? (event.error ? 'error' : 'done') }
    : call)
}

function upsertToolCall(calls: ChatToolCall[], call: ChatToolCall): ChatToolCall[] {
  const index = calls.findIndex((item) => item.id === call.id)
  if (index < 0) return [...calls, call]
  return calls.map((item, itemIndex) => itemIndex === index ? { ...item, ...call } : item)
}

function upsertArtifact(artifacts: ChatArtifact[], artifact: ChatArtifact): ChatArtifact[] {
  const index = artifacts.findIndex((item) => item.id === artifact.id)
  if (index < 0) return [...artifacts, artifact]
  return artifacts.map((item, itemIndex) => (
    itemIndex === index && artifact.version >= item.version ? artifact : item
  ))
}

function upsertGeneratedFile(files: GeneratedFile[], file: GeneratedFile): GeneratedFile[] {
  const index = files.findIndex((item) => item.id === file.id || item.url === file.url)
  if (index < 0) return [...files, file]
  return files.map((item, itemIndex) => itemIndex === index ? { ...item, ...file } : item)
}

function upsertCitation(citations: Citation[], citation: Citation): Citation[] {
  if (citations.some((item) => item.id === citation.id || item.url === citation.url)) return citations
  return [...citations, citation]
}

function collectArtifacts(turns: ChatTurn[]): ChatArtifact[] {
  const byId = new Map<string, ChatArtifact>()
  for (const turn of turns) {
    for (const artifact of turn.artifacts ?? []) {
      const existing = byId.get(artifact.id)
      if (!existing || artifact.version >= existing.version) byId.set(artifact.id, artifact)
    }
  }
  return [...byId.values()]
}

function collectEvidenceSources(turns: ChatTurn[]): EvidenceSource[] {
  const seen = new Set<string>()
  const result: EvidenceSource[] = []
  for (const turn of turns) {
    for (const source of turn.grounding?.sources ?? []) {
      const key = `knowledge:${source.documentId || source.id}`
      if (seen.has(key)) continue
      seen.add(key)
      result.push(source)
    }
    for (const citation of turn.citations ?? []) {
      const key = `web:${citation.url || citation.id}`
      if (seen.has(key)) continue
      seen.add(key)
      result.push({ ...citation, kind: 'web' })
    }
  }
  return result
}

function collectLatestGrounding(turns: ChatTurn[]): ChatKnowledgeGrounding | null {
  for (let index = turns.length - 1; index >= 0; index -= 1) {
    const grounding = turns[index]?.grounding
    if (grounding) return grounding
  }
  return null
}

function selectLatestImageArtifact(turns: ChatTurn[]): ChatArtifact | null {
  const images = collectArtifacts(turns).filter((artifact) => artifact.kind.toLowerCase() === 'image')
  return images.at(-1) ?? null
}

function normalizeGrounding(value: unknown): ChatKnowledgeGrounding | undefined {
  const root = objectValue(value)
  if (!root) return undefined
  const sources = Array.isArray(root.sources)
    ? root.sources.map(normalizeGroundingSource).filter((source): source is ChatGroundingSource => Boolean(source))
    : []
  const facts = Array.isArray(root.facts)
    ? root.facts.map(normalizeGroundingFact).filter((fact): fact is ChatGroundingFact => Boolean(fact))
    : []
  const graph = normalizeGroundingGraph(root.graph)
  return {
    mode: root.mode === 'hybrid' ? 'hybrid' : 'retrieve',
    query: stringValue(root.query) ?? '',
    traceId: stringValue(root.traceId) ?? stringValue(root.trace_id),
    lowConfidence: Boolean(root.lowConfidence ?? root.low_confidence),
    factCount: numberValue(root.factCount) ?? numberValue(root.fact_count) ?? facts.length,
    sourceCount: numberValue(root.sourceCount) ?? numberValue(root.source_count) ?? sources.length,
    facts,
    sources,
    graph,
  }
}

function normalizeGroundingSource(value: unknown): ChatGroundingSource | null {
  const source = objectValue(value)
  if (!source) return null
  return {
    id: stringValue(source.id) ?? createId('source'),
    kind: 'knowledge',
    title: stringValue(source.title) ?? 'Knowledge source',
    snippet: stringValue(source.snippet) ?? '',
    provider: stringValue(source.provider) ?? 'knowledge',
    sourceType: stringValue(source.sourceType) ?? stringValue(source.source_type) ?? 'document',
    documentId: stringValue(source.documentId) ?? stringValue(source.document_id) ?? '',
    href: stringValue(source.href) ?? '#',
    score: numberValue(source.score) ?? 0,
  }
}

function normalizeGroundingFact(value: unknown): ChatGroundingFact | null {
  const fact = objectValue(value)
  if (!fact) return null
  return {
    knowledgeId: stringValue(fact.knowledgeId) ?? stringValue(fact.knowledge_id) ?? '',
    documentId: stringValue(fact.documentId) ?? stringValue(fact.document_id) ?? '',
    text: stringValue(fact.text) ?? '',
    score: numberValue(fact.score) ?? 0,
    sourceTitle: stringValue(fact.sourceTitle) ?? stringValue(fact.source_title) ?? '',
    sourceType: stringValue(fact.sourceType) ?? stringValue(fact.source_type) ?? '',
    provider: stringValue(fact.provider) ?? '',
    chunkIndex: numberValue(fact.chunkIndex) ?? numberValue(fact.chunk_index) ?? 0,
  }
}

function normalizeGroundingGraph(value: unknown): ChatGroundingGraph | undefined {
  const graph = objectValue(value)
  if (!graph) return undefined
  const nodes = Array.isArray(graph.nodes)
    ? graph.nodes.map((item) => {
        const node = objectValue(item)
        if (!node) return null
        return {
          id: stringValue(node.id) ?? createId('node'),
          label: stringValue(node.label) ?? stringValue(node.name) ?? 'Node',
          kind: stringValue(node.kind) ?? 'entity',
        }
      }).filter((node): node is ChatGroundingGraphNode => Boolean(node))
    : []
  const summaries = Array.isArray(graph.communitySummaries)
    ? graph.communitySummaries.filter((item): item is string => typeof item === 'string')
    : Array.isArray(graph.community_summaries)
      ? graph.community_summaries.filter((item): item is string => typeof item === 'string')
      : []
  return {
    traceId: stringValue(graph.traceId) ?? stringValue(graph.trace_id),
    communitySummaries: summaries,
    edgeCount: numberValue(graph.edgeCount) ?? numberValue(graph.edge_count) ?? 0,
    nodes,
  }
}

function objectValue(value: unknown): Record<string, unknown> | undefined {
  return value && typeof value === 'object' ? value as Record<string, unknown> : undefined
}

function stringValue(value: unknown): string | undefined {
  return typeof value === 'string' ? value : undefined
}

function numberValue(value: unknown): number | undefined {
  return typeof value === 'number' ? value : undefined
}

function normalizeTaskStatus(status?: string): TaskStepStatus {
  if (status === 'done' || status === 'active' || status === 'waiting' || status === 'error' || status === 'stopped') return status
  if (status === 'running') return 'active'
  if (status === 'failed') return 'error'
  return 'active'
}

function inferArtifactKind(content: string) {
  if (content.startsWith('data:image') || /^https?:\/\/\S+\.(?:png|jpe?g|gif|webp|svg)/i.test(content)) return 'image'
  return 'text'
}

function artifactTitle(kind: string) {
  if (kind === 'image') return 'Generated image'
  if (PROSE_ARTIFACT_KINDS.has(kind)) return 'Generated document'
  return 'Artifact'
}

function imageArtifactSrc(content: string) {
  if (/^(data:|blob:|https?:\/\/)/i.test(content)) return content
  return `data:image/png;base64,${content}`
}

function hostname(url: string): string {
  try {
    return new URL(url).hostname.replace(/^www\./, '')
  } catch {
    return url
  }
}

function createChatTitle(turns: ChatTurn[]) {
  const first = turns.find((turn) => turn.role === 'user')
  if (!first) return 'Velion Chat'
  return createPreview(first.content, 48)
}

function createPreview(content: string, max = 34) {
  const clean = content.replace(/\s+/g, ' ').trim()
  return clean.length > max ? `${clean.slice(0, max - 1)}...` : clean
}

function shouldShowDateDivider(previous: ChatTurn | undefined, current: ChatTurn) {
  return !previous || dayKey(previous.createdAt) !== dayKey(current.createdAt)
}

function dayKey(value: string) {
  return new Date(value).toDateString()
}

function formatDayLabel(value: string) {
  const date = new Date(value)
  const today = new Date()
  const yesterday = new Date(today)
  yesterday.setDate(today.getDate() - 1)
  if (date.toDateString() === today.toDateString()) return 'Today'
  if (date.toDateString() === yesterday.toDateString()) return 'Yesterday'
  return new Intl.DateTimeFormat('en', { weekday: 'long', month: 'short', day: 'numeric' }).format(date)
}

function formatRelative(value: string) {
  const timestamp = new Date(value).getTime()
  if (Number.isNaN(timestamp)) return ''
  const minutes = Math.max(0, Math.round((Date.now() - timestamp) / 60000))
  if (minutes < 1) return 'Just now'
  if (minutes < 60) return `${minutes}m ago`
  const hours = Math.round(minutes / 60)
  if (hours < 24) return `${hours}h ago`
  return new Intl.DateTimeFormat('en', { month: 'short', day: 'numeric' }).format(new Date(value))
}

function formatTime(value: string) {
  return new Intl.DateTimeFormat('en', { hour: '2-digit', minute: '2-digit' }).format(new Date(value))
}

function formatLatency(ms: number) {
  if (ms < 1000) return `${Math.round(ms)} ms`
  return `${(ms / 1000).toFixed(1)} s`
}

function prettyModel(model: string) {
  const lower = model.toLowerCase()
  if (lower.includes('gpt-4o-mini')) return 'GPT-4o Mini'
  if (lower.includes('gpt-4.1')) return 'GPT-4.1'
  if (lower.includes('gpt-4o')) return 'GPT-4o'
  if (lower.includes('claude')) return 'Claude Sonnet'
  if (lower.includes('reason')) return 'Velion Reasoner'
  return model.length > 22 ? `${model.slice(0, 22)}...` : model
}

function formatBytes(bytes: number): string {
  if (!bytes || bytes < 0) return ''
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

function formatToolArgs(args: unknown): string {
  if (args == null) return ''
  if (typeof args === 'string') return args
  try {
    return JSON.stringify(args, null, 2)
  } catch {
    return String(args)
  }
}

function readAloud(text: string) {
  if (!('speechSynthesis' in window)) return
  const clean = text.trim()
  if (!clean) return
  window.speechSynthesis.cancel()
  const utterance = new SpeechSynthesisUtterance(clean)
  utterance.lang = 'nb-NO'
  window.speechSynthesis.speak(utterance)
}

function getTaskStepIcon(status: TaskStepStatus): { className: string; node: JSX.Element } {
  if (status === 'done') return { className: 'is-done', node: <CheckCircle2 size={14} /> }
  if (status === 'active') return { className: 'is-active', node: <Clock3 size={14} /> }
  if (status === 'waiting') return { className: 'is-waiting', node: <AlertCircle size={14} /> }
  if (status === 'error') return { className: 'is-error', node: <AlertCircle size={14} /> }
  return { className: 'is-stopped', node: <Square size={11} /> }
}

function parseMarkdownBlocks(markdown: string): MarkdownBlock[] {
  const lines = markdown.replace(/\r\n/g, '\n').split('\n')
  const blocks: MarkdownBlock[] = []
  let index = 0

  while (index < lines.length) {
    const line = lines[index] ?? ''
    if (!line.trim()) {
      index += 1
      continue
    }

    const fence = /^```(.*)$/.exec(line.trim())
    if (fence) {
      const code: string[] = []
      const lang = fence[1]?.trim() ?? ''
      index += 1
      while (index < lines.length && !/^```/.test(lines[index]?.trim() ?? '')) {
        code.push(lines[index] ?? '')
        index += 1
      }
      if (index < lines.length) index += 1
      blocks.push({ kind: 'code', lang, text: code.join('\n') })
      continue
    }

    if (/^\s*(-{3,}|\*{3,}|_{3,})\s*$/.test(line)) {
      blocks.push({ kind: 'hr' })
      index += 1
      continue
    }

    const heading = /^(#{1,6})\s+(.*)$/.exec(line)
    if (heading) {
      const marks = heading[1] ?? ''
      const text = heading[2] ?? ''
      blocks.push({
        kind: 'heading',
        level: marks.length <= 1 ? 1 : marks.length === 2 ? 2 : 3,
        text,
      })
      index += 1
      continue
    }

    if (/^\s*>\s?/.test(line)) {
      const quoted: string[] = []
      while (index < lines.length && /^\s*>\s?/.test(lines[index] ?? '')) {
        quoted.push((lines[index] ?? '').replace(/^\s*>\s?/, ''))
        index += 1
      }
      blocks.push({ kind: 'quote', text: quoted.join('\n') })
      continue
    }

    const list = parseList(lines, index)
    if (list) {
      blocks.push(list.block)
      index = list.next
      continue
    }

    const paragraph: string[] = []
    while (index < lines.length && lines[index]?.trim() && !isMarkdownBlockStart(lines[index] ?? '')) {
      paragraph.push(lines[index] ?? '')
      index += 1
    }
    blocks.push({ kind: 'paragraph', text: paragraph.join('\n').trim() })
  }

  return blocks
}

function parseList(lines: string[], start: number): { block: Extract<MarkdownBlock, { kind: 'list' }>; next: number } | null {
  const first = /^(\s*)([-*+]|\d+[.)])\s+(.*)$/.exec(lines[start] ?? '')
  if (!first) return null
  const ordered = /\d+[.)]/.test(first[2] ?? '')
  const items: string[] = []
  let index = start
  while (index < lines.length) {
    const match = /^(\s*)([-*+]|\d+[.)])\s+(.*)$/.exec(lines[index] ?? '')
    if (!match || /\d+[.)]/.test(match[2] ?? '') !== ordered) break
    items.push(match[3] ?? '')
    index += 1
  }
  return { block: { kind: 'list', ordered, items }, next: index }
}

function isMarkdownBlockStart(line: string) {
  return /^\s*```/.test(line)
    || /^\s*(-{3,}|\*{3,}|_{3,})\s*$/.test(line)
    || /^#{1,6}\s+/.test(line)
    || /^\s*>\s?/.test(line)
    || /^(\s*)([-*+]|\d+[.)])\s+/.test(line)
}

function parseInline(text: string): Array<string | JSX.Element> {
  const nodes: Array<string | JSX.Element> = []
  const pattern = /(`[^`]+`|\*\*[^*]+\*\*|\*[^*]+\*|\[[^\]]+\]\([^)]+\))/g
  let cursor = 0
  for (const match of text.matchAll(pattern)) {
    if (match.index == null) continue
    if (match.index > cursor) nodes.push(text.slice(cursor, match.index))
    const token = match[0]
    if (token.startsWith('`')) {
      nodes.push(<code>{token.slice(1, -1)}</code>)
    } else if (token.startsWith('**')) {
      nodes.push(<strong>{token.slice(2, -2)}</strong>)
    } else if (token.startsWith('*')) {
      nodes.push(<em>{token.slice(1, -1)}</em>)
    } else {
      const link = /^\[([^\]]+)\]\(([^)]+)\)$/.exec(token)
      nodes.push(link ? <a href={link[2]} target="_blank" rel="noopener noreferrer">{link[1]}</a> : token)
    }
    cursor = match.index + token.length
  }
  if (cursor < text.length) nodes.push(text.slice(cursor))
  return nodes
}
