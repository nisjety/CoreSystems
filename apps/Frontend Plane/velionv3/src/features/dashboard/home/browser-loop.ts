/**
 * Pause/resume/stop control for the capped AI browser loop.
 *
 * The controller is framework-free so the state machine is unit-testable: the
 * loop driver (KnowledgeComposer) mirrors state changes into a Solid signal via
 * `onChange`. User interrupts take effect between steps — the driver awaits
 * `gate()` before each suggest and each act, so a pause or stop never aborts a
 * browser action that is already executing, and a stopped loop never leaves the
 * session in an unknown state (the session itself stays open and interactive).
 */

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
   */
  gate: () => Promise<BrowserLoopGateResult>
  finish: (outcome: 'done' | 'stopped', error?: string | null) => void
  markActing: () => void
  markStepDone: () => void
  markSuggesting: () => void
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

  const emit = (patch: Partial<BrowserLoopState>) => {
    state = { ...state, ...patch }
    onChange?.(state)
  }

  const releaseWaiters = () => {
    const pending = resumeWaiters
    resumeWaiters = []
    for (const resolve of pending) resolve()
  }

  return {
    begin(goal: string): boolean {
      if (isBrowserLoopRunning(state.status)) return false
      pauseRequested = false
      stopRequested = false
      emit({ error: null, goal, status: 'suggesting', step: 0 })
      return true
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
    requestPause() {
      if (isBrowserLoopRunning(state.status)) pauseRequested = true
    },
    requestResume() {
      pauseRequested = false
      releaseWaiters()
    },
    requestStop() {
      if (!isBrowserLoopRunning(state.status)) return
      stopRequested = true
      releaseWaiters()
    },
    state: () => state,
  }
}
