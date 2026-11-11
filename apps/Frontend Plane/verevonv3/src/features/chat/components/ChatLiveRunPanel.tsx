/**
 * Chat-side live agent panel — the right half of the chat split view.
 *
 * Nothing here reimplements a browser surface. When the watched run has real
 * frame evidence the panel mounts the EXISTING `BrowserChrome` component (the
 * same one the Knowledge surface renders) in its compact variant; the run's
 * per-step screenshots come from the BFF artifact route, and every step's
 * `screenshotRef` — parsed by `run-console-client.ts` and previously dropped on
 * the floor — is rendered as an expandable thumbnail.
 *
 * The panel never shows a spinner it cannot resolve: `screenshotStateFor` in
 * `chat-run-watch.ts` separates "not yet" from "never will be (ZDR)" from
 * "never will be (no capture)" from "existed but could not be fetched", and
 * each renders its own honest copy plus whatever metadata IS present.
 *
 * When the run used a terminal or a file and VITE_CHAT_COMPUTER_PANEL is on, a tab strip (ComputerPanelTabs) sits above
 * the browser region: Browser, Terminal and Files, each drawn from the run's own tool calls. Never on a zero-retention turn.
 */

import { For, Show, createEffect, createMemo, createSignal, untrack } from 'solid-js'
import type { JSX } from '@solidjs/web'
import {
  CameraOff,
  CircleAlert,
  EyeOff,
  ImageOff,
  Loader2,
  MonitorPlay,
  Pause,
  PanelRightClose,
  PanelRightOpen,
  Play,
} from '@/shared/icons'
import {
  BrowserChrome,
} from '@/features/dashboard/home/BrowserChrome'
import { approvalKindLabel } from '@/shared/approvals/approval-copy'
import { listRunEventReplay, streamRunEvents } from '@/shared/api/run-console-client'
import { controlBrowserAiRun, type BrowserAiRunControlAction } from '@/shared/api/browser-run-client'
import type { BrowserControlMode } from '@/shared/api/browser-client'
import { STALE_CONTROL_COPY, changeBrowserControl } from '@/features/browser/dock/browser-control'
import type {
  BrowserActionApprovalRequiredEvent,
  RunPausedForApprovalEvent,
} from '@/shared/api/run-console-client'
import { getRun } from '@/shared/api/runs-client'
import { teachSkillFromRun } from '@/shared/api/learning-client'
import { ApiError } from '@/shared/api/http'
import {
  CHAT_RUN_SCREENSHOT_NOTES,
  applyBrowserAction,
  applyBrowserObservation,
  applyDurableRunEvent,
  appendActivity,
  applyBrowserControl,
  browserSessionFromChatRun,
  chatRunHostname,
  closeChatRunWatch,
  emptyChatRunWatch,
  hasBrowserFrames,
  latestBrowserStep,
  markScreenshotFailed,
  screenshotStateFor,
  type ChatRunBrowserControl,
  type ChatRunBrowserStep,
  type ChatRunScreenshotState,
  type ChatRunWatchState,
} from '@/features/chat/lib/chat-run-watch'
import { useI18n, translateApiError } from '@/shared/i18n'
import { Button } from '@/shared/ui/Button'
import { VerevonField } from '@/shared/ui/verevon/VerevonField'
import { VerevonInput } from '@/shared/ui/verevon/VerevonInput'
import { VerevonTextarea } from '@/shared/ui/verevon/VerevonTextarea'
import { isSafeLinkHref } from '../lib/safe-href'
import { RunNotifyButton } from './RunNotifyButton'
import { RunCostMeter } from './RunCostMeter'
import type { ChatToolCall, RunMeter } from './chat-types'
import { ComputerPanelTabs, computerPicksOf, type ComputerPicks, type ComputerTab } from './computer/ComputerPanelTabs'
import { chatComputerPanelEnabled } from './computer/ComputerViewer'

// The run stores its error as this Norwegian text; the reader's language is chosen where it is shown.
const RUN_STREAM_ERROR = 'Kunne ikke lese hendelsesstrømmen for denne kjøringen.'
const RUN_STREAM_ERROR_EN = 'The event stream for this run could not be read.'

/** A run that cannot emit more work. Used when opening a chat on an older
 * completed turn: the run-event endpoint is a live tail when no cursor is
 * supplied, so it intentionally stays open for active runs and has no reason
 * to close itself for a terminal run. Read the durable run status first to
 * avoid presenting a finished run as perpetually "Kjører" after reload. */
const TERMINAL_RUN_STATUSES = new Set([
  'completed',
  'failed',
  'cancelled',
  'canceled',
  'stopped',
  'rejected',
  'expired',
])

/**
 * Take over and Give back of the chat Do run's browser (EP-1 S1). Off by default: it needs Quarry
 * to accept the person as the run's owner of record (Q-1) and the turn's browser binding (WS21-S6).
 */
export const chatBrowserTakeoverEnabled = () => import.meta.env.VITE_CHAT_BROWSER_TAKEOVER === 'true'

/**
 * The longest note the person may hand back with the browser (EP-1 S3, D6). The note is the
 * person's own words to the agent; execution-core holds the model to the same cap.
 */
export const GIVE_BACK_NOTE_MAX_CHARS = 500

/**
 * Teach mode (F-t3-081): "Save these steps as a skill" after a give-back in a run the person took over. Off by
 * default. The server keeps its own switch too (`MODEL_GATEWAY_TEACH_MODE_ENABLED`), and refuses a zero-retention turn.
 */
export const teachModeEnabled = () => import.meta.env.VITE_TEACH_MODE === 'true'

/** The longest hint the person may give a taught skill's draft, as the teach route bounds it. */
const TEACH_HINT_MAX_CHARS = 500

/** The copy for the typed refusals a teach draft names (F-t3-081). Any other refusal gets the generic copy. */
const TEACH_REFUSAL_COPY: Readonly<Record<string, { no: string; en: string }>> = {
  teach_name_taken: {
    no: 'Det finnes allerede en ferdighet med dette navnet.',
    en: 'A skill with this name already exists.',
  },
  teach_name_invalid: {
    no: 'Navnet må bestå av små bokstaver og tall, med bindestrek mellom ordene, og være høyst 64 tegn.',
    en: 'The name must be lowercase letters and digits with hyphens between words, at most 64 characters.',
  },
  teach_run_not_teachable: {
    no: 'Denne kjøringen kan ikke brukes til å lage en ferdighet.',
    en: 'This run cannot be turned into a skill.',
  },
  teach_no_steps: {
    no: 'Kjøringen har ingen steg å lære av.',
    en: 'The run has no steps to learn from.',
  },
}

/** What a failed save says: the zero-retention refusal, a named refusal, or the generic copy. Never the gateway's text. */
function teachFailureMessage(failure: unknown, tr: (noText: string, enText: string) => string): string {
  if (failure instanceof ApiError && failure.feature === 'teach_mode') {
    return tr('Ferdigheter kan ikke lagres med null datalagring.', 'A skill cannot be saved with zero data retention.')
  }
  const copy = failure instanceof ApiError && failure.code ? TEACH_REFUSAL_COPY[failure.code] : undefined
  if (copy) return tr(copy.no, copy.en)
  return translateApiError(failure, tr, {
    no: 'Forslaget kunne ikke lagres. Prøv igjen.',
    en: 'The proposal could not be saved. Try again.',
  })
}

function isTerminalRunStatus(status?: string): boolean {
  return TERMINAL_RUN_STATUSES.has((status ?? '').trim().toLowerCase())
}

function screenshotNoteIcon(state: Exclude<ChatRunScreenshotState, 'ready'>) {
  if (state === 'pending') return <Loader2 size={13} class="verevon-run-spin" />
  if (state === 'withheld') return <EyeOff size={13} />
  if (state === 'failed') return <ImageOff size={13} />
  return <CameraOff size={13} />
}

function stepLabel(step: ChatRunBrowserStep, untitled: string): string {
  const host = chatRunHostname(step.url)
  const action = step.actionType ?? untitled
  return host ? `${action} · ${host}` : action
}

/**
 * What to show when the run produced no loadable frame. Every branch states a
 * reason and surfaces the metadata that does exist — never an empty frame and
 * never an unresolvable spinner.
 */
function ChatRunEvidenceFallback(props: {
  error: string | null
  live: boolean
  onReconnect: () => void
  steps: ChatRunBrowserStep[]
  zdr: boolean
}) {
  const i18n = useI18n()
  const latest = () => (props.steps.length > 0 ? props.steps[props.steps.length - 1] ?? null : null)
  const note = () => {
    const step = latest()
    if (!step) return null
    const state = screenshotStateFor(step, { live: props.live, zdr: props.zdr })
    return state === 'ready' ? null : state
  }

  return (
    <div class="verevon-chat-run-fallback">
      <Show when={props.error}>
        {(message) => (
          <div class="verevon-chat-run-fallback__error" role="alert">
            <p><CircleAlert size={14} /> {message() === RUN_STREAM_ERROR ? i18n.tr(RUN_STREAM_ERROR, RUN_STREAM_ERROR_EN) : message()}</p>
            <button type="button" onClick={props.onReconnect}>{i18n.tr('Koble til igjen', 'Reconnect')}</button>
          </div>
        )}
      </Show>

      <Show
        when={latest()}
        fallback={(
          <div class="verevon-chat-run-fallback__idle">
            <Show
              when={props.live}
              fallback={(
                <>
                  <CameraOff size={18} />
                  <strong>{i18n.tr('Ingen nettleseraktivitet', 'No browser activity')}</strong>
                  <p>{i18n.tr('Denne kjøringen brukte ikke nettleseren, så det finnes ingen skjermbilder.', 'This run did not use the browser, so there are no screenshots.')}</p>
                </>
              )}
            >
              <Loader2 size={18} class="verevon-run-spin" />
              <strong>{i18n.tr('Venter på agenten', 'Waiting for the agent')}</strong>
              <p>{i18n.tr('Agenten har ikke åpnet nettleseren ennå. Steg vises her så snart de skjer.', 'The agent has not opened the browser yet. Steps appear here as they happen.')}</p>
            </Show>
          </div>
        )}
      >
        {(step) => (
          <div class="verevon-chat-run-fallback__meta">
            <Show when={note()}>
              {(state) => (
                <p class="verevon-chat-run-fallback__note">
                  {screenshotNoteIcon(state())} {CHAT_RUN_SCREENSHOT_NOTES[state()]}
                </p>
              )}
            </Show>
            <dl>
              <div>
                <dt>Side</dt>
                <dd>{step().pageTitle || chatRunHostname(step().url) || 'Ukjent side'}</dd>
              </div>
              <Show when={step().url}>
                {(url) => (
                  <div>
                    <dt>URL</dt>
                    <dd>
                      {isSafeLinkHref(url())
                        ? <a href={url()} target="_blank" rel="noopener noreferrer">{url()}</a>
                        : <span class="verevon-chat-link-blocked" title={i18n.tr('Lenke blokkert: ukjent adresse', 'Link blocked: unsupported address')}>{url()}</span>}
                    </dd>
                  </div>
                )}
              </Show>
              <Show when={step().status}>
                {(status) => (
                  <div>
                    <dt>Status</dt>
                    <dd>{status()}</dd>
                  </div>
                )}
              </Show>
            </dl>
          </div>
        )}
      </Show>
    </div>
  )
}

/**
 * Teach mode (F-t3-081): saves the steps the person took in this run as a skill proposal. The panel mounts it only
 * after a give-back in a run they took over (see `teachRunId`), so a new run starts with an empty form. The proposal
 * waits in the learning inbox and is used only once the owner accepts it there.
 */
function SaveRunAsSkill(props: { runId: string }) {
  const i18n = useI18n()
  const [open, setOpen] = createSignal(false)
  const [name, setName] = createSignal('')
  const [hint, setHint] = createSignal('')
  const [pending, setPending] = createSignal(false)
  const [failure, setFailure] = createSignal<string | null>(null)
  const [saved, setSaved] = createSignal(false)

  const save = async (event: SubmitEvent) => {
    event.preventDefault()
    const skillName = name().trim()
    if (!skillName || pending()) return
    setPending(true)
    setFailure(null)
    try {
      await teachSkillFromRun(props.runId, { name: skillName, goalHint: hint().trim() })
      setSaved(true)
      setOpen(false)
    } catch (error) {
      setFailure(teachFailureMessage(error, i18n.tr))
    } finally {
      setPending(false)
    }
  }

  return (
    <section class="verevon-chat-run-teach" aria-label={i18n.tr('Lagre som ferdighet', 'Save as a skill')}>
      <Show
        when={!saved()}
        fallback={(
          <p class="verevon-chat-run-teach__done" role="status">
            {i18n.tr('Forslag opprettet — gjennomgå det i', 'Proposal created — review it in the')}{' '}
            <a href="/settings/learning" link>{i18n.tr('læringsinnboksen', 'learning inbox')}</a>.
          </p>
        )}
      >
        <Show
          when={open()}
          fallback={(
            <Button variant="secondary" size="sm" onClick={() => setOpen(true)}>
              {i18n.tr('Lagre disse stegene som en ferdighet', 'Save these steps as a skill')}
            </Button>
          )}
        >
          <form class="verevon-chat-run-teach__form" onSubmit={(event) => void save(event)}>
            <VerevonField label={i18n.tr('Navn på ferdigheten', 'Skill name')}>
              <VerevonInput
                type="text"
                required
                maxlength={64}
                pattern="[a-z0-9]+(-[a-z0-9]+)*"
                title={i18n.tr(
                  'Små bokstaver og tall, med bindestrek mellom ordene',
                  'Lowercase letters and digits, with hyphens between words',
                )}
                value={name()}
                disabled={pending()}
                onInput={(event) => setName(event.currentTarget.value)}
              />
            </VerevonField>
            <VerevonField label={i18n.tr('Hint', 'Hint')} optionalLabel={i18n.tr('valgfritt', 'optional')}>
              <VerevonTextarea
                rows={2}
                maxlength={TEACH_HINT_MAX_CHARS}
                value={hint()}
                disabled={pending()}
                onInput={(event) => setHint(event.currentTarget.value)}
              />
            </VerevonField>
            <Show when={failure()}>
              {(message) => <p class="verevon-chat-run-panel__control-error" role="alert">{message()}</p>}
            </Show>
            <div class="verevon-chat-run-teach__actions">
              <Button type="submit" variant="primary" size="sm" disabled={pending() || name().trim().length === 0}>
                {i18n.tr('Send forslaget', 'Send the proposal')}
              </Button>
              <Button variant="ghost" size="sm" disabled={pending()} onClick={() => setOpen(false)}>
                {i18n.tr('Avbryt', 'Cancel')}
              </Button>
            </div>
          </form>
        </Show>
      </Show>
    </section>
  )
}

export function ChatLiveRunPanel(props: {
  collapsed: boolean
  /** Verified organization used by the existing browser control proxy. */
  orgId?: string
  /** Keep the watcher mounted while another contextual canvas owns the rail. */
  hidden?: boolean
  onToggleCollapsed: () => void
  /** Orchestration run id for the turn being watched; null hides the panel. */
  runId: string | null
  /** Zero Data Retention turn — decides whether a missing shot is "never". */
  zdr: boolean
  /** Optional Work-tab projection rendered in this same run-owned canvas. */
  workContent?: JSX.Element
  /**
   * Wide-research fan-out cards (chat-wave2-specs). Hosted INSIDE this rail so
   * they take the rail's width instead of becoming a third flex column beside
   * the transcript, and kept mounted — merely hidden — while the rail is
   * collapsed: the cards are built from live events, which a remount would not
   * replay. Read exactly once below, so the caller should hand over a memoised
   * element (as `workContent` is) or every read would build a fresh tree.
   */
  wideResearch?: JSX.Element
  /** Local workspace tabs shown when Work owns the contextual canvas. */
  navigation?: JSX.Element
  /**
   * The browser dock's layout picker (WS9.G). Shown in the header while the panel is open and the
   * Work tab does not own the rail. Absent while the dock is off, so the header is unchanged.
   */
  dockControls?: JSX.Element
  /** Close Work without collapsing the background run watcher. */
  onCloseWork?: () => void
  /**
   * Ask the owning chat turn to re-read approvals from the orchestration
   * authority when a durable run enters or leaves an approval gate. The live
   * panel only observes the run stream; it never manufactures approval rows.
   */
  onRefreshApprovals?: (runId: string) => void
  /** The watched turn's running cost and usage (WS27-S4). Shown here while no Work panel is open. */
  meter?: RunMeter
  /**
   * The watched turn's tool calls (EP-1 W4-P4). The computer panel's Terminal and Files tabs draw from them, behind
   * `VITE_CHAT_COMPUTER_PANEL`; a zero-retention turn never shows them.
   */
  computerCalls?: ChatToolCall[]
  /**
   * The Space the watched run's processes live in (EP-1 W4-P4). The Terminal tab's live tail reads through it; without
   * it the tab shows the transcript only.
   */
  spaceRef?: string
  /**
   * The Quarry run the turn's browser runs on and who holds it (EP-1 S1), from the owner-only
   * `browser_session_opened` binding (WS21-S6). Absent until the turn is bound; read only while
   * `VITE_CHAT_BROWSER_TAKEOVER` is on.
   */
  browserBinding?: ChatRunBrowserControl
  /**
   * The person gave the browser back (EP-1 S1). The owning chat turn decides the open
   * `browser_human_assist` approval and continues the run; the panel never decides approvals.
   * `note` is the person's optional hand-back note (EP-1 S3), trimmed and capped, empty when they
   * wrote none: the turn sends it as the approval's decision reason, the one way it reaches the
   * model. Nothing typed into the page is ever sent.
   */
  onBrowserGiveBack?: (runId: string, note: string) => void
}) {
  const i18n = useI18n()
  // One name for two attributes: the button carries the same text as its
  // accessible name and its tooltip, and they must not drift apart.
  const toggleLabel = () => props.workContent
    ? i18n.tr('Lukk arbeidsflaten', 'Close the workspace')
    : props.collapsed
      ? i18n.tr('Vis live-panelet', 'Show the live panel')
      : i18n.tr('Skjul live-panelet', 'Hide the live panel')
  const [watch, setWatch] = createSignal<ChatRunWatchState | null>(null)
  const [expandedStep, setExpandedStep] = createSignal<number | null>(null)
  const [reconnectVersion, setReconnectVersion] = createSignal(0)
  const [controlPending, setControlPending] = createSignal<BrowserAiRunControlAction | null>(null)
  const [controlError, setControlError] = createSignal<string | null>(null)
  let activeStreamGeneration = 0

  // Guarded so late callbacks from an aborted stream can never write into the
  // state of the run that replaced it.
  const update = (
    runId: string,
    apply: (state: ChatRunWatchState) => ChatRunWatchState,
    generation = activeStreamGeneration,
  ) => {
    setWatch((current) => (
      current && current.runId === runId && generation === activeStreamGeneration
        ? apply(current)
        : current
    ))
  }

  createEffect(
    () => {
      // Reading this signal makes the effect restart only when the user asks
      // for a reconnect, while keeping the already-reduced run state.
      return {
        existing: untrack(() => watch()),
        reconnect: reconnectVersion(),
        runId: props.runId,
        zdr: props.zdr,
      }
    },
    ({ existing, runId, zdr }) => {
      const generation = ++activeStreamGeneration
      setExpandedStep(null)
      if (!runId) {
        setWatch(null)
        return
      }
      const resuming = existing?.runId === runId
      if (resuming && existing) {
        setWatch({ ...existing, error: null, live: true, zdr })
      } else {
        setWatch(emptyChatRunWatch(runId, zdr))
      }
      const resumeFrom = resuming ? existing?.lastEventId ?? undefined : undefined
      const controller = new AbortController()
      const connect = async () => {
        // A fresh mount has no Last-Event-ID, and the live run stream
        // intentionally starts at the tail in that case. Rehydrate the
        // allowlisted browser projection first, then resume the live tail from
        // the canonical cursor returned by the replay page. This closes the
        // reload gap without opening a second transport or duplicating event
        // authority in the browser.
        let durableCursor = resumeFrom
        if (!resuming && !zdr) {
          const pageLimit = 500
          const maxPages = 10
          for (let pageNumber = 0; pageNumber < maxPages; pageNumber += 1) {
            if (controller.signal.aborted || generation !== activeStreamGeneration) return
            const page = await listRunEventReplay(runId, {
              afterEventId: durableCursor,
              limit: pageLimit,
            }).catch(() => null)
            if (!page || controller.signal.aborted || generation !== activeStreamGeneration) break
            for (const event of page.events) {
              update(runId, (state) => applyDurableRunEvent(state, event), generation)
            }
            const nextCursor = page.nextEventId?.trim()
            if (!page.truncated || !nextCursor || nextCursor === durableCursor) break
            durableCursor = nextCursor
          }
          if (durableCursor) {
            update(runId, (state) => ({ ...state, lastEventId: durableCursor ?? null }), generation)
          }
        }
        const refreshApprovals = (candidateRunId?: string) => {
          if (!candidateRunId || generation !== activeStreamGeneration || controller.signal.aborted) return
          props.onRefreshApprovals?.(candidateRunId)
        }
        // Start the live tail immediately so an active run can deliver events
        // without waiting on the read model. A fresh page has no
        // Last-Event-ID cursor, so the durable status check runs in parallel
        // and closes the tail only when it proves the run is terminal.
        void getRun(runId, controller.signal).then((detail) => {
          if (
            detail &&
            isTerminalRunStatus(detail.status) &&
            generation === activeStreamGeneration &&
            !controller.signal.aborted
          ) {
            update(runId, (state) => closeChatRunWatch(state), generation)
            controller.abort()
          }
        }).catch(() => {
          // The stream remains the source of live events when the read model
          // is temporarily unavailable.
        })
        if (controller.signal.aborted || generation !== activeStreamGeneration) return
        await streamRunEvents(runId, {
        onFrameId: (id) => update(runId, (state) => ({ ...state, lastEventId: id }), generation),
        onApproval: (event) => {
          update(runId, (state) => appendActivity(state, {
            at: event.at ?? new Date().toISOString(),
            detail: event.approvalKind ? approvalKindLabel(event.approvalKind, i18n) : '',
            id: `approval-${event.approvalId ?? state.activity.length}`,
            kind: 'approval',
            status: event.to,
            title: i18n.tr('Godkjenning', 'Approval'),
          }))
          refreshApprovals(event.runId)
        },
        onRunPaused: (event: RunPausedForApprovalEvent) => {
          update(runId, (state) => appendActivity(state, {
            at: event.at ?? new Date().toISOString(),
            detail: event.approvalId ? `Venter på godkjenning · ${event.approvalId}` : 'Venter på godkjenning.',
            id: `run-paused-${event.approvalId ?? state.activity.length}`,
            kind: 'pause',
            status: 'paused',
            title: 'Pauset for godkjenning',
          }))
          refreshApprovals(event.runId)
        },
        onRunResumed: (event) => {
          update(runId, (state) => appendActivity(state, {
            at: event.at ?? new Date().toISOString(),
            detail: 'Kjøringen fortsetter etter godkjenning.',
            id: `run-resumed-${event.approvalId ?? state.activity.length}`,
            kind: 'resume',
            status: 'resumed',
            title: 'Fortsatte',
          }))
          refreshApprovals(event.runId)
        },
        onBrowserActionApprovalRequired: (event: BrowserActionApprovalRequiredEvent) => {
          update(runId, (state) => appendActivity(state, {
            at: event.at ?? new Date().toISOString(),
            detail: [event.actionType, event.reason].filter(Boolean).join(' · ') || 'En nettleserhandling venter på godkjenning.',
            id: `browser-approval-${event.approvalId ?? event.actionId ?? state.activity.length}`,
            kind: 'approval',
            status: 'pending',
            title: 'Nettleserhandling krever godkjenning',
          }))
          refreshApprovals(event.runId)
        },
        onBrowserActionDecided: (event) => {
          update(runId, (state) => appendActivity(state, {
            at: event.at ?? new Date().toISOString(),
            detail: event.decision ? `Beslutning: ${event.decision}.` : 'Godkjenningsbeslutning mottatt.',
            id: `browser-decision-${event.approvalId ?? event.actionId ?? state.activity.length}`,
            kind: 'approval',
            status: event.decision,
            title: 'Godkjenning behandlet',
          }))
          refreshApprovals(event.runId)
        },
        onBrowserAction: (event) => update(runId, (state) => applyBrowserAction(state, event)),
        onBrowserObservation: (event) => update(runId, (state) => applyBrowserObservation(state, event)),
        onBrowserRunPaused: (event) => update(runId, (state) => appendActivity({ ...state, controlState: 'paused' }, {
          at: event.at ?? new Date().toISOString(),
          detail: 'Nettleserkjøringen ble satt på pause.',
          id: `paused-${state.activity.length}`,
          kind: 'pause',
          title: 'Pauset',
        })),
        onBrowserRunResumed: (event) => update(runId, (state) => appendActivity({ ...state, controlState: 'running' }, {
          at: event.at ?? new Date().toISOString(),
          detail: 'Nettleserkjøringen fortsatte.',
          id: `resumed-${state.activity.length}`,
          kind: 'resume',
          title: 'Fortsatte',
        })),
        onDone: () => update(runId, (state) => closeChatRunWatch(state), generation),
        onError: () => update(runId, (state) => closeChatRunWatch(state, RUN_STREAM_ERROR), generation),
        onPlan: (event) => update(runId, (state) => appendActivity(state, {
          at: event.at ?? new Date().toISOString(),
          detail: [event.from, event.to].filter(Boolean).join(' → '),
          id: `plan-${event.planId ?? state.activity.length}`,
          kind: 'plan',
          status: event.to,
          title: 'Plan',
        })),
        onStep: (event) => update(runId, (state) => appendActivity(state, {
          at: new Date().toISOString(),
          detail: event.detail ?? '',
          id: `step-${event.id ?? state.activity.length}`,
          kind: 'step',
          status: event.status,
          title: event.title ?? 'Steg',
        })),
        onSubagentAttached: (event) => update(runId, (state) => appendActivity(state, {
          at: event.at ?? new Date().toISOString(),
          detail: event.role ?? '',
          id: `subagent-${event.childRunId ?? state.activity.length}`,
          kind: 'subagent',
          title: 'Underagent',
        })),
        }, controller.signal, durableCursor)
      }
      void connect()
      return () => controller.abort()
    },
  )

  // The turn's browser binding (EP-1 S1). Applied to the watched run only; a zero-retention turn
  // ignores it (`applyBrowserControl`).
  createEffect(
    () => ({ binding: props.browserBinding, runId: props.runId }),
    ({ binding, runId }) => {
      if (!binding || !runId) return
      update(runId, (state) => applyBrowserControl(state, binding))
    },
  )

  const [takeoverPending, setTakeoverPending] = createSignal(false)
  /** The person's hand-back note (EP-1 S3), kept until the give-back is accepted. */
  const [giveBackNote, setGiveBackNote] = createSignal('')
  /** The runs the person took the browser over in, and gave it back in, as Quarry confirmed them (F-t3-081 teach mode). */
  const [takenOverRun, setTakenOverRun] = createSignal<string | null>(null)
  const [givenBackRun, setGivenBackRun] = createSignal<string | null>(null)
  /** Take over and Give back are offered only with the switch on, for a non-ZDR turn, in an org. */
  const takeover = () => chatBrowserTakeoverEnabled() && !zdr() && Boolean(props.orgId?.trim())
  /** The note field shows only while the person holds the turn's browser. */
  const personHolds = () => takeover() && watch()?.browserControl?.controlMode === 'human_takeover'
  /**
   * Flips who holds the turn's browser through the existing BFF control route, compare-and-set on
   * the epoch the person saw. Quarry answers with the new control, which the panel shows; a stale
   * epoch shows what Quarry now says and changes nothing. After a give-back the owning turn spends
   * the takeover approval: the executor re-checks control before it acts, so the order is safe.
   */
  const changeTakeover = async (mode: BrowserControlMode, expectedEpoch?: number | null) => {
    const orgId = props.orgId?.trim()
    const currentRunId = props.runId?.trim()
    const sessionId = watch()?.browserControl?.sessionId
    if (!orgId || !currentRunId || !sessionId || takeoverPending()) return
    setTakeoverPending(true)
    setControlError(null)
    try {
      const outcome = await changeBrowserControl(orgId, sessionId, mode, expectedEpoch)
      const control = outcome.response.session.control
      if (control) {
        update(currentRunId, (state) => applyBrowserControl(state, {
          controlEpoch: control.epoch ?? null,
          controlMode: control.mode,
          sessionId,
        }))
      }
      if (outcome.stale) {
        setControlError(i18n.tr(STALE_CONTROL_COPY.no, STALE_CONTROL_COPY.en))
        return
      }
      if (mode === 'human_takeover' && control?.mode === 'human_takeover') {
        setTakenOverRun(currentRunId)
      }
      if (mode === 'agent_control') {
        setGivenBackRun(currentRunId)
        const note = giveBackNote().trim().slice(0, GIVE_BACK_NOTE_MAX_CHARS)
        setGiveBackNote('')
        props.onBrowserGiveBack?.(currentRunId, note)
      }
    } catch (error) {
      setControlError(translateApiError(error, i18n.tr, { no: 'Styringen av nettleseren kunne ikke endres.', en: 'Browser control could not be changed.' }))
    } finally {
      setTakeoverPending(false)
    }
  }

  const browserControl = async (action: BrowserAiRunControlAction) => {
    const orgId = props.orgId?.trim()
    const currentRunId = props.runId?.trim()
    if (!orgId || !currentRunId || controlPending()) return
    setControlPending(action)
    setControlError(null)
    try {
      // The proxy is the authority for browser pause/resume. We keep the
      // local state unchanged until its durable run event arrives, so a lost
      // response cannot make the UI claim that a pause took effect.
      await controlBrowserAiRun(orgId, currentRunId, action)
    } catch (error) {
      setControlError(translateApiError(error, i18n.tr, { no: 'Nettleserkontrollen kunne ikke sendes.', en: 'The browser control could not be sent.' }))
    } finally {
      setControlPending(null)
    }
  }

  // The computer panel's tabs (EP-1 W4-P4, F-t3-035). Off unless VITE_CHAT_COMPUTER_PANEL is on, and never on a
  // zero-retention turn. A run that used neither a terminal nor a file keeps the panel it always had: no tab strip.
  const [computerTab, setComputerTab] = createSignal<ComputerTab>('browser')
  const computerPicks = createMemo<ComputerPicks>(() => (
    chatComputerPanelEnabled() && !props.zdr ? computerPicksOf(props.computerCalls ?? []) : {}
  ))
  const computerTabsShown = createMemo(() => computerPicks().terminal !== undefined || computerPicks().file !== undefined)
  /** The tab on screen: the chosen one while this run still has its kind, otherwise Browser. */
  const activeComputerTab = createMemo<ComputerTab>(() => {
    const tab = computerTab()
    const picks = computerPicks()
    if (tab === 'terminal' && picks.terminal) return 'terminal'
    if (tab === 'files' && picks.file) return 'files'
    return 'browser'
  })
  const browserTabShown = () => activeComputerTab() === 'browser'
  const steps = () => watch()?.steps ?? []
  const activity = () => watch()?.activity ?? []
  const live = () => watch()?.live ?? false
  const zdr = () => watch()?.zdr ?? props.zdr
  const framed = createMemo(() => {
    const current = watch()
    return current ? hasBrowserFrames(current) : false
  })
  const session = createMemo(() => {
    const current = watch()
    return current ? browserSessionFromChatRun(current, { takeover: takeover() }) : null
  })
  const statusLabel = () => {
    const current = watch()
    if (!current) return i18n.tr('Kobler til …', 'Connecting…')
    if (current.error) return i18n.tr('Avbrutt', 'Interrupted')
    if (!current.live) return i18n.tr('Fullført', 'Completed')
    const step = latestBrowserStep(current)
    return step ? i18n.tr(`Steg ${step.step}`, `Step ${step.step}`) : i18n.tr('Kjører', 'Running')
  }
  const shotState = (step: ChatRunBrowserStep) => screenshotStateFor(step, { live: live(), zdr: zdr() })
  const expanded = () => {
    const selected = expandedStep()
    if (selected === null) return null
    return steps().find((step) => step.step === selected) ?? null
  }
  /**
   * The watched run, when "Save these steps as a skill" is offered (F-t3-081): teach mode is on, the person took the
   * browser over in this run and has given it back, and the turn keeps content. A zero-retention or temporary turn
   * never offers it, and a new run offers it only after its own takeover and give-back.
   */
  const teachRunId = (): string | null => {
    const current = props.runId?.trim()
    if (!current || !teachModeEnabled() || zdr()) return null
    return takenOverRun() === current && givenBackRun() === current ? current : null
  }

  return (
    <Show when={props.runId}>
      {(runId) => (
        <aside
          class={[
            'verevon-chat-run-panel',
            {
              'verevon-chat-run-panel--collapsed': props.collapsed,
              // Hosting the Work tab means this panel IS the workspace rail, so it
              // must not keep the narrow basis meant for sitting beside the canvas.
              'verevon-chat-run-panel--workspace': Boolean(props.workContent),
            },
          ]}
          aria-label={props.workContent
            ? i18n.tr('Arbeidsflate', 'Workspace')
            : i18n.tr('Live agentkjøring', 'Live agent run')}
          style={{ display: props.hidden ? 'none' : undefined }}
        >
          <header class="verevon-chat-run-panel__head">
            <button
              type="button"
              class="verevon-chat-run-panel__toggle"
              aria-expanded={!props.collapsed ? 'true' : 'false'}
              aria-label={toggleLabel()}
              title={toggleLabel()}
              onClick={() => props.workContent ? props.onCloseWork?.() : props.onToggleCollapsed()}
            >
              <Show when={props.collapsed} fallback={<PanelRightClose size={15} />}>
                <PanelRightOpen size={15} />
              </Show>
            </button>
            <Show when={!props.collapsed}>
              <Show
                when={props.workContent}
                fallback={(
                  <>
                    <div class="verevon-chat-run-panel__title">
                      <strong><MonitorPlay size={13} /> Live agent</strong>
                      <span>{statusLabel()}</span>
                    </div>
                    <code class="verevon-chat-run-panel__runid" title={runId()}>{runId()}</code>
                    {props.dockControls}
                  </>
                )}
              >
                <div class="verevon-chat-run-panel__workspace-navigation">{props.navigation}</div>
                <span class="verevon-chat-run-panel__workspace-status">{statusLabel()}</span>
              </Show>
              {/* "Varsle meg når ferdig" while the run is live. Gone once it has
                  finished, and for a ZDR run, whose events are suppressed. */}
              <Show when={live() && !zdr() ? runId() : undefined}>
                {(watchedRunId) => <RunNotifyButton runId={watchedRunId()} compact />}
              </Show>
              <Show when={props.orgId && framed() && live()}>
                <div class="verevon-chat-run-panel__controls" role="group" aria-label={i18n.tr('Nettleserkjøring', 'Browser run')}>
                  <Show
                    when={watch()?.controlState === 'paused'}
                    fallback={(
                      <button
                        type="button"
                        class="verevon-chat-run-panel__control"
                        disabled={controlPending() !== null}
                        aria-label={i18n.tr('Sett nettleserkjøring på pause', 'Pause the browser run')}
                        title={i18n.tr('Pause etter gjeldende steg', 'Pause after the current step')}
                        onClick={() => void browserControl('pause')}
                      >
                        <Show when={controlPending() === 'pause'} fallback={<Pause size={12} />}>
                          <Loader2 size={12} class="verevon-run-spin" />
                        </Show>
                      </button>
                    )}
                  >
                    <button
                      type="button"
                      class="verevon-chat-run-panel__control"
                      disabled={controlPending() !== null}
                      aria-label={i18n.tr('Fortsett nettleserkjøring', 'Resume the browser run')}
                      title={i18n.tr('Fortsett etter pause', 'Resume after the pause')}
                      onClick={() => void browserControl('resume')}
                    >
                      <Show when={controlPending() === 'resume'} fallback={<Play size={12} />}>
                        <Loader2 size={12} class="verevon-run-spin" />
                      </Show>
                    </button>
                  </Show>
                </div>
              </Show>
            </Show>
          </header>

          <div
            class="verevon-chat-run-panel__wide-research"
            style={{ display: props.collapsed ? 'none' : undefined }}
          >
            {props.wideResearch}
          </div>

          <Show when={!props.collapsed}>
            <div class="verevon-chat-run-panel__body">
              {/* The Work panel carries the meter itself, so the live rail shows it only while Work is closed. */}
              <Show
                when={props.workContent}
                fallback={<RunCostMeter meter={props.meter} quotaRefreshKey={props.runId ?? undefined} />}
              >
                <section
                  id="verevon-chat-tabpanel-steps"
                  class="verevon-chat-run-panel__work"
                  role="tabpanel"
                  aria-label={i18n.tr('Arbeidsdetaljer', 'Work details')}
                >
                  {props.workContent}
                </section>
              </Show>
              <Show when={controlError()}>
                {(message) => <p class="verevon-chat-run-panel__control-error" role="alert">{message()}</p>}
              </Show>
              {/* F-t3-081 teach mode: after a give-back in a run the person took over, save those steps as a skill. */}
              <Show when={teachRunId()}>
                {(taughtRunId) => <SaveRunAsSkill runId={taughtRunId()} />}
              </Show>
              {/* EP-1 W4-P4: the Terminal and Files tabs for the kinds this run used. Nothing mounts without one, or while the panel is hidden. */}
              <Show when={computerTabsShown() && !props.hidden}>
                <ComputerPanelTabs
                  picks={computerPicks()}
                  selected={activeComputerTab()}
                  onSelect={(tab) => setComputerTab(tab)}
                  runId={runId()}
                  spaceRef={props.spaceRef}
                />
              </Show>
              <Show
                when={browserTabShown() && framed() && session()}
                fallback={browserTabShown() ? (
                  <ChatRunEvidenceFallback
                    error={watch()?.error ?? null}
                    live={live()}
                    onReconnect={() => setReconnectVersion((version) => version + 1)}
                    steps={steps()}
                    zdr={zdr()}
                  />
                ) : undefined}
              >
                {(model) => (
                  <>
                    <BrowserChrome
                      compact
                      session={model()}
                      browserBusy={takeoverPending()}
                      onBrowserControlMode={takeover() && model().sessionId
                        ? (mode, expectedEpoch) => void changeTakeover(mode, expectedEpoch)
                        : undefined}
                    />
                    <Show when={personHolds() && model().sessionId}>
                      <label class="verevon-chat-run-panel__give-back-note">
                        <span>{i18n.tr('Beskjed til agenten når du gir tilbake (valgfritt)', 'Note to the agent when you give back (optional)')}</span>
                        <textarea
                          maxlength={GIVE_BACK_NOTE_MAX_CHARS}
                          rows={2}
                          value={giveBackNote()}
                          disabled={takeoverPending()}
                          onInput={(event) => setGiveBackNote(event.currentTarget.value)}
                        />
                      </label>
                    </Show>
                  </>
                )}
              </Show>

              <Show when={browserTabShown() && steps().length > 0}>
                <section class="verevon-chat-run-shots" aria-label={i18n.tr('Skjermbilder per steg', 'Screenshots per step')}>
                  <div class="verevon-chat-run-shots__rail">
                    <For each={steps()}>
                      {(step) => (
                        <button
                          type="button"
                          class={['verevon-chat-run-shot', { 'verevon-chat-run-shot--active': expandedStep() === step.step }]}
                          aria-pressed={expandedStep() === step.step ? 'true' : 'false'}
                          title={i18n.tr(`Steg ${step.step} · ${stepLabel(step, 'steg')}`, `Step ${step.step} - ${stepLabel(step, 'step')}`)}
                          onClick={() => setExpandedStep((current) => (current === step.step ? null : step.step))}
                        >
                          <span class="verevon-chat-run-shot__frame">
                            <Show
                              when={shotState(step) === 'ready' && step.screenshotUrl}
                              fallback={(
                                <span class="verevon-chat-run-shot__placeholder">
                                  {screenshotNoteIcon(shotState(step) as Exclude<ChatRunScreenshotState, 'ready'>)}
                                </span>
                              )}
                            >
                              {(src) => (
                                <img
                                  src={src()}
                                  alt={i18n.tr(`Skjermbilde fra steg ${step.step}`, `Screenshot from step ${step.step}`)}
                                  loading="lazy"
                                  decoding="async"
                                  onError={() => update(runId(), (state) => markScreenshotFailed(state, step.actionId))}
                                />
                              )}
                            </Show>
                          </span>
                          <span class="verevon-chat-run-shot__label">
                            <em>#{step.step}</em> {stepLabel(step, i18n.tr('steg', 'step'))}
                          </span>
                        </button>
                      )}
                    </For>
                  </div>

                  <Show when={expanded()}>
                    {(step) => (
                      <figure class="verevon-chat-run-shot-expanded">
                        <Show
                          when={shotState(step()) === 'ready' && step().screenshotUrl}
                          fallback={(
                            <figcaption class="verevon-chat-run-shot-expanded__note">
                              {screenshotNoteIcon(shotState(step()) as Exclude<ChatRunScreenshotState, 'ready'>)}
                              {' '}
                              {CHAT_RUN_SCREENSHOT_NOTES[shotState(step()) as Exclude<ChatRunScreenshotState, 'ready'>]}
                            </figcaption>
                          )}
                        >
                          {(src) => (
                            <img
                              src={src()}
                              alt={i18n.tr(`Skjermbilde fra steg ${step().step}`, `Screenshot from step ${step().step}`)}
                              decoding="async"
                              onError={() => update(runId(), (state) => markScreenshotFailed(state, step().actionId))}
                            />
                          )}
                        </Show>
                        <figcaption class="verevon-chat-run-shot-expanded__meta">
                          <strong>{i18n.tr('Steg', 'Step')} {step().step} · {step().actionType ?? i18n.tr('steg', 'step')}</strong>
                          <Show when={step().pageTitle}>{(title) => <span>{title()}</span>}</Show>
                          <Show when={step().url}>
                            {(url) => (
                              isSafeLinkHref(url())
                                ? <a href={url()} target="_blank" rel="noopener noreferrer">{url()}</a>
                                : <span class="verevon-chat-link-blocked" title={i18n.tr('Lenke blokkert: ukjent adresse', 'Link blocked: unsupported address')}>{url()}</span>
                            )}
                          </Show>
                          <Show when={step().reason}>{(reason) => <p>{reason()}</p>}</Show>
                        </figcaption>
                      </figure>
                    )}
                  </Show>
                </section>
              </Show>

              <Show when={activity().length > 0}>
                <details class="verevon-chat-run-activity-disclosure">
                  <summary>
                    <span>Teknisk aktivitet</span>
                    <em>{activity().length}</em>
                  </summary>
                  <section class="verevon-chat-run-activity" aria-label={i18n.tr('Kjøringshendelser', 'Run events')}>
                    <For each={activity().slice(-12)}>
                      {(entry) => (
                        <div class="verevon-chat-run-activity__row" data-status={entry.status ?? ''}>
                          <strong>{entry.title}</strong>
                          <Show when={entry.detail}><span>{entry.detail}</span></Show>
                        </div>
                      )}
                    </For>
                  </section>
                </details>
              </Show>
            </div>
          </Show>
        </aside>
      )}
    </Show>
  )
}
