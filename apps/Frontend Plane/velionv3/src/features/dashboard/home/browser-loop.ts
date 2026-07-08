/**
 * Pause/resume/stop control for the capped AI browser loop.
 *
 * The controller is framework-free so the state machine is unit-testable: the
 * loop driver (KnowledgeComposer) mirrors state changes into a Solid signal via
 * `onChange`. User interrupts take effect between steps — the driver awaits
 * `gate()` before each suggest and each act, so a pause or stop never aborts a
 * browser action that is already executing, and a stopped loop never leaves the
 * session in an unknown state (the session itself stays open and interactive).
 *
 * Phase 2 (durable browser-agent run) note: the loop now legitimately runs
 * two different ways —
 *  1. The original client-side capped loop (`performBrowserAutoRun` in
 *     KnowledgeComposer.tsx), which still drives `begin`/`gate`/`markActing`/
 *     `markSuggesting`/`markStepDone`/`finish` exactly as before. Unchanged.
 *  2. A durable, server-side run (`beginBrowserAiRun`, pending wiring) whose
 *     progress arrives as SSE events, not client-side polling. For that mode
 *     `requestPause`/`requestResume`/`requestStop` call the new
 *     `controlBrowserAiRun` network trigger instead of flipping a local flag
 *     — the actual status transition happens when the corresponding
 *     `browser_run_paused`/`browser_run_resumed`/terminal SSE event arrives,
 *     via the new `onActionDispatched`/`onObservationReceived`/
 *     `onRunPaused`/`onRunResumed` reducer methods below. `attachRun` records
 *     which run a subsequent `request*` call controls; without it the
 *     `request*` calls fall back to the pre-Phase-2 local-flag behavior, so
 *     the client-side loop above is entirely unaffected.
 */

import { controlBrowserAiRun } from '@/shared/api/browser-run-client'

export type BrowserLoopStatus = 'idle' | 'suggesting' | 'acting' | 'paused' | 'stopped' | 'done'

export type BrowserLoopState = {
  error: string | null
  goal: string
  status: BrowserLoopStatus
  /** Completed act steps in the current run. */
  step: number
}

export type BrowserLoopGateResult = 'continue' | 'stopped'

export type BrowserLoopController = {
  /** Start a new run; returns false when a run is already active. */
  begin: (goal: string) => boolean
  /**
   * Checkpoint between steps: resolves 'stopped' when the user stopped the
   * loop, waits while paused, otherwise resolves 'continue' immediately.
   * Client-side-loop mode only (see file header) — a durable server-side run
   * never calls this.
   */
  gate: () => Promise<BrowserLoopGateResult>
  /**
   * Phase 2: record which durable server-side run `requestPause`/
   * `requestResume`/`requestStop` should control. Call once the run has
   * actually started (after `startBrowserAiRun` resolves). Until called,
   * `request*` fall back to the pre-Phase-2 local-flag behavior, so the
   * client-side loop (mode 1, see file header) is unaffected.
   */
  attachRun: (params: { orgId: string; runId: string } | null) => void
  finish: (outcome: 'done' | 'stopped', error?: string | null) => void
  markActing: () => void
  markStepDone: () => void
  markSuggesting: () => void
  /** Phase 2 reducer: a server-driven run dispatched an action but hasn't
   * observed its result yet — same visual state as `markActing`. */
  onActionDispatched: () => void
  /** Phase 2 reducer: a server-driven run's dispatched action resolved —
   * same visual state (and step increment) as `markStepDone`. */
  onObservationReceived: () => void
  /** Phase 2 reducer: the server confirmed the run is paused. */
  onRunPaused: () => void
  /** Phase 2 reducer: the server confirmed the run resumed — returns to
   * whichever running status the loop was in before the pause. */
  onRunResumed: () => void
  requestPause: () => void
  requestResume: () => void
  requestStop: () => void
  state: () => BrowserLoopState
}

const RUNNING_STATUSES: ReadonlySet<BrowserLoopStatus> = new Set(['suggesting', 'acting', 'paused'])

export function isBrowserLoopRunning(status: BrowserLoopStatus): boolean {
  return RUNNING_STATUSES.has(status)
}

export function createBrowserLoopController(
  onChange?: (state: BrowserLoopState) => void,
): BrowserLoopController {
  let state: BrowserLoopState = { error: null, goal: '', status: 'idle', step: 0 }
  let pauseRequested = false
  let stopRequested = false
  let resumeWaiters: Array<() => void> = []
  // Phase 2: which durable server-side run (if any) `request*` controls, and
  // which running status to restore when the server confirms a resume.
  let activeRun: { orgId: string; runId: string } | null = null
  let statusBeforePause: BrowserLoopStatus | null = null

  const emit = (patch: Partial<BrowserLoopState>) => {
    state = { ...state, ...patch }
    onChange?.(state)
  }

  const releaseWaiters = () => {
    const pending = resumeWaiters
    resumeWaiters = []
    for (const resolve of pending) resolve()
  }

  // Fire-and-forget: the actual status transition comes from the run's own
  // SSE event (browser_run_paused/browser_run_resumed/terminal), not from
  // this call's HTTP response — a transport hiccup here must not desync the
  // UI from what the server stream reports.
  const controlActiveRun = (action: 'pause' | 'resume' | 'stop') => {
    if (!activeRun) return false
    void controlBrowserAiRun(activeRun.orgId, activeRun.runId, action).catch(() => {
      // Best-effort; surfaced (if at all) via the run's own error/done event.
    })
    return true
  }

  return {
    begin(goal: string): boolean {
      if (isBrowserLoopRunning(state.status)) return false
      pauseRequested = false
      stopRequested = false
      activeRun = null
      statusBeforePause = null
      emit({ error: null, goal, status: 'suggesting', step: 0 })
      return true
    },
    attachRun(params) {
      activeRun = params
    },
    async gate(): Promise<BrowserLoopGateResult> {
      if (stopRequested) {
        emit({ status: 'stopped' })
        return 'stopped'
      }
      if (pauseRequested) {
        emit({ status: 'paused' })
        await new Promise<void>((resolve) => {
          resumeWaiters.push(resolve)
        })
        if (stopRequested) {
          emit({ status: 'stopped' })
          return 'stopped'
        }
      }
      return 'continue'
    },
    finish(outcome: 'done' | 'stopped', error?: string | null) {
      pauseRequested = false
      stopRequested = false
      activeRun = null
      statusBeforePause = null
      releaseWaiters()
      emit({ error: error ?? null, status: outcome })
    },
    markActing() {
      if (!stopRequested && isBrowserLoopRunning(state.status)) emit({ status: 'acting' })
    },
    markStepDone() {
      emit({ step: state.step + 1 })
    },
    markSuggesting() {
      if (!stopRequested && isBrowserLoopRunning(state.status)) emit({ status: 'suggesting' })
    },
    onActionDispatched() {
      if (isBrowserLoopRunning(state.status)) emit({ status: 'acting' })
    },
    onObservationReceived() {
      emit({ step: state.step + 1 })
    },
    onRunPaused() {
      statusBeforePause = isBrowserLoopRunning(state.status) && state.status !== 'paused' ? state.status : statusBeforePause
      emit({ status: 'paused' })
    },
    onRunResumed() {
      emit({ status: statusBeforePause ?? 'acting' })
      statusBeforePause = null
    },
    requestPause() {
      if (controlActiveRun('pause')) return
      if (isBrowserLoopRunning(state.status)) pauseRequested = true
    },
    requestResume() {
      if (controlActiveRun('resume')) return
      pauseRequested = false
      releaseWaiters()
    },
    requestStop() {
      if (controlActiveRun('stop')) return
      if (!isBrowserLoopRunning(state.status)) return
      stopRequested = true
      releaseWaiters()
    },
    state: () => state,
  }
}
