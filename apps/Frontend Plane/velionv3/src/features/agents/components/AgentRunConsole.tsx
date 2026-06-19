import {
  createEffect,
  createMemo,
  createResource,
  createSignal,
  For,
  onCleanup,
  Show,
  type JSX,
} from 'solid-js'
import { createStore } from 'solid-js/store'
import {
  ArrowLeft,
  Bot,
  Brain,
  CheckCircle2,
  CheckSquare,
  CircleDot,
  Clock,
  Coins,
  Gauge,
  Globe2,
  History,
  Link2,
  ListChecks,
  Loader2,
  PauseCircle,
  Play,
  RotateCcw,
  ShieldCheck,
  Sparkles,
  Square,
  Timer,
  Users,
  Wrench,
  type LucideProps,
} from 'lucide-solid'
import { A } from '@solidjs/router'
import { Button } from '@/shared/ui/Button'
import { cn } from '@/shared/lib/cn'
import { agentBlueprints } from '@/features/agents/lib/velion-agent-blueprints'
import type { AgentBlueprint } from '@/features/agents/lib/velion-agent-page-types'
import { controlFocusClass } from '@/features/agents/lib/velion-agent-page-styles'
import {
  streamChat,
  VELION_BALANCE_MODE_ID,
  VELION_MODES,
  type ChatStreamHandlers,
  type VelionMode,
} from '@/shared/api/chat-client'
import {
  streamRunEvents,
  type RunEventHandlers,
} from '@/shared/api/run-console-client'
import {
  cancelRun,
  decideApproval,
  listApprovals,
  resumeRun,
  type Approval,
  type ApprovalDecision,
} from '@/shared/api/orchestration-client'
import {
  getRun,
  listRuns,
  type RunDetail,
} from '@/shared/api/runs-client'

// ── Types ─────────────────────────────────────────────────────────────────────

type RunStatus = 'idle' | 'running' | 'paused' | 'done' | 'failed' | 'cancelled'

type TimelineKind =
  | 'step'
  | 'tool'
  | 'reasoning'
  | 'plan'
  | 'todo'
  | 'browser'
  | 'subagent'
  | 'pause'
  | 'resume'

type IconComponent = (props: LucideProps) => JSX.Element

/** A single row in the live run timeline. */
type TimelineEntry = {
  /** Stable key. Steps/tools dedup by their backend id; raw events get a unique key. */
  id: string
  kind: TimelineKind
  title: string
  detail: string
  status?: string
  at: string
}

type Citation = {
  id: string
  title: string
  url: string
  snippet: string
}

type RunUsage = {
  inputTokens?: number
  outputTokens?: number
  costUsd?: number
  latencyMs?: number
  confidence?: number
}

type ConsoleState = {
  status: RunStatus
  runId: string | null
  threadId: string | null
  modelUsed: string | null
  answer: string
  reasoning: string
  timeline: TimelineEntry[]
  approvals: Approval[]
  /** Approval ids with an in-flight decide/resume/cancel call. */
  decidingIds: string[]
  citations: Citation[]
  usage: RunUsage | null
  error: string | null
}

// ── Page ──────────────────────────────────────────────────────────────────────

export default function AgentRunConsole() {
  const [goal, setGoal] = createSignal('')
  const [blueprintId, setBlueprintId] = createSignal<AgentBlueprint['id']>(agentBlueprints[0]!.id)
  const [modeId, setModeId] = createSignal<string>(VELION_BALANCE_MODE_ID)
  const [browseWeb, setBrowseWeb] = createSignal(false)

  const [state, setState] = createStore<ConsoleState>({
    status: 'idle',
    runId: null,
    threadId: null,
    modelUsed: null,
    answer: '',
    reasoning: '',
    timeline: [],
    approvals: [],
    decidingIds: [],
    citations: [],
    usage: null,
    error: null,
  })

  let controller: AbortController | undefined
  // Guard so the durable run-events stream is started exactly once per run.
  let runEventsStarted = false

  const blueprint = createMemo(
    () => agentBlueprints.find((item) => item.id === blueprintId()) ?? agentBlueprints[0]!,
  )
  const mode = createMemo(
    () => VELION_MODES.find((item) => item.id === modeId()) ?? VELION_MODES[1]!,
  )

  const isActive = () => state.status === 'running' || state.status === 'paused'
  const canRun = () => goal().trim().length > 0 && !isActive()
  const pendingApprovals = createMemo(() =>
    state.approvals.filter((approval) => (approval.status ?? 'PENDING').toUpperCase() === 'PENDING'),
  )
  const hasTrust = createMemo(() => state.citations.length > 0 || state.usage !== null)

  // ── Runs history ───────────────────────────────────────────────────────────
  // The history rail lists past runs for the current thread (newest-first). It
  // is additive: the live run continues to drive `state`. `historyTick` bumps
  // after each run settles so a freshly-finished run appears without a reload.
  const [historyTick, setHistoryTick] = createSignal(0)
  const [runs] = createResource(
    () => {
      const threadId = state.threadId
      if (!threadId) return null
      return { threadId, tick: historyTick() }
    },
    async (key) => {
      const page = await listRuns({ threadId: key.threadId, limit: 50 })
      return page.runs
    },
  )

  // The run whose telemetry the side panel shows. Defaults to the live run; a
  // history-rail click overrides it until the user starts a new run.
  const [selectedRunId, setSelectedRunId] = createSignal<string | null>(null)
  const activeRunId = createMemo(() => selectedRunId() ?? state.runId)
  const [runDetail] = createResource(activeRunId, (runId) => getRun(runId))

  // Telemetry merges the durable RunDetail (steps, tokens, status) with the
  // live usage event the chat stream emits (cost, latency, confidence) — the
  // latter only exists for the in-flight run, so it is preferred when present.
  const telemetry = createMemo(() => {
    const detail = runDetail()
    const live = activeRunId() === state.runId ? state.usage : null
    if (!detail && !live) return null
    return { detail: detail ?? null, live }
  })

  let replayController: AbortController | undefined
  onCleanup(() => {
    controller?.abort()
    replayController?.abort()
  })

  // ── Replay a past run ────────────────────────────────────────────────────
  // Selecting a history row loads that run into the live surface read-only:
  // reset the timeline, hydrate status/goal from the detail, then replay the
  // durable event stream (which closes immediately for a finished run, leaving
  // the reconstructed timeline). The live controller is untouched so an
  // in-flight run keeps streaming underneath.
  const replayRun = (run: RunDetail) => {
    if (isActive()) return
    if (run.runId === state.runId && selectedRunId() === null) return
    replayController?.abort()
    replayController = new AbortController()
    setSelectedRunId(run.runId)

    setState({
      status: statusToRunStatus(run.status),
      runId: run.runId,
      threadId: run.threadId ?? state.threadId,
      modelUsed: run.mode ?? null,
      answer: run.finalOutput ?? '',
      reasoning: '',
      timeline: [],
      approvals: [],
      decidingIds: [],
      citations: [],
      usage: null,
      error: run.error && run.error.trim().length > 0 ? run.error : null,
    })

    void streamRunEvents(run.runId, buildRunHandlers(), replayController.signal)
    void refreshApprovals(run.runId)
  }

  // ── Timeline helpers ─────────────────────────────────────────────────────
  // Steps and tool calls dedup on their backend id (both streams may emit the
  // same `step_update`), updating the existing row in place. Everything else is
  // appended in arrival order with a generated key.

  const upsertById = (entry: TimelineEntry) => {
    setState('timeline', (entries) => {
      const index = entries.findIndex((item) => item.id === entry.id)
      if (index < 0) return [...entries, entry]
      const next = entries.slice()
      next[index] = { ...next[index]!, ...entry }
      return next
    })
  }

  const append = (entry: TimelineEntry) => {
    setState('timeline', (entries) => [...entries, entry])
  }

  // ── Approvals ────────────────────────────────────────────────────────────

  const refreshApprovals = async (runId: string) => {
    try {
      const approvals = await listApprovals(runId, controller?.signal)
      setState('approvals', approvals)
    } catch {
      // A run with no orchestration worker has no approvals endpoint state yet —
      // keep whatever we have; a sparse run must never surface as an error.
    }
  }

  const handleApprovalDecision = async (approvalId: string, decision: ApprovalDecision) => {
    const runId = state.runId
    // Mark the decision in-flight (spinner + disabled buttons) before any await,
    // so the active control reflects the pending network call immediately.
    setState('decidingIds', (prev) => (prev.includes(approvalId) ? prev : [...prev, approvalId]))
    // Optimistically drop the decided approval so the card resolves instantly.
    setState('approvals', (prev) => prev.filter((approval) => approval.id !== approvalId))
    try {
      await decideApproval(approvalId, decision)
      if (runId) {
        if (decision === 'approve') {
          await resumeRun(runId)
        } else {
          // A rejection records the denial; the agent may route around the tool,
          // but the user intent here is to stop the gated path — cancel the run.
          await cancelRun(runId).catch(() => undefined)
          setState('status', 'cancelled')
        }
        await refreshApprovals(runId)
      }
    } catch {
      // Surface the failure and re-sync from the source of truth.
      setState('error', 'Could not record your decision — try again.')
      if (runId) void refreshApprovals(runId)
    } finally {
      setState('decidingIds', (prev) => prev.filter((id) => id !== approvalId))
    }
  }

  // ── Run lifecycle ──────────────────────────────────────────────────────────

  // Shared durable-event handler set, reused by the live run (`startRunEvents`)
  // and by history replay (`replayRun`). All callbacks read/write the same
  // `state`, so the surface renders identically whether the events arrive live
  // or are replayed from a finished run's durable stream.
  const buildRunHandlers = (): RunEventHandlers => {
    return {
      onStep: (event) => {
        if (!event.id && !event.title) return
        upsertById({
          id: event.id ?? `step-${state.timeline.length}`,
          kind: 'step',
          title: event.title ?? 'Step',
          detail: event.detail ?? '',
          status: event.status,
          at: new Date().toISOString(),
        })
      },
      onPlan: (event) => {
        append({
          id: `plan-${event.planId ?? state.timeline.length}-${event.to ?? ''}`,
          kind: 'plan',
          title: 'Plan',
          detail: `${event.from ? `${event.from} → ` : ''}${event.to ?? 'updated'}`,
          status: event.to,
          at: event.at ?? new Date().toISOString(),
        })
      },
      onTodo: (event) => {
        append({
          id: `todo-${event.todoId ?? state.timeline.length}-${event.to ?? ''}`,
          kind: 'todo',
          title: 'Todo',
          detail: `${event.from ? `${event.from} → ` : ''}${event.to ?? 'updated'}`,
          status: event.to,
          at: event.at ?? new Date().toISOString(),
        })
      },
      onApproval: (event) => {
        if (state.runId) void refreshApprovals(state.runId)
        append({
          id: `approval-${event.approvalId ?? state.timeline.length}-${event.to ?? ''}`,
          kind: 'pause',
          title: 'Approval',
          detail: `${event.approvalKind ?? 'Action'} · ${event.to ?? 'requested'}`,
          status: event.to,
          at: event.at ?? new Date().toISOString(),
        })
      },
      onRunPaused: (event) => {
        setState('status', 'paused')
        if (state.runId) void refreshApprovals(state.runId)
        append({
          id: `paused-${event.approvalId ?? state.timeline.length}`,
          kind: 'pause',
          title: 'Paused for approval',
          detail: 'The agent is waiting for your decision before the next step.',
          status: 'paused',
          at: event.at ?? new Date().toISOString(),
        })
      },
      onRunResumed: (event) => {
        if (state.status === 'paused') setState('status', 'running')
        append({
          id: `resumed-${event.approvalId ?? state.timeline.length}`,
          kind: 'resume',
          title: 'Resumed',
          detail: 'Decision recorded — the run continues.',
          status: 'resumed',
          at: event.at ?? new Date().toISOString(),
        })
      },
      onBrowserAction: (event) => {
        append({
          id: `browse-act-${event.actionId ?? state.timeline.length}`,
          kind: 'browser',
          title: `Browser · ${event.actionType ?? 'navigate'}`,
          detail: event.url ?? '',
          status: 'dispatched',
          at: event.at ?? new Date().toISOString(),
        })
      },
      onBrowserObservation: (event) => {
        append({
          id: `browse-obs-${event.actionId ?? state.timeline.length}`,
          kind: 'browser',
          title: event.pageTitle ? `Observed · ${event.pageTitle}` : 'Browser observation',
          detail: event.pageUrl ?? '',
          status: event.status ?? 'received',
          at: event.at ?? new Date().toISOString(),
        })
      },
      onSubagentAttached: (event) => {
        append({
          id: `subagent-on-${event.childRunId ?? state.timeline.length}`,
          kind: 'subagent',
          title: `Sub-agent attached${event.role ? ` · ${event.role}` : ''}`,
          detail: event.childRunId ? `run ${shortId(event.childRunId)}` : '',
          status: 'attached',
          at: event.at ?? new Date().toISOString(),
        })
      },
      onSubagentStopped: (event) => {
        append({
          id: `subagent-off-${event.childRunId ?? state.timeline.length}`,
          kind: 'subagent',
          title: 'Sub-agent stopped',
          detail: event.childRunId ? `run ${shortId(event.childRunId)}` : '',
          status: event.status ?? 'stopped',
          at: event.at ?? new Date().toISOString(),
        })
      },
      // A run with no durable worker closes its event stream immediately. That is
      // expected in the current dev stack — swallow the error silently so the
      // chat answer still flows and the timeline simply stays sparse.
      onError: () => undefined,
      onDone: () => undefined,
    }
  }

  const startRunEvents = (runId: string) => {
    if (runEventsStarted) return
    runEventsStarted = true
    void streamRunEvents(runId, buildRunHandlers(), controller?.signal)
  }

  const runTask = () => {
    const content = goal().trim()
    if (!content || isActive()) return

    // A new run aborts the previous controller first — no leaked streams. Any
    // history replay is torn down and the telemetry panel re-pins to the live
    // run.
    controller?.abort()
    replayController?.abort()
    controller = new AbortController()
    runEventsStarted = false
    setSelectedRunId(null)

    setState({
      status: 'running',
      runId: null,
      threadId: null,
      modelUsed: null,
      answer: '',
      reasoning: '',
      timeline: [],
      approvals: [],
      decidingIds: [],
      citations: [],
      usage: null,
      error: null,
    })

    const chatHandlers: ChatStreamHandlers = {
      onConnected: ({ runId, threadId, model }) => {
        if (threadId) setState('threadId', threadId)
        if (model) setState('modelUsed', model)
        if (runId) {
          setState('runId', runId)
          // Capture run id, THEN start the durable raw-event stream exactly once.
          startRunEvents(runId)
        }
      },
      onMessage: ({ content: delta }) => {
        setState('answer', (prev) => prev + delta)
      },
      onReasoning: ({ delta }) => {
        if (delta) setState('reasoning', (prev) => prev + delta)
      },
      onStep: (event) => {
        if (!event.id && !event.title) return
        upsertById({
          id: event.id ?? `chat-step-${state.timeline.length}`,
          kind: 'step',
          title: event.title ?? 'Step',
          detail: event.detail ?? '',
          status: event.status,
          at: new Date().toISOString(),
        })
        // The chat stream surfaces the orchestration `paused` status before the
        // run-events stream's `run_paused_for_approval` arrives — react to either.
        const rawStatus = (event.status ?? '').toLowerCase()
        if (rawStatus === 'paused') {
          setState('status', 'paused')
          const runId = state.runId ?? event.id
          if (runId) {
            if (!state.runId) setState('runId', runId)
            void refreshApprovals(runId)
          }
        }
      },
      onToolCall: (event) => {
        if (!event.id) return
        upsertById({
          id: `tool-${event.id}`,
          kind: 'tool',
          title: event.name ?? 'Tool call',
          detail: 'Running…',
          status: 'running',
          at: new Date().toISOString(),
        })
      },
      onToolResult: (event) => {
        if (!event.id) return
        upsertById({
          id: `tool-${event.id}`,
          kind: 'tool',
          title: 'Tool result',
          detail: event.error ?? event.output ?? 'Completed.',
          status: event.error ? 'error' : (event.status ?? 'done'),
          at: new Date().toISOString(),
        })
      },
      onCitation: (event) => {
        const id = event.id ?? event.url ?? `cite-${state.citations.length}`
        const url = event.url ?? ''
        if (!url && !event.title) return
        setState('citations', (prev) => {
          if (prev.some((item) => item.id === id)) return prev
          return [
            ...prev,
            {
              id,
              title: event.title ?? url,
              url,
              snippet: event.snippet ?? '',
            },
          ]
        })
      },
      onUsage: (usage) => {
        setState('usage', {
          inputTokens: usage.inputTokens,
          outputTokens: usage.outputTokens,
          costUsd: usage.costUsd,
          latencyMs: usage.latencyMs,
          confidence: usage.confidence,
        })
      },
      onDone: () => {
        // Don't clobber a paused/cancelled run — the chat stream can close while
        // the durable run is still gated on a human decision.
        setState('status', (prev) => (prev === 'running' ? 'done' : prev))
      },
      onError: ({ message }) => {
        // The agentic stream can close early in a sparse dev stack; only flip to
        // failed if we never produced an answer, so a graceful fallback (direct
        // inference after ~25s) still reads as a completed run.
        if (controller?.signal.aborted) return
        if (state.answer.trim().length > 0) {
          setState('status', (prev) => (prev === 'running' ? 'done' : prev))
          return
        }
        setState('status', 'failed')
        setState('error', message)
      },
    }

    void streamChat(
      { content, model: modeId(), planMode: true, browseWeb: browseWeb() },
      chatHandlers,
      controller.signal,
    ).then(() => {
      // streamChat resolves when the SSE stream ends; settle any non-terminal
      // state so the UI never hangs in "running" after the stream closed.
      setState('status', (prev) => (prev === 'running' ? 'done' : prev))
    })
  }

  const cancel = () => {
    controller?.abort()
    if (state.runId) void cancelRun(state.runId).catch(() => undefined)
    setState('status', 'cancelled')
  }

  const useExample = (text: string) => {
    if (isActive()) return
    setGoal(text)
  }

  const working = () => state.status === 'running'
  const awaiting = () => pendingApprovals().length > 0
  const deciding = (id: string) => state.decidingIds.includes(id)

  // The approval deck is the highest-priority surface: scroll it into view when a
  // new pending approval appears, unless the user prefers reduced motion.
  let deckRef: HTMLDivElement | undefined
  createEffect(() => {
    if (pendingApprovals().length === 0) return
    const reduce = window.matchMedia?.('(prefers-reduced-motion: reduce)').matches ?? false
    deckRef?.scrollIntoView({ behavior: reduce ? 'auto' : 'smooth', block: 'nearest' })
  })

  // Refresh the history rail when a live run settles so the just-finished run
  // (and its final status) shows up without a reload. Reading `state.status`
  // and `state.runId` makes this re-run on each transition; the bump only
  // fires on a terminal status for a real run.
  createEffect(() => {
    const status = state.status
    const runId = state.runId
    if (runId && (status === 'done' || status === 'failed' || status === 'cancelled')) {
      setHistoryTick((tick) => tick + 1)
    }
  })

  return (
    <div class="velion-run-console">
      <div class="velion-run-console__container">
        <RunConsoleHeader
          blueprint={blueprint()}
          mode={mode()}
          runId={state.runId}
          status={state.status}
        />

        <div class="velion-run-console__grid">
          <section class="velion-run-console__launcher" aria-label="Task launcher">
            <Launcher
              blueprintId={blueprintId()}
              browseWeb={browseWeb()}
              canRun={canRun()}
              goal={goal()}
              isActive={isActive()}
              modeId={modeId()}
              onBlueprint={setBlueprintId}
              onBrowseWeb={setBrowseWeb}
              onCancel={cancel}
              onGoal={setGoal}
              onMode={setModeId}
              onRun={runTask}
            />

            <HistoryRail
              activeRunId={activeRunId()}
              loading={runs.loading}
              runs={runs() ?? []}
              onSelect={replayRun}
            />
          </section>

          <section
            class="velion-run-console__live"
            classList={{
              'velion-run-console__live--active': isActive(),
              'velion-run-console__live--awaiting': awaiting(),
            }}
            aria-label="Live run"
          >
            <Show when={pendingApprovals().length > 0}>
              <div ref={deckRef}>
                <ApprovalDeck
                  approvals={pendingApprovals()}
                  deciding={deciding}
                  onDecide={handleApprovalDecision}
                />
              </div>
            </Show>

            <Show when={telemetry()}>
              {(view) => (
                <TelemetryPanel
                  detail={view().detail}
                  live={view().live}
                  mode={mode()}
                  status={state.status}
                  onResume={() => {
                    const runId = activeRunId()
                    if (runId) void resumeRun(runId).catch(() => undefined)
                  }}
                />
              )}
            </Show>

            <AnswerPanel
              answer={state.answer}
              reasoning={state.reasoning}
              status={state.status}
              streaming={working()}
              onUseExample={useExample}
            />

            <Show when={hasTrust()}>
              <TrustPanel citations={state.citations} usage={state.usage} />
            </Show>

            <TimelinePanel entries={state.timeline} status={state.status} />

            <Show when={state.error}>
              <p class="velion-run-console__error" role="alert">{state.error}</p>
            </Show>
          </section>
        </div>
      </div>
    </div>
  )
}

// ── Header ──────────────────────────────────────────────────────────────────

const STATUS_LABEL: Record<RunStatus, string> = {
  idle: 'Ready',
  running: 'Running',
  paused: 'Paused for approval',
  done: 'Completed',
  failed: 'Failed',
  cancelled: 'Cancelled',
}

function RunConsoleHeader(props: {
  blueprint: AgentBlueprint
  mode: VelionMode
  runId: string | null
  status: RunStatus
}) {
  const Icon = createMemo(() => props.blueprint.Icon)
  return (
    <header class="velion-run-console__topbar">
      <div class="velion-run-console__title-group">
        <A href="/agents" class={cn('velion-run-console__back', controlFocusClass)} aria-label="Back to agents">
          <ArrowLeft size={16} />
        </A>
        <div>
          <p class="velion-run-console__eyebrow">
            <Sparkles size={13} strokeWidth={2.1} /> Task console
          </p>
          <h1 class="velion-run-console__title">Agent Run Console</h1>
        </div>
      </div>
      <div class="velion-run-console__status-row">
        <span class={cn('velion-run-console__status', `velion-run-console__status--${props.status}`)}>
          <StatusDot status={props.status} />
          {STATUS_LABEL[props.status]}
        </span>
        <Show when={props.runId}>
          {(runId) => <span class="velion-run-console__run-id">run {shortId(runId())}</span>}
        </Show>
        <span class="velion-run-console__meta-chip">
          {(() => {
            const Glyph = Icon()
            return <Glyph size={13} strokeWidth={2.1} />
          })()}
          {props.blueprint.shortTitle}
        </span>
        <span class="velion-run-console__meta-chip">{props.mode.label}</span>
      </div>
    </header>
  )
}

function StatusDot(props: { status: RunStatus }) {
  return (
    <Show
      when={props.status === 'running' || props.status === 'paused'}
      fallback={<CircleDot size={11} strokeWidth={2.4} />}
    >
      <span class="velion-run-console__pulse" aria-hidden="true" />
    </Show>
  )
}

// ── Launcher ────────────────────────────────────────────────────────────────

/** Roving-tabindex arrow-key handler shared by both radiogroups. */
function radioGroupKeyDown(event: KeyboardEvent, count: number, index: number, select: (next: number) => void) {
  const forward = event.key === 'ArrowDown' || event.key === 'ArrowRight'
  const backward = event.key === 'ArrowUp' || event.key === 'ArrowLeft'
  if (!forward && !backward) return
  event.preventDefault()
  const next = forward ? (index + 1) % count : (index - 1 + count) % count
  select(next)
  const group = (event.currentTarget as HTMLElement).parentElement
  const target = group?.querySelectorAll<HTMLElement>('[role="radio"]')[next]
  target?.focus()
}

function Launcher(props: {
  blueprintId: AgentBlueprint['id']
  browseWeb: boolean
  canRun: boolean
  goal: string
  isActive: boolean
  modeId: string
  onBlueprint: (id: AgentBlueprint['id']) => void
  onBrowseWeb: (value: boolean) => void
  onCancel: () => void
  onGoal: (value: string) => void
  onMode: (id: string) => void
  onRun: () => void
}) {
  const blueprint = () => agentBlueprints.find((item) => item.id === props.blueprintId) ?? agentBlueprints[0]!
  const mode = () => VELION_MODES.find((item) => item.id === props.modeId) ?? VELION_MODES[1]!
  const advancedLabel = () =>
    `${blueprint().shortTitle} · ${mode().label} · Web ${props.browseWeb ? 'on' : 'off'}`

  return (
    <div class="velion-run-launcher">
      <div class="velion-run-launcher__field">
        <label class="velion-run-launcher__label" for="run-goal">What should the agent do?</label>
        <textarea
          id="run-goal"
          class={cn('velion-run-launcher__textarea', controlFocusClass)}
          placeholder="e.g. Research our top 3 competitors' pricing and draft a comparison summary."
          rows={4}
          value={props.goal}
          disabled={props.isActive}
          onInput={(event) => props.onGoal(event.currentTarget.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter' && (event.metaKey || event.ctrlKey) && props.canRun) {
              event.preventDefault()
              props.onRun()
            }
          }}
        />
      </div>

      <details class="velion-run-launcher__advanced" open>
        <summary>
          <span class="velion-run-launcher__advanced-title">Configuration</span>
          <span class="velion-run-launcher__advanced-value">{advancedLabel()}</span>
        </summary>

        <div class="velion-run-launcher__field">
          <p class="velion-run-launcher__label">Agent blueprint</p>
          <div class="velion-run-launcher__blueprints" role="radiogroup" aria-label="Agent blueprint">
            <For each={agentBlueprints}>
              {(item, index) => {
                const Icon = item.Icon
                const selected = () => props.blueprintId === item.id
                return (
                  <button
                    type="button"
                    role="radio"
                    aria-checked={selected()}
                    tabindex={selected() ? 0 : -1}
                    disabled={props.isActive}
                    onClick={() => props.onBlueprint(item.id)}
                    onKeyDown={(event) =>
                      radioGroupKeyDown(event, agentBlueprints.length, index(), (next) =>
                        props.onBlueprint(agentBlueprints[next]!.id),
                      )
                    }
                    class={cn(
                      'velion-run-blueprint',
                      selected() && 'velion-run-blueprint--active',
                      controlFocusClass,
                    )}
                  >
                    <span class="velion-run-blueprint__icon">
                      <Icon size={15} strokeWidth={2.1} />
                    </span>
                    <span class="velion-run-blueprint__copy">
                      <span class="velion-run-blueprint__title">{item.shortTitle}</span>
                      <span class="velion-run-blueprint__eyebrow">{item.eyebrow}</span>
                    </span>
                  </button>
                )
              }}
            </For>
          </div>
        </div>

        <div class="velion-run-launcher__field">
          <p class="velion-run-launcher__label">Velion mode</p>
          <div class="velion-run-launcher__modes" role="radiogroup" aria-label="Velion mode">
            <For each={VELION_MODES}>
              {(item, index) => {
                const selected = () => props.modeId === item.id
                return (
                  <button
                    type="button"
                    role="radio"
                    aria-checked={selected()}
                    tabindex={selected() ? 0 : -1}
                    disabled={props.isActive}
                    onClick={() => props.onMode(item.id)}
                    onKeyDown={(event) =>
                      radioGroupKeyDown(event, VELION_MODES.length, index(), (next) =>
                        props.onMode(VELION_MODES[next]!.id),
                      )
                    }
                    class={cn(
                      'velion-run-mode',
                      selected() && 'velion-run-mode--active',
                      controlFocusClass,
                    )}
                  >
                    <span class="velion-run-mode__head">
                      <span class="velion-run-mode__label">{item.label}</span>
                      <span class={cn('velion-run-mode__badge', `velion-run-mode__badge--${item.badge}`)}>
                        {item.badge === 'premium' ? 'Premium' : 'Cheap'}
                      </span>
                    </span>
                    <span class="velion-run-mode__desc">{item.description}</span>
                  </button>
                )
              }}
            </For>
          </div>
        </div>

        <label class={cn('velion-run-launcher__toggle', controlFocusClass)}>
          <input
            type="checkbox"
            checked={props.browseWeb}
            disabled={props.isActive}
            onChange={(event) => props.onBrowseWeb(event.currentTarget.checked)}
          />
          <Globe2 size={14} strokeWidth={2.1} />
          Allow web browsing
        </label>
      </details>

      <p class="velion-run-launcher__note">
        <ShieldCheck size={13} strokeWidth={2.1} />
        Risky tools pause for your approval before they run.
      </p>

      <div class="velion-run-launcher__actions">
        <Show
          when={props.isActive}
          fallback={(
            <Button
              variant="primary"
              size="md"
              shape="pill"
              class={cn('velion-run-launcher__run', controlFocusClass)}
              disabled={!props.canRun}
              onClick={() => props.onRun()}
            >
              <Play size={15} strokeWidth={2.2} />
              Run task
            </Button>
          )}
        >
          <Button
            variant="primary"
            size="md"
            shape="pill"
            class={cn('velion-run-launcher__cancel', controlFocusClass)}
            onClick={() => props.onCancel()}
          >
            <Square size={13} strokeWidth={2.4} />
            Cancel run
          </Button>
        </Show>
      </div>
    </div>
  )
}

// ── Approval deck ─────────────────────────────────────────────────────────────

function ApprovalDeck(props: {
  approvals: Approval[]
  deciding: (approvalId: string) => boolean
  onDecide: (approvalId: string, decision: ApprovalDecision) => void
}) {
  return (
    <div class="velion-run-approvals" role="group" aria-label="Pending approvals">
      <For each={props.approvals}>
        {(approval) => {
          const busy = () => props.deciding(approval.id)
          return (
          <div class="velion-run-approval">
            <div class="velion-run-approval__head">
              <span class="velion-run-approval__badge">
                <PauseCircle size={13} strokeWidth={2.2} />
                Approval required
              </span>
              <span class="velion-run-approval__kind">{approval.kind ?? 'Gated action'}</span>
            </div>
            <p class="velion-run-approval__detail">
              {approval.detail ?? 'The agent is waiting for your decision before the next step.'}
            </p>
            <Show when={approval.requestedBy}>
              <p class="velion-run-approval__meta">Requested by {approval.requestedBy}</p>
            </Show>
            <div class="velion-run-approval__actions">
              <button
                type="button"
                class={cn('velion-run-approval__approve', controlFocusClass)}
                disabled={busy()}
                onClick={() => props.onDecide(approval.id, 'approve')}
              >
                <Show when={busy()} fallback={<CheckCircle2 size={14} strokeWidth={2.2} />}>
                  <Loader2 size={14} strokeWidth={2.2} class="velion-run-spin" />
                </Show>
                Approve
              </button>
              <button
                type="button"
                class={cn('velion-run-approval__reject', controlFocusClass)}
                disabled={busy()}
                onClick={() => props.onDecide(approval.id, 'reject')}
              >
                <Show when={busy()} fallback={<Square size={12} strokeWidth={2.4} />}>
                  <Loader2 size={13} strokeWidth={2.2} class="velion-run-spin" />
                </Show>
                Reject
              </button>
            </div>
          </div>
          )
        }}
      </For>
    </div>
  )
}

// ── Answer panel ──────────────────────────────────────────────────────────────

const ONBOARD_FLOW: readonly string[] = ['Plan', 'Act', 'Approve', 'Answer']

const ONBOARD_EXAMPLES: readonly string[] = [
  "Research our top 3 competitors' pricing and draft a comparison summary.",
  'Find the 5 most recent support tickets about refunds and summarize the themes.',
  'Pull this quarter’s revenue numbers and draft a short status update for the team.',
]

function AnswerPanel(props: {
  answer: string
  reasoning: string
  status: RunStatus
  streaming: boolean
  onUseExample: (text: string) => void
}) {
  const working = () => props.status === 'running' || props.status === 'paused'
  const emptyWorking = () => working() && props.answer.trim().length === 0
  const terminal = () =>
    props.status === 'done' || props.status === 'cancelled' || props.status === 'failed'

  return (
    <div class="velion-run-panel velion-run-answer">
      <div class="velion-run-panel__head">
        <span class="velion-run-panel__eyebrow"><Bot size={14} strokeWidth={2.1} /> Answer</span>
      </div>
      <Show when={props.reasoning.trim()}>
        <details class="velion-run-answer__reasoning">
          <summary><Brain size={13} strokeWidth={2.1} /> Reasoning</summary>
          <p>{props.reasoning}</p>
        </details>
      </Show>
      <Show
        when={!emptyWorking()}
        fallback={(
          <div class="velion-run-answer__working" aria-live="polite">
            <Loader2 size={15} strokeWidth={2.2} class="velion-run-spin" />
            <span>Agent is working…</span>
          </div>
        )}
      >
        <Show
          when={props.answer.trim()}
          fallback={(
            <Show
              when={props.status === 'idle'}
              fallback={<p class="velion-run-answer__empty">No answer was produced for this run.</p>}
            >
              <div class="velion-run-answer__onboard">
                <div class="velion-run-answer__how">
                  <For each={ONBOARD_FLOW}>
                    {(label, index) => (
                      <>
                        <span class="velion-run-answer__how-step">{label}</span>
                        <Show when={index() < ONBOARD_FLOW.length - 1}>
                          <span class="velion-run-answer__how-arrow" aria-hidden="true">→</span>
                        </Show>
                      </>
                    )}
                  </For>
                </div>
                <p class="velion-run-answer__empty">Launch a task to watch the agent plan, act, and report here. Try one:</p>
                <div class="velion-run-answer__examples">
                  <For each={ONBOARD_EXAMPLES}>
                    {(example) => (
                      <button
                        type="button"
                        class={cn('velion-run-answer__example', controlFocusClass)}
                        onClick={() => props.onUseExample(example)}
                      >
                        {example}
                      </button>
                    )}
                  </For>
                </div>
              </div>
            </Show>
          )}
        >
          <div
            class="velion-run-answer__body"
            classList={{ 'velion-run-answer--streaming': props.streaming }}
          >
            <For each={props.answer.split('\n')}>
              {(line) => (line.trim() ? <p>{line}</p> : <br />)}
            </For>
          </div>
          <Show when={working()}>
            <span class="velion-run-answer__cursor" aria-hidden="true" />
          </Show>
        </Show>
      </Show>
      <Show when={terminal()}>
        <Show when={props.status !== 'failed'}>
          <div
            class="velion-run-answer__status"
            classList={{
              'velion-run-answer__status--done': props.status === 'done',
              'velion-run-answer__status--cancelled': props.status === 'cancelled',
            }}
          >
            <Show
              when={props.status === 'cancelled'}
              fallback={(
                <>
                  <CheckCircle2 size={14} strokeWidth={2.2} />
                  <span>Completed</span>
                </>
              )}
            >
              <Square size={13} strokeWidth={2.4} />
              <span>Run cancelled — partial output above</span>
            </Show>
          </div>
        </Show>
      </Show>
    </div>
  )
}

// ── Trust panel ───────────────────────────────────────────────────────────────

function TrustPanel(props: { citations: Citation[]; usage: RunUsage | null }) {
  return (
    <div class="velion-run-panel velion-run-trust">
      <div class="velion-run-panel__head">
        <span class="velion-run-panel__eyebrow"><Link2 size={14} strokeWidth={2.1} /> Sources &amp; cost</span>
      </div>
      <Show when={props.usage}>
        {(usage) => (
          <>
            <p class="velion-run-trust__subhead">Usage</p>
            <div class="velion-run-trust__usage">
              <Show when={usage().confidence != null}>
                <UsageStat label="Confidence" value={`${Math.round((usage().confidence ?? 0) * 100)}%`} />
              </Show>
              <Show when={usage().inputTokens != null || usage().outputTokens != null}>
                <UsageStat
                  label="Tokens"
                  value={`${usage().inputTokens ?? 0} in · ${usage().outputTokens ?? 0} out`}
                />
              </Show>
              <Show when={usage().costUsd != null}>
                <UsageStat label="Cost" value={`$${(usage().costUsd ?? 0).toFixed(4)}`} />
              </Show>
              <Show when={usage().latencyMs != null}>
                <UsageStat label="Latency" value={`${usage().latencyMs} ms`} />
              </Show>
            </div>
          </>
        )}
      </Show>
      <Show when={props.citations.length > 0}>
        <p class="velion-run-trust__subhead">Sources</p>
        <div class="velion-run-trust__citations">
          <For each={props.citations}>
            {(citation, index) => (
              <a
                class="velion-run-citation"
                href={citation.url || '#'}
                target="_blank"
                rel="noopener noreferrer"
              >
                <span class="velion-run-citation__index">{index() + 1}</span>
                <span class="velion-run-citation__body">
                  <span class="velion-run-citation__title">{citation.title || citation.url}</span>
                  <Show when={citation.snippet}>
                    <span class="velion-run-citation__snippet">{citation.snippet}</span>
                  </Show>
                </span>
              </a>
            )}
          </For>
        </div>
      </Show>
    </div>
  )
}

function UsageStat(props: { label: string; value: string }) {
  return (
    <div class="velion-run-trust__stat">
      <span class="velion-run-trust__stat-label">{props.label}</span>
      <strong class="velion-run-trust__stat-value">{props.value}</strong>
    </div>
  )
}

// ── Timeline ──────────────────────────────────────────────────────────────────

const TIMELINE_ICON: Record<TimelineKind, IconComponent> = {
  step: CircleDot,
  tool: Wrench,
  reasoning: Brain,
  plan: ListChecks,
  todo: CheckSquare,
  browser: Globe2,
  subagent: Users,
  pause: PauseCircle,
  resume: Play,
}

/** Humanize raw orchestration status strings for the timeline pill. */
function statusLabel(status: string): string {
  const value = status.toLowerCase()
  if (value === 'dispatched') return 'Sent'
  if (value === 'received' || value === 'attached') return 'Done'
  if (value === 'paused') return 'Waiting'
  if (value === 'resumed') return 'Resumed'
  return status
}

function TimelinePanel(props: { entries: TimelineEntry[]; status: RunStatus }) {
  return (
    <div class="velion-run-panel velion-run-timeline">
      <div class="velion-run-panel__head">
        <span class="velion-run-panel__eyebrow"><ListChecks size={14} strokeWidth={2.1} /> Timeline</span>
        <span class="velion-run-timeline__count">{props.entries.length}</span>
      </div>
      <Show
        when={props.entries.length > 0}
        fallback={(
          <div class="velion-run-timeline__empty">
            <Show
              when={props.status === 'running'}
              fallback={<p>Live plan transitions, tool calls, and browser steps will appear here during a run.</p>}
            >
              <Loader2 size={15} strokeWidth={2.2} class="velion-run-spin" />
              <p>Agent is working… orchestration steps will stream in if the run emits them.</p>
            </Show>
          </div>
        )}
      >
        <ol class="velion-run-timeline__list">
          <For each={props.entries}>
            {(entry) => <TimelineRow entry={entry} />}
          </For>
        </ol>
      </Show>
    </div>
  )
}

function TimelineRow(props: { entry: TimelineEntry }) {
  const Icon = createMemo(() => TIMELINE_ICON[props.entry.kind])
  const statusTone = () => normalizeStatusTone(props.entry.status)
  const time = () => formatClock(props.entry.at)
  const pill = () => {
    const status = props.entry.status
    if (!status) return null
    const label = statusLabel(status)
    // Suppress the pill when it merely echoes the title (e.g. "Plan" / "Todo").
    if (label.toLowerCase() === props.entry.title.toLowerCase()) return null
    return label
  }
  return (
    <li class={cn('velion-run-timeline__row', `velion-run-timeline__row--${props.entry.kind}`)}>
      <span class="velion-run-timeline__icon">
        {(() => {
          const Glyph = Icon()
          return <Glyph size={14} strokeWidth={2.1} />
        })()}
      </span>
      <div class="velion-run-timeline__copy">
        <div class="velion-run-timeline__title-row">
          <span class="velion-run-timeline__title">{props.entry.title}</span>
          <Show when={time()}>
            <time class="velion-run-timeline__time">{time()}</time>
          </Show>
          <Show when={pill()}>
            {(label) => (
              <span class={cn('velion-run-timeline__pill', `velion-run-timeline__pill--${statusTone()}`)}>
                {label()}
              </span>
            )}
          </Show>
        </div>
        <Show when={props.entry.detail}>
          <p class="velion-run-timeline__detail">{props.entry.detail}</p>
        </Show>
      </div>
    </li>
  )
}

// ── Helpers ─────────────────────────────────────────────────────────────────

function shortId(id: string): string {
  return id.length > 10 ? `${id.slice(0, 8)}…` : id
}

/** Format an ISO timestamp as HH:MM:SS; empty string if unparseable. */
function formatClock(at: string): string {
  if (!at) return ''
  const date = new Date(at)
  if (Number.isNaN(date.getTime())) return ''
  return date.toLocaleTimeString(undefined, { hour12: false, hour: '2-digit', minute: '2-digit', second: '2-digit' })
}

function normalizeStatusTone(status?: string): 'ok' | 'warn' | 'error' | 'neutral' {
  const value = (status ?? '').toLowerCase()
  if (value.includes('error') || value.includes('fail') || value.includes('denied') || value.includes('reject')) {
    return 'error'
  }
  if (value.includes('pause') || value.includes('wait') || value.includes('request')) return 'warn'
  if (
    value.includes('done') ||
    value.includes('complete') ||
    value.includes('grant') ||
    value.includes('approve') ||
    value.includes('resume') ||
    value.includes('received')
  ) {
    return 'ok'
  }
  return 'neutral'
}

/** Map a backend run status string onto the console's `RunStatus` union. */
function statusToRunStatus(status: string): RunStatus {
  switch (status.toLowerCase()) {
    case 'running':
    case 'queued':
      return 'running'
    case 'paused':
    case 'awaiting_approval':
      return 'paused'
    case 'completed':
      return 'done'
    case 'failed':
      return 'failed'
    case 'cancelled':
      return 'cancelled'
    default:
      return 'idle'
  }
}

/** Recency buckets for the history rail, in display order. */
type RunGroup = { label: string; runs: RunDetail[] }

/** Group runs into Today / This week / Earlier by their creation date. */
function groupRunsByRecency(runs: RunDetail[]): RunGroup[] {
  const now = Date.now()
  const startOfToday = new Date()
  startOfToday.setHours(0, 0, 0, 0)
  const todayMs = startOfToday.getTime()
  const weekMs = todayMs - 6 * 24 * 60 * 60 * 1000

  const today: RunDetail[] = []
  const week: RunDetail[] = []
  const earlier: RunDetail[] = []

  for (const run of runs) {
    const at = run.createdAt ? new Date(run.createdAt).getTime() : now
    if (Number.isNaN(at) || at >= todayMs) today.push(run)
    else if (at >= weekMs) week.push(run)
    else earlier.push(run)
  }

  return [
    { label: 'Today', runs: today },
    { label: 'This week', runs: week },
    { label: 'Earlier', runs: earlier },
  ].filter((group) => group.runs.length > 0)
}

/** Relative "x ago" for the history row time; falls back to a clock. */
function formatRelative(at?: string): string {
  if (!at) return ''
  const time = new Date(at).getTime()
  if (Number.isNaN(time)) return ''
  const diff = Date.now() - time
  const min = Math.floor(diff / 60_000)
  if (min < 1) return 'just now'
  if (min < 60) return `${min}m ago`
  const hr = Math.floor(min / 60)
  if (hr < 24) return `${hr}h ago`
  const day = Math.floor(hr / 24)
  if (day < 7) return `${day}d ago`
  return new Date(at).toLocaleDateString(undefined, { month: 'short', day: 'numeric' })
}

/** Elapsed run time between two ISO timestamps, e.g. "1m 12s". Empty if unknown. */
function formatDuration(startAt?: string, endAt?: string): string {
  if (!startAt || !endAt) return ''
  const start = new Date(startAt).getTime()
  const end = new Date(endAt).getTime()
  if (Number.isNaN(start) || Number.isNaN(end) || end < start) return ''
  const totalSec = Math.round((end - start) / 1000)
  if (totalSec < 60) return `${totalSec}s`
  const min = Math.floor(totalSec / 60)
  const sec = totalSec % 60
  return `${min}m ${sec}s`
}

// ── History rail ───────────────────────────────────────────────────────────

/** The Velion modes whose label this run was launched under, by stored mode/id. */
function modeLabelFor(value?: string): string | undefined {
  if (!value) return undefined
  return VELION_MODES.find((item) => item.id === value)?.label
}

function HistoryRail(props: {
  activeRunId: string | null
  loading: boolean
  runs: RunDetail[]
  onSelect: (run: RunDetail) => void
}) {
  const groups = createMemo(() => groupRunsByRecency(props.runs))

  return (
    <div class="velion-run-panel velion-run-history" aria-label="Run history">
      <div class="velion-run-panel__head">
        <span class="velion-run-panel__eyebrow"><History size={14} strokeWidth={2.1} /> History</span>
        <Show when={props.runs.length > 0}>
          <span class="velion-run-timeline__count">{props.runs.length}</span>
        </Show>
      </div>

      <Show
        when={props.runs.length > 0}
        fallback={(
          <p class="velion-run-history__empty">
            <Show when={props.loading} fallback="Past runs for this conversation will appear here.">
              Loading runs…
            </Show>
          </p>
        )}
      >
        <div class="velion-run-history__groups">
          <For each={groups()}>
            {(group) => (
              <div class="velion-run-history__group">
                <p class="velion-run-history__group-label">{group.label}</p>
                <ul class="velion-run-history__list">
                  <For each={group.runs}>
                    {(run) => (
                      <li>
                        <button
                          type="button"
                          class="velion-run-history__row"
                          classList={{ 'velion-run-history__row--active': run.runId === props.activeRunId }}
                          aria-current={run.runId === props.activeRunId ? 'true' : undefined}
                          onClick={() => props.onSelect(run)}
                        >
                          <span class="velion-run-history__goal">{run.goal || 'Untitled run'}</span>
                          <span class="velion-run-history__meta">
                            <RunStatusPill status={run.status} />
                            <span class="velion-run-history__time">{formatRelative(run.createdAt)}</span>
                          </span>
                        </button>
                      </li>
                    )}
                  </For>
                </ul>
              </div>
            )}
          </For>
        </div>
      </Show>
    </div>
  )
}

/** Compact status pill for a history row, toned by the run's terminal state. */
function RunStatusPill(props: { status: string }) {
  const tone = createMemo(() => normalizeStatusTone(props.status))
  return (
    <span class="velion-run-history__pill" data-tone={tone()}>
      {statusLabel(props.status)}
    </span>
  )
}

// ── Telemetry panel ──────────────────────────────────────────────────────────

function TelemetryPanel(props: {
  detail: RunDetail | null
  live: RunUsage | null
  mode: VelionMode
  status: RunStatus
  onResume: () => void
}) {
  // Step N of M: completed steps from the durable detail; total prefers the
  // checkpoint count when it leads the completed count (Rox-style progress).
  const stepLabel = createMemo(() => {
    const detail = props.detail
    if (!detail) return null
    const done = detail.stepsCompleted
    const total = Math.max(done, detail.checkpointIndex)
    return total > 0 ? `Step ${done} of ${total}` : `${done} steps`
  })

  // ETA is only meaningful for an in-flight run with known progress.
  const eta = createMemo(() => {
    const detail = props.detail
    if (!detail || props.status !== 'running') return null
    const done = detail.stepsCompleted
    const total = Math.max(done, detail.checkpointIndex)
    if (total <= done) return null
    const remaining = total - done
    return `~${remaining} step${remaining === 1 ? '' : 's'} left`
  })

  const tokens = createMemo(() => {
    const live = props.live
    const detail = props.detail
    const input = live?.inputTokens ?? detail?.inputTokens ?? 0
    const output = live?.outputTokens ?? detail?.outputTokens ?? 0
    if (input === 0 && output === 0) return null
    return `${input} in · ${output} out`
  })

  // The mode this run was launched under: stored mode string on the detail, else
  // the live picker selection.
  const launchedMode = createMemo(
    () => modeLabelFor(props.detail?.mode) ?? props.mode.label,
  )

  const runTime = createMemo(() => formatDuration(props.detail?.createdAt, props.detail?.updatedAt))
  const residency = createMemo(() => props.detail?.residency)

  return (
    <div class="velion-run-panel velion-run-telemetry">
      <div class="velion-run-panel__head">
        <span class="velion-run-panel__eyebrow"><Gauge size={14} strokeWidth={2.1} /> Run telemetry</span>
        <span class="velion-run-telemetry__status" data-tone={normalizeStatusTone(props.status)}>
          {STATUS_LABEL[props.status]}
        </span>
      </div>

      <div class="velion-run-telemetry__grid">
        <Show when={stepLabel()}>
          {(label) => (
            <TelemetryStat Icon={ListChecks} label="Progress" value={label()} hint={eta() ?? undefined} />
          )}
        </Show>
        <Show when={props.live?.costUsd != null}>
          <TelemetryStat Icon={Coins} label="Cost" value={`$${(props.live?.costUsd ?? 0).toFixed(4)}`} />
        </Show>
        <Show when={runTime()}>
          {(value) => <TelemetryStat Icon={Timer} label="Run time" value={value()} />}
        </Show>
        <Show when={props.live?.latencyMs != null}>
          <TelemetryStat Icon={Clock} label="Latency" value={`${props.live?.latencyMs} ms`} />
        </Show>
        <Show when={tokens()}>
          {(value) => <TelemetryStat Icon={CircleDot} label="Tokens" value={value()} />}
        </Show>
        <TelemetryStat Icon={Sparkles} label="Mode" value={launchedMode()} />
      </div>

      <div class="velion-run-telemetry__provenance">
        <ShieldCheck size={13} strokeWidth={2.1} />
        <Show
          when={residency()}
          fallback={<span>Grounded in your workspace · EU data residency</span>}
        >
          {(region) => <span>Processed in {region()} · grounded in your workspace</span>}
        </Show>
      </div>

      <Show when={props.status === 'paused'}>
        <div class="velion-run-telemetry__actions">
          <Button variant="primary" size="sm" shape="pill" onClick={() => props.onResume()}>
            <RotateCcw class="size-3.5" />
            Resume run
          </Button>
        </div>
      </Show>
    </div>
  )
}

function TelemetryStat(props: { Icon: IconComponent; label: string; value: string; hint?: string }) {
  return (
    <div class="velion-run-telemetry__stat">
      <span class="velion-run-telemetry__stat-label">
        <props.Icon size={13} strokeWidth={2.1} /> {props.label}
      </span>
      <strong class="velion-run-telemetry__stat-value">{props.value}</strong>
      <Show when={props.hint}>
        <span class="velion-run-telemetry__stat-hint">{props.hint}</span>
      </Show>
    </div>
  )
}
