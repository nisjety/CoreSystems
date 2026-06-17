'use client'

import { useEffect, useRef, useState } from 'react'

/**
 * Shared run-event (task-graph) subscription.
 *
 * One `EventSource` to `/api/agents/runs/{runId}/events` replaces the per-hook
 * polling that chat / playground / finetune / cron each reinvented. The browser
 * carries `Last-Event-Id` across reconnects automatically, so the gateway +
 * session-core resume from the buffered cursor (docs/HARNESS_PHASE1.md §3a).
 *
 * The hook reduces the event stream into the current task-graph view:
 * latest plan/todo/approval states, whether the run is paused awaiting an
 * approval, and the connection status. It does not own any network retry —
 * `EventSource` reconnects on its own.
 */

export type RunConnectionStatus = 'idle' | 'connecting' | 'open' | 'closed'

export interface PlanTransition {
  planId: string
  from: string
  to: string
}

export interface TodoTransition {
  todoId: string
  from: string
  to: string
}

export interface ApprovalSnapshot {
  approvalId: string
  kind: string
  state: string
  decidedBy: string
}

export interface SubagentEdge {
  parentRunId: string
  childRunId: string
  role: string
}

export interface RunEventsState {
  status: RunConnectionStatus
  /** True while the run is suspended awaiting a human approval / handoff. */
  paused: boolean
  /** Approval id the run is currently blocked on, if any. */
  pendingApprovalId: string | null
  /** Latest transition per plan id. */
  plans: Record<string, PlanTransition>
  /** Latest transition per todo id. */
  todos: Record<string, TodoTransition>
  /** Latest known state per approval id. */
  approvals: Record<string, ApprovalSnapshot>
  /** Subagent lineage edges seen on this connection. */
  subagents: SubagentEdge[]
  /** Last SSE event id observed (the resume cursor). */
  lastEventId: string | null
}

const INITIAL: RunEventsState = {
  status: 'idle',
  paused: false,
  pendingApprovalId: null,
  plans: {},
  todos: {},
  approvals: {},
  subagents: [],
  lastEventId: null,
}

type EventName =
  | 'plan_transitioned'
  | 'todo_transitioned'
  | 'approval_state_changed'
  | 'subagent_attached'
  | 'subagent_stopped'
  | 'run_paused_for_approval'
  | 'run_resumed_after_approval'

const EVENT_NAMES: readonly EventName[] = [
  'plan_transitioned',
  'todo_transitioned',
  'approval_state_changed',
  'subagent_attached',
  'subagent_stopped',
  'run_paused_for_approval',
  'run_resumed_after_approval',
]

function reduce(state: RunEventsState, name: EventName, data: Record<string, unknown>): RunEventsState {
  switch (name) {
    case 'plan_transitioned': {
      const planId = String(data.plan_id ?? '')
      if (!planId) return state
      return {
        ...state,
        plans: {
          ...state.plans,
          [planId]: { planId, from: String(data.from ?? ''), to: String(data.to ?? '') },
        },
      }
    }
    case 'todo_transitioned': {
      const todoId = String(data.todo_id ?? '')
      if (!todoId) return state
      return {
        ...state,
        todos: {
          ...state.todos,
          [todoId]: { todoId, from: String(data.from ?? ''), to: String(data.to ?? '') },
        },
      }
    }
    case 'approval_state_changed': {
      const approvalId = String(data.approval_id ?? '')
      if (!approvalId) return state
      const snapshot: ApprovalSnapshot = {
        approvalId,
        kind: String(data.approval_kind ?? ''),
        state: String(data.to ?? ''),
        decidedBy: String(data.decided_by ?? ''),
      }
      // A decided approval clears the pause it caused.
      const decided = snapshot.state !== '' && snapshot.state !== 'APPROVAL_STATE_REQUESTED'
      return {
        ...state,
        approvals: { ...state.approvals, [approvalId]: snapshot },
        paused: decided && state.pendingApprovalId === approvalId ? false : state.paused,
        pendingApprovalId:
          decided && state.pendingApprovalId === approvalId ? null : state.pendingApprovalId,
      }
    }
    case 'subagent_attached': {
      return {
        ...state,
        subagents: [
          ...state.subagents,
          {
            parentRunId: String(data.parent_run_id ?? ''),
            childRunId: String(data.child_run_id ?? ''),
            role: String(data.role ?? ''),
          },
        ],
      }
    }
    case 'subagent_stopped':
      return state
    case 'run_paused_for_approval':
      return {
        ...state,
        paused: true,
        pendingApprovalId: String(data.approval_id ?? '') || null,
      }
    case 'run_resumed_after_approval':
      return { ...state, paused: false, pendingApprovalId: null }
    default:
      return state
  }
}

export interface UseRunEventsOptions {
  /** Pause the subscription without unmounting (e.g. tab hidden). */
  enabled?: boolean
}

export function useRunEvents(
  runId: string | null | undefined,
  options: UseRunEventsOptions = {},
): RunEventsState {
  const enabled = options.enabled ?? true
  const [state, setState] = useState<RunEventsState>(INITIAL)
  // Track the latest event id outside React state so the reducer effect doesn't
  // need it as a dependency.
  const lastIdRef = useRef<string | null>(null)

  useEffect(() => {
    if (!runId || !enabled) {
      setState(INITIAL)
      lastIdRef.current = null
      return
    }

    setState({ ...INITIAL, status: 'connecting' })

    // Seed current authoritative state from the snapshot before/while the live
    // tail attaches. This is the correctness backstop: on reload (no
    // Last-Event-Id) or when the server replay buffer has evicted our cursor,
    // the snapshot reconciles state that the live feed alone would miss.
    let cancelled = false
    void fetch(`/api/agents/runs/${encodeURIComponent(runId)}/snapshot`, {
      cache: 'no-store',
    })
      .then((res) => (res.ok ? res.json() : null))
      .then((snap: SnapshotResponse | null) => {
        if (cancelled || !snap) return
        setState((prev) => applySnapshot(prev, snap))
      })
      .catch(() => {
        /* snapshot is best-effort; the live feed still attaches */
      })

    const source = new EventSource(`/api/agents/runs/${encodeURIComponent(runId)}/events`)

    source.onopen = () => {
      setState((prev) => ({ ...prev, status: 'open' }))
    }

    source.onerror = () => {
      // EventSource auto-reconnects (and resends Last-Event-Id). Surface the
      // transient closed state; it flips back to 'open' on reconnect.
      setState((prev) => ({ ...prev, status: 'connecting' }))
    }

    const handlers = EVENT_NAMES.map((name) => {
      const listener = (event: MessageEvent) => {
        let data: Record<string, unknown>
        try {
          data = JSON.parse(event.data) as Record<string, unknown>
        } catch {
          return
        }
        if (event.lastEventId) lastIdRef.current = event.lastEventId
        setState((prev) => ({
          ...reduce(prev, name, data),
          status: 'open',
          lastEventId: lastIdRef.current,
        }))
      }
      source.addEventListener(name, listener as EventListener)
      return { name, listener }
    })

    return () => {
      cancelled = true
      for (const { name, listener } of handlers) {
        source.removeEventListener(name, listener as EventListener)
      }
      source.close()
    }
  }, [runId, enabled])

  return state
}

interface SnapshotPlan {
  id: string
  state: string
}

interface SnapshotApproval {
  id: string
  kind: string
  state: string
  decided_by?: string | null
}

interface SnapshotResponse {
  plans?: SnapshotPlan[]
  approvals?: SnapshotApproval[]
}

/** Merge a run snapshot into existing state without clobbering live events. */
function applySnapshot(prev: RunEventsState, snap: SnapshotResponse): RunEventsState {
  const plans = { ...prev.plans }
  for (const p of snap.plans ?? []) {
    // Don't overwrite a transition already seen live (which carries `from`).
    if (!plans[p.id]) plans[p.id] = { planId: p.id, from: '', to: p.state }
  }

  const approvals = { ...prev.approvals }
  let paused = prev.paused
  let pendingApprovalId = prev.pendingApprovalId
  for (const a of snap.approvals ?? []) {
    if (!approvals[a.id]) {
      approvals[a.id] = {
        approvalId: a.id,
        kind: a.kind,
        state: a.state,
        decidedBy: a.decided_by ?? '',
      }
    }
    if (a.state === 'APPROVAL_STATE_REQUESTED' && !pendingApprovalId) {
      paused = true
      pendingApprovalId = a.id
    }
  }

  return { ...prev, plans, approvals, paused, pendingApprovalId }
}
