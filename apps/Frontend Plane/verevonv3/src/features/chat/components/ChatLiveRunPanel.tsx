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
 */

import {
  For,
  Show,
  createEffect,
  createMemo,
  createSignal,
  untrack,
} from 'solid-js'
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
import { listRunEventReplay, streamRunEvents } from '@/shared/api/run-console-client'
import { controlBrowserAiRun, type BrowserAiRunControlAction } from '@/shared/api/browser-run-client'
import type {
  BrowserActionApprovalRequiredEvent,
  RunPausedForApprovalEvent,
} from '@/shared/api/run-console-client'
import { getRun } from '@/shared/api/runs-client'
import {
  CHAT_RUN_SCREENSHOT_NOTES,
  applyBrowserAction,
  applyBrowserObservation,
  applyDurableRunEvent,
  appendActivity,
  browserSessionFromChatRun,
  chatRunHostname,
  closeChatRunWatch,
  emptyChatRunWatch,
  hasBrowserFrames,
  latestBrowserStep,
  markScreenshotFailed,
  screenshotStateFor,
  type ChatRunBrowserStep,
  type ChatRunScreenshotState,
  type ChatRunWatchState,
} from '@/features/chat/lib/chat-run-watch'
import { useI18n } from '@/shared/i18n'

const RUN_STREAM_ERROR = 'Kunne ikke lese hendelsesstrømmen for denne kjøringen.'

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

function isTerminalRunStatus(status?: string): boolean {
  return TERMINAL_RUN_STATUSES.has((status ?? '').trim().toLowerCase())
}

function screenshotNoteIcon(state: Exclude<ChatRunScreenshotState, 'ready'>) {
  if (state === 'pending') return <Loader2 size={13} class="verevon-run-spin" />
  if (state === 'withheld') return <EyeOff size={13} />
  if (state === 'failed') return <ImageOff size={13} />
  return <CameraOff size={13} />
}

function stepLabel(step: ChatRunBrowserStep): string {
  const host = chatRunHostname(step.url)
  const action = step.actionType ?? 'steg'
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
            <p><CircleAlert size={14} /> {message()}</p>
            <button type="button" onClick={props.onReconnect}>Koble til igjen</button>
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
                      <a href={url()} target="_blank" rel="noopener noreferrer">{url()}</a>
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
  /** Local workspace tabs shown when Work owns the contextual canvas. */
  navigation?: JSX.Element
  /** Close Work without collapsing the background run watcher. */
  onCloseWork?: () => void
  /**
   * Ask the owning chat turn to re-read approvals from the orchestration
   * authority when a durable run enters or leaves an approval gate. The live
   * panel only observes the run stream; it never manufactures approval rows.
   */
  onRefreshApprovals?: (runId: string) => void
}) {
  const i18n = useI18n()
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
            detail: event.approvalKind ?? '',
            id: `approval-${event.approvalId ?? state.activity.length}`,
            kind: 'approval',
            status: event.to,
            title: 'Godkjenning',
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
      setControlError(error instanceof Error ? error.message : 'Nettleserkontrollen kunne ikke sendes.')
    } finally {
      setControlPending(null)
    }
  }

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
    return current ? browserSessionFromChatRun(current) : null
  })
  const statusLabel = () => {
    const current = watch()
    if (!current) return 'Kobler til …'
    if (current.error) return 'Avbrutt'
    if (!current.live) return 'Fullført'
    return latestBrowserStep(current) ? `Steg ${latestBrowserStep(current)?.step}` : 'Kjører'
  }
  const shotState = (step: ChatRunBrowserStep) => screenshotStateFor(step, { live: live(), zdr: zdr() })
  const expanded = () => {
    const selected = expandedStep()
    if (selected === null) return null
    return steps().find((step) => step.step === selected) ?? null
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
          aria-label={props.workContent ? 'Arbeidsflate' : 'Live agentkjøring'}
          style={{ display: props.hidden ? 'none' : undefined }}
        >
          <header class="verevon-chat-run-panel__head">
            <button
              type="button"
              class="verevon-chat-run-panel__toggle"
              aria-expanded={!props.collapsed ? 'true' : 'false'}
              aria-label={props.workContent ? 'Lukk arbeidsflaten' : props.collapsed ? 'Vis live-panelet' : 'Skjul live-panelet'}
              title={props.workContent ? 'Lukk arbeidsflaten' : props.collapsed ? 'Vis live-panelet' : 'Skjul live-panelet'}
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
                  </>
                )}
              >
                <div class="verevon-chat-run-panel__workspace-navigation">{props.navigation}</div>
                <span class="verevon-chat-run-panel__workspace-status">{statusLabel()}</span>
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

          <Show when={!props.collapsed}>
            <div class="verevon-chat-run-panel__body">
              <Show when={props.workContent}>
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
              <Show
                when={framed() && session()}
                fallback={(
                  <ChatRunEvidenceFallback
                    error={watch()?.error ?? null}
                    live={live()}
                    onReconnect={() => setReconnectVersion((version) => version + 1)}
                    steps={steps()}
                    zdr={zdr()}
                  />
                )}
              >
                {(model) => <BrowserChrome compact session={model()} />}
              </Show>

              <Show when={steps().length > 0}>
                <section class="verevon-chat-run-shots" aria-label={i18n.tr('Skjermbilder per steg', 'Screenshots per step')}>
                  <div class="verevon-chat-run-shots__rail">
                    <For each={steps()}>
                      {(step) => (
                        <button
                          type="button"
                          class={['verevon-chat-run-shot', { 'verevon-chat-run-shot--active': expandedStep() === step.step }]}
                          aria-pressed={expandedStep() === step.step ? 'true' : 'false'}
                          title={i18n.tr(`Steg ${step.step} · ${stepLabel(step)}`, `Step ${step.step} - ${stepLabel(step)}`)}
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
                            <em>#{step.step}</em> {stepLabel(step)}
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
                          <strong>Steg {step().step} · {step().actionType ?? 'steg'}</strong>
                          <Show when={step().pageTitle}>{(title) => <span>{title()}</span>}</Show>
                          <Show when={step().url}>
                            {(url) => (
                              <a href={url()} target="_blank" rel="noopener noreferrer">{url()}</a>
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
