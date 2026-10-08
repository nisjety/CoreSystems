/**
 * "Varsle meg når ferdig" — a small toggle that asks to be told when a run
 * finishes, so a long turn can finish in the background.
 *
 * It rides the existing watcher endpoints through `runs-client` (`watchRun`,
 * `unwatchRun`, `getRunWatchStatus`): a durable per-user subscription that
 * survives the tab closing, delivered through the notification bell. The
 * endpoints are idempotent and owner-only by the verified principal, so this
 * control carries no identity of its own and sends nothing but the run id.
 *
 * Written fresh rather than lifted out of `AgentRunConsole`: that file carries
 * other work in progress, and the console's toggle lives inside its telemetry
 * panel. This one has to sit in two places — beside the elapsed counter of the
 * waiting state, and in the live run panel's header.
 *
 * State is kept per run id at module level, not inside the component. The
 * transcript rebuilds the waiting indicator on every derivation (see
 * `ThinkingDots`), so a control that held its own state would fall back to "off"
 * and ask the server again each time a streamed tool call arrived. Held per run,
 * a rebuilt control is correct on its first frame and the status is read once.
 */

import { Show, createEffect, createMemo, createRoot, createSignal, type Accessor } from 'solid-js'
import { Bell, BellRing } from '@/shared/icons'
import { getRunWatchStatus, unwatchRun, watchRun } from '@/shared/api/runs-client'
import { useI18n } from '@/shared/i18n'

type NotifyFailure = 'on' | 'off'

type RunNotifyState = {
  watching: Accessor<boolean>
  setWatching: (value: boolean) => void
  pending: Accessor<boolean>
  setPending: (value: boolean) => void
  failure: Accessor<NotifyFailure | null>
  setFailure: (value: NotifyFailure | null) => void
  /** Not reactive: guards a second click that lands before Solid commits `pending`. */
  inFlight: boolean
  /** The reader has acted, so a status read that lands late must not overwrite it. */
  touched: boolean
  /** The status read for this run, started at most once. */
  statusRead: boolean
}

/**
 * One entry per run the reader has seen a waiting indicator for. Bounded so a
 * long session cannot grow it without limit; the oldest entry goes first, and a
 * forgotten run only costs one more status read when it is next shown.
 */
const MAX_TRACKED_RUNS = 64
const states = new Map<string, RunNotifyState>()

function createState(): RunNotifyState {
  const [watching, setWatching] = createSignal(false)
  const [pending, setPending] = createSignal(false)
  const [failure, setFailure] = createSignal<NotifyFailure | null>(null)
  return {
    watching,
    setWatching: (value) => setWatching(value),
    pending,
    setPending: (value) => setPending(value),
    failure,
    setFailure: (value) => setFailure(value),
    inFlight: false,
    touched: false,
    statusRead: false,
  }
}

function stateFor(runId: string): RunNotifyState {
  const known = states.get(runId)
  if (known) return known
  if (states.size >= MAX_TRACKED_RUNS) {
    const oldest = states.keys().next().value
    if (oldest !== undefined) states.delete(oldest)
  }
  // A detached root: signals made inside a component's own memo would be owned by
  // it and go with it on unmount, which is exactly when a rebuilt control needs them.
  const created = createRoot(() => createState())
  states.set(runId, created)
  return created
}

/** Test isolation only: the per-run state outlives any one mounted control. */
export function __resetRunNotifyForTests(): void {
  states.clear()
}

/**
 * Read the caller's current watch on this run, once. An unreadable status is
 * treated as "not watching" and never surfaced: the reader asked for nothing
 * yet, and pressing the control still does the right thing (the POST is
 * idempotent).
 */
async function readStatusOnce(runId: string, state: RunNotifyState): Promise<void> {
  if (state.statusRead) return
  state.statusRead = true
  try {
    const status = await getRunWatchStatus(runId)
    if (!state.touched) state.setWatching(status.watching)
  } catch {
    // See above.
  }
}

async function toggle(runId: string, state: RunNotifyState): Promise<void> {
  if (state.inFlight) return
  state.inFlight = true
  state.touched = true
  const next = !state.watching()
  state.setPending(true)
  state.setFailure(null)
  // Optimistic, and reverted below if the server refuses: the control never
  // keeps a false "on" or "off".
  state.setWatching(next)
  try {
    if (next) await watchRun(runId)
    else await unwatchRun(runId)
  } catch {
    state.setWatching(!next)
    state.setFailure(next ? 'on' : 'off')
  } finally {
    state.inFlight = false
    state.setPending(false)
  }
}

export function RunNotifyButton(props: {
  /** The durable run to be told about. */
  runId: string
  /**
   * Short wording for a cramped surface (the live panel's header). The full
   * sentence stays in the tooltip, so nothing is lost but the width.
   */
  compact?: boolean
}) {
  const i18n = useI18n()
  const state = createMemo(() => stateFor(props.runId))

  createEffect(
    () => props.runId,
    (runId) => {
      void readStatusOnce(runId, stateFor(runId))
    },
  )

  const label = () => {
    if (state().watching()) {
      return props.compact
        ? i18n.tr('Du varsles', "You'll be told")
        : i18n.tr('Du varsles når ferdig', "You'll be told when done")
    }
    return props.compact
      ? i18n.tr('Varsle meg', 'Notify me')
      : i18n.tr('Varsle meg når ferdig', 'Notify me when done')
  }
  const hint = () => state().watching()
    ? i18n.tr(
      'Du får et varsel i bjella når svaret er ferdig. Klikk for å avslutte varslingen.',
      "You'll get a notice in the bell when the answer is done. Click to cancel.",
    )
    : i18n.tr(
      'Få et varsel i bjella når svaret er ferdig, så du kan gjøre noe annet imens.',
      "Get a notice in the bell when the answer is done, so you can do something else meanwhile.",
    )
  const failureText = () => {
    const failed = state().failure()
    if (failed === 'on') return i18n.tr('Kunne ikke slå på varsling', "Couldn't turn on the notification")
    if (failed === 'off') return i18n.tr('Kunne ikke slå av varsling', "Couldn't turn off the notification")
    return ''
  }

  return (
    <>
      <button
        type="button"
        class={[
          'verevon-run-watch-toggle',
          'verevon-run-notify',
          { 'verevon-run-watch-toggle--active': state().watching() },
        ]}
        aria-pressed={state().watching() ? 'true' : 'false'}
        disabled={state().pending()}
        title={hint()}
        onClick={() => void toggle(props.runId, state())}
      >
        <Show when={state().watching()} fallback={<Bell size={13} strokeWidth={2.1} />}>
          <BellRing size={13} strokeWidth={2.1} />
        </Show>
        {label()}
      </button>
      <Show when={state().failure()}>
        <small class="verevon-chat-thinking__elapsed" role="alert">{failureText()}</small>
      </Show>
    </>
  )
}
