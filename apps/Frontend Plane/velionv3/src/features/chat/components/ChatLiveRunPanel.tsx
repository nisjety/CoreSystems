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
  on,
  onCleanup,
} from 'solid-js'
import {
  CameraOff,
  CircleAlert,
  EyeOff,
  ImageOff,
  Loader2,
  MonitorPlay,
  PanelRightClose,
  PanelRightOpen,
} from 'lucide-solid'
import {
  BrowserChrome,
} from '@/features/dashboard/home/BrowserChrome'
import {
  streamRunEvents,
} from '@/shared/api/run-console-client'
import {
  CHAT_RUN_SCREENSHOT_NOTES,
  applyBrowserAction,
  applyBrowserObservation,
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

const RUN_STREAM_ERROR = 'Kunne ikke lese hendelsesstrømmen for denne kjøringen.'

function screenshotNoteIcon(state: Exclude<ChatRunScreenshotState, 'ready'>) {
  if (state === 'pending') return <Loader2 size={13} class="velion-run-spin" />
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
  steps: ChatRunBrowserStep[]
  zdr: boolean
}) {
  const latest = () => (props.steps.length > 0 ? props.steps[props.steps.length - 1] ?? null : null)
  const note = () => {
    const step = latest()
    if (!step) return null
    const state = screenshotStateFor(step, { live: props.live, zdr: props.zdr })
    return state === 'ready' ? null : state
  }

  return (
    <div class="velion-chat-run-fallback">
      <Show when={props.error}>
        {(message) => (
          <p class="velion-chat-run-fallback__error" role="alert">
            <CircleAlert size={14} /> {message()}
          </p>
        )}
      </Show>

      <Show
        when={latest()}
        fallback={(
          <div class="velion-chat-run-fallback__idle">
            <Show
              when={props.live}
              fallback={(
                <>
                  <CameraOff size={18} />
                  <strong>Ingen nettleseraktivitet</strong>
                  <p>Denne kjøringen brukte ikke nettleseren, så det finnes ingen skjermbilder.</p>
                </>
              )}
            >
              <Loader2 size={18} class="velion-run-spin" />
              <strong>Venter på agenten</strong>
              <p>Agenten har ikke åpnet nettleseren ennå. Steg vises her så snart de skjer.</p>
            </Show>
          </div>
        )}
      >
        {(step) => (
          <div class="velion-chat-run-fallback__meta">
            <Show when={note()}>
              {(state) => (
                <p class="velion-chat-run-fallback__note">
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
  onToggleCollapsed: () => void
  /** Orchestration run id for the turn being watched; null hides the panel. */
  runId: string | null
  /** Zero Data Retention turn — decides whether a missing shot is "never". */
  zdr: boolean
}) {
  const [watch, setWatch] = createSignal<ChatRunWatchState | null>(null)
  const [expandedStep, setExpandedStep] = createSignal<number | null>(null)

  // Guarded so late callbacks from an aborted stream can never write into the
  // state of the run that replaced it.
  const update = (runId: string, apply: (state: ChatRunWatchState) => ChatRunWatchState) => {
    setWatch((current) => (current && current.runId === runId ? apply(current) : current))
  }

  createEffect(on(() => props.runId, (runId) => {
    setExpandedStep(null)
    if (!runId) {
      setWatch(null)
      return
    }
    setWatch(emptyChatRunWatch(runId, props.zdr))
    const controller = new AbortController()
    void streamRunEvents(runId, {
      onApproval: (event) => update(runId, (state) => appendActivity(state, {
        at: event.at ?? new Date().toISOString(),
        detail: event.approvalKind ?? '',
        id: `approval-${event.approvalId ?? state.activity.length}`,
        kind: 'approval',
        status: event.to,
        title: 'Godkjenning',
      })),
      onBrowserAction: (event) => update(runId, (state) => applyBrowserAction(state, event)),
      onBrowserObservation: (event) => update(runId, (state) => applyBrowserObservation(state, event)),
      onBrowserRunPaused: (event) => update(runId, (state) => appendActivity(state, {
        at: event.at ?? new Date().toISOString(),
        detail: 'Nettleserkjøringen ble satt på pause.',
        id: `paused-${state.activity.length}`,
        kind: 'pause',
        title: 'Pauset',
      })),
      onBrowserRunResumed: (event) => update(runId, (state) => appendActivity(state, {
        at: event.at ?? new Date().toISOString(),
        detail: 'Nettleserkjøringen fortsatte.',
        id: `resumed-${state.activity.length}`,
        kind: 'resume',
        title: 'Fortsatte',
      })),
      onDone: () => update(runId, (state) => closeChatRunWatch(state)),
      onError: () => update(runId, (state) => closeChatRunWatch(state, RUN_STREAM_ERROR)),
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
    }, controller.signal)
    onCleanup(() => controller.abort())
  }))

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
          class="velion-chat-run-panel"
          classList={{ 'velion-chat-run-panel--collapsed': props.collapsed }}
          aria-label="Live agentkjøring"
        >
          <header class="velion-chat-run-panel__head">
            <button
              type="button"
              class="velion-chat-run-panel__toggle"
              aria-expanded={!props.collapsed}
              aria-label={props.collapsed ? 'Vis live-panelet' : 'Skjul live-panelet'}
              title={props.collapsed ? 'Vis live-panelet' : 'Skjul live-panelet'}
              onClick={() => props.onToggleCollapsed()}
            >
              <Show when={props.collapsed} fallback={<PanelRightClose size={15} />}>
                <PanelRightOpen size={15} />
              </Show>
            </button>
            <Show when={!props.collapsed}>
              <div class="velion-chat-run-panel__title">
                <strong><MonitorPlay size={13} /> Live agent</strong>
                <span>{statusLabel()}</span>
              </div>
              <code class="velion-chat-run-panel__runid" title={runId()}>{runId()}</code>
            </Show>
          </header>

          <Show when={!props.collapsed}>
            <div class="velion-chat-run-panel__body">
              <Show
                when={framed() && session()}
                fallback={(
                  <ChatRunEvidenceFallback
                    error={watch()?.error ?? null}
                    live={live()}
                    steps={steps()}
                    zdr={zdr()}
                  />
                )}
              >
                {(model) => <BrowserChrome compact session={model()} />}
              </Show>

              <Show when={steps().length > 0}>
                <section class="velion-chat-run-shots" aria-label="Skjermbilder per steg">
                  <div class="velion-chat-run-shots__rail">
                    <For each={steps()}>
                      {(step) => (
                        <button
                          type="button"
                          class="velion-chat-run-shot"
                          classList={{ 'velion-chat-run-shot--active': expandedStep() === step.step }}
                          aria-pressed={expandedStep() === step.step}
                          title={`Steg ${step.step} · ${stepLabel(step)}`}
                          onClick={() => setExpandedStep((current) => (current === step.step ? null : step.step))}
                        >
                          <span class="velion-chat-run-shot__frame">
                            <Show
                              when={shotState(step) === 'ready' && step.screenshotUrl}
                              fallback={(
                                <span class="velion-chat-run-shot__placeholder">
                                  {screenshotNoteIcon(shotState(step) as Exclude<ChatRunScreenshotState, 'ready'>)}
                                </span>
                              )}
                            >
                              {(src) => (
                                <img
                                  src={src()}
                                  alt={`Skjermbilde fra steg ${step.step}`}
                                  loading="lazy"
                                  decoding="async"
                                  onError={() => update(runId(), (state) => markScreenshotFailed(state, step.actionId))}
                                />
                              )}
                            </Show>
                          </span>
                          <span class="velion-chat-run-shot__label">
                            <em>#{step.step}</em> {stepLabel(step)}
                          </span>
                        </button>
                      )}
                    </For>
                  </div>

                  <Show when={expanded()}>
                    {(step) => (
                      <figure class="velion-chat-run-shot-expanded">
                        <Show
                          when={shotState(step()) === 'ready' && step().screenshotUrl}
                          fallback={(
                            <figcaption class="velion-chat-run-shot-expanded__note">
                              {screenshotNoteIcon(shotState(step()) as Exclude<ChatRunScreenshotState, 'ready'>)}
                              {' '}
                              {CHAT_RUN_SCREENSHOT_NOTES[shotState(step()) as Exclude<ChatRunScreenshotState, 'ready'>]}
                            </figcaption>
                          )}
                        >
                          {(src) => (
                            <img
                              src={src()}
                              alt={`Skjermbilde fra steg ${step().step}`}
                              decoding="async"
                              onError={() => update(runId(), (state) => markScreenshotFailed(state, step().actionId))}
                            />
                          )}
                        </Show>
                        <figcaption class="velion-chat-run-shot-expanded__meta">
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
                <section class="velion-chat-run-activity" aria-label="Kjøringshendelser">
                  <For each={activity().slice(-12)}>
                    {(entry) => (
                      <div class="velion-chat-run-activity__row" data-status={entry.status ?? ''}>
                        <strong>{entry.title}</strong>
                        <Show when={entry.detail}><span>{entry.detail}</span></Show>
                      </div>
                    )}
                  </For>
                </section>
              </Show>
            </div>
          </Show>
        </aside>
      )}
    </Show>
  )
}
