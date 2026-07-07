import {
  AlertCircle,
  ArrowLeft,
  ArrowRight,
  Camera,
  Code2,
  Copy,
  ExternalLink,
  Eye,
  History,
  Keyboard,
  LockKeyhole,
  MoreVertical,
  MousePointer2,
  Navigation,
  Network,
  PanelRight,
  Pause,
  Play,
  RefreshCw,
  Send,
  ShieldCheck,
  Sparkles,
  Square,
  SquareMousePointer,
  Terminal,
  TextCursorInput,
  Timer,
  X,
} from 'lucide-solid'
import {
  createEffect,
  createSignal,
  createUniqueId,
  For,
  Match,
  onCleanup,
  Show,
  Switch,
  type JSX,
} from 'solid-js'
import type {
  BrowserAction,
  BrowserActionSuggestionResponse,
  BrowserProfileRestoreProbe,
} from '@/shared/api/browser-client'
import { isBrowserLoopRunning, type BrowserLoopState } from './browser-loop'
import {
  closeBrowserChromePopover,
  evidenceIsEphemeral,
  initialBrowserChromeState,
  rationaleForStep,
  timelineDetail,
  toggleBrowserChromePanel,
  toggleBrowserChromePopover,
  type BrowserChromePanel,
  type BrowserChromePopover,
  type BrowserSessionViewModel,
  type BrowserStepRationale,
  type BrowserTimelineViewEntry,
} from './browser-session'
import { BrowserTimelineDetailPanel, compactBrowserUrl, consoleTone, networkTone } from './BrowserTimelineDetail'
import { hostnameOf } from './knowledge-preview'

export const SESSION_STATUS_LABELS: Record<BrowserSessionViewModel['status'], string> = {
  closed: 'Lukket',
  degraded: 'Degradert',
  live: 'Live',
}

export const RENDER_MODE_LABELS: Record<BrowserSessionViewModel['renderMode'], string> = {
  chromium: 'Chromium',
  dom_snapshot: 'DOM-snapshot',
  readability_fallback: 'Readability',
}

const PROFILE_SCOPE_LABELS: Record<BrowserSessionViewModel['profileScope'], string> = {
  ephemeral: 'Efemer',
  org_shared: 'Org-delt',
  run_scoped: 'Kjøringsscopet',
  user_private: 'Privat',
}

const LOOP_STATUS_LABELS: Record<BrowserLoopState['status'], string> = {
  acting: 'Utfører',
  done: 'Ferdig',
  idle: 'Inaktiv',
  paused: 'Pauset',
  stopped: 'Stoppet',
  suggesting: 'Foreslår',
}

function compactArtifactId(value: string): string {
  if (value.length <= 22) return value
  return `${value.slice(0, 9)}...${value.slice(-8)}`
}

type DevtoolsTab = 'console' | 'dom' | 'network' | 'vision'

/** Kort fanetekst for et snapshot-steg: tittel, ellers vertsnavn, ellers stegnummer. */
function snapshotTabLabel(entry: BrowserTimelineViewEntry): string {
  if (entry.title) return entry.title
  if (entry.url) return hostnameOf(entry.url)
  return `Steg ${entry.step}`
}

/** DOM-nodeliste delt mellom sidevisningens fallback og DevTools-panelet. */
function DomNodeList(props: { nodes: BrowserSessionViewModel['domNodes'] }) {
  return (
    <div class="knowledge-browser-live-dom">
      <For
        each={props.nodes}
        fallback={<p class="knowledge-browser-empty">Ingen DOM-noder returnert ennå.</p>}
      >
        {(node) => (
          <div class="knowledge-browser-live-dom__node">
            <span>{node.kind}</span>
            <p>{node.text}</p>
            <Show when={node.selector}>
              {(selector) => <code>{selector()}</code>}
            </Show>
          </div>
        )}
      </For>
    </div>
  )
}

/**
 * Unified browser chrome for the knowledge-card browser tab: ONE tab strip
 * (live + snapshot frames), ONE toolbar (nav, padlock+address, status, tools),
 * and content that dominates. Everything that used to be a permanent band —
 * profile strip, manual action bar, observation inspector, timeline + step
 * detail — now lives behind toolbar toggles as a popover, a collapsible right
 * devtools panel, or a bottom evidence drawer. The only always-visible marker
 * is the ZDR chip: the plan's honesty constraint requires ephemeral sessions to
 * say so at all times.
 */
export function BrowserChrome(props: {
  browserBusy?: boolean
  browserLoop?: BrowserLoopState
  browserRationales?: BrowserStepRationale[]
  /** Fallback body (non-live render modes); shown instead of the live page. */
  children?: JSX.Element
  onBrowserAction?: (action: BrowserAction) => void
  onBrowserAutoRun?: (goal: string) => Promise<BrowserActionSuggestionResponse | null>
  onBrowserLoopPause?: () => void
  onBrowserLoopResume?: () => void
  onBrowserLoopStop?: () => void
  onBrowserSuggestAction?: (goal: string) => Promise<BrowserActionSuggestionResponse | null>
  profileProbe?: BrowserProfileRestoreProbe | null
  session: BrowserSessionViewModel
}) {
  const uid = createUniqueId()
  const actionbarId = `browser-actionbar-${uid}`
  const devtoolsId = `browser-devtools-${uid}`
  const evidenceId = `browser-evidence-${uid}`
  const profilePopoverId = `browser-profile-${uid}`
  const overflowPopoverId = `browser-overflow-${uid}`

  const [chrome, setChrome] = createSignal(initialBrowserChromeState)
  const [devtoolsTab, setDevtoolsTab] = createSignal<DevtoolsTab>('dom')
  const [selectedTimelineStep, setSelectedTimelineStep] = createSignal<number | null>(null)
  const [brokenFrameUrl, setBrokenFrameUrl] = createSignal<string | null>(null)
  const [addressInput, setAddressInput] = createSignal('')
  const [lastObservedUrl, setLastObservedUrl] = createSignal('')
  const [selectorInput, setSelectorInput] = createSignal('')
  const [textInput, setTextInput] = createSignal('')
  const [keyInput, setKeyInput] = createSignal('Enter')
  const [goalInput, setGoalInput] = createSignal('Capture useful evidence from this page')
  const [lastSuggestion, setLastSuggestion] = createSignal<BrowserActionSuggestionResponse | null>(null)
  const [suggestionDismissed, setSuggestionDismissed] = createSignal(false)
  const [copyState, setCopyState] = createSignal<'copied' | 'failed' | 'idle'>('idle')

  const session = () => props.session
  const isLive = () => session().renderMode === 'chromium'
  const controlsDisabled = () => !isLive() || props.browserBusy || !session().sessionId
  const loopStatus = () => props.browserLoop?.status ?? 'idle'
  const loopRunning = () => isBrowserLoopRunning(loopStatus())
  const timelineTabs = () => session().timeline.slice(-8)
  const selectedTimelineEntry = () => {
    const selected = selectedTimelineStep()
    if (selected === null) return null
    return session().timeline.find((entry) => entry.step === selected) ?? null
  }
  const selectedTimelineDetail = () => timelineDetail(session().timeline, selectedTimelineStep())
  const selectedRationale = () => rationaleForStep(props.browserRationales ?? [], selectedTimelineStep())
  const frameUrl = () => {
    const url = selectedTimelineEntry()?.screenshotUrl ?? session().frameUrl
    return url && brokenFrameUrl() !== url ? url : null
  }
  const selector = () => selectorInput().trim()
  const address = () => addressInput().trim()
  const textValue = () => textInput()
  const keyValue = () => keyInput().trim() || 'Enter'
  const selectorActionDisabled = () => controlsDisabled() || selector().length === 0
  const typeActionDisabled = () => selectorActionDisabled() || textValue().length === 0
  const modelLoopDisabled = () => controlsDisabled() || !props.onBrowserAutoRun
  const modelActionDisabled = () => controlsDisabled() || !props.onBrowserSuggestAction
  const probe = () => {
    const current = props.profileProbe
    return current && current.profile_id === session().profileId ? current : null
  }
  const statusTitle = () => {
    const base = `Øktstatus: ${SESSION_STATUS_LABELS[session().status]}`
    const reason = session().degradedReason
    return reason ? `${base} — ${reason}` : base
  }
  const loopTitle = () => {
    const parts = [`AI-loop: ${LOOP_STATUS_LABELS[loopStatus()]}`]
    const goal = props.browserLoop?.goal
    const loopError = props.browserLoop?.error
    if (goal) parts.push(`Mål: ${goal}`)
    if (loopError) parts.push(`Feil: ${loopError}`)
    return parts.join(' · ')
  }
  const shotArtifactId = () => session().frameArtifactId ?? session().screenshotArtifactId ?? null

  const togglePanel = (panel: BrowserChromePanel) => setChrome((state) => toggleBrowserChromePanel(state, panel))
  const togglePopover = (popover: BrowserChromePopover) =>
    setChrome((state) => toggleBrowserChromePopover(state, popover))
  const closePopover = () => setChrome(closeBrowserChromePopover)

  const runAction = (action: BrowserAction) => {
    if (controlsDisabled()) return
    props.onBrowserAction?.(action)
  }
  const navigateFromAddress = () => {
    const target = address() || session().url
    if (!target) return
    runAction({ type: 'navigate', url: target })
  }
  const suggestAction = async () => {
    if (modelActionDisabled()) return
    const suggestion = await props.onBrowserSuggestAction?.(goalInput().trim())
    setLastSuggestion(suggestion ?? null)
    setSuggestionDismissed(false)
  }
  const runModelLoop = async () => {
    if (modelLoopDisabled()) return
    setSelectedTimelineStep(null)
    const suggestion = await props.onBrowserAutoRun?.(goalInput().trim())
    setLastSuggestion(suggestion ?? null)
    setSuggestionDismissed(false)
  }
  const copyProfileId = async () => {
    const profileId = session().profileId
    if (!profileId) return
    try {
      await navigator.clipboard.writeText(profileId)
      setCopyState('copied')
    } catch {
      setCopyState('failed')
    }
    window.setTimeout(() => setCopyState('idle'), 1800)
  }

  createEffect(() => {
    const url = session().url
    if (url && lastObservedUrl() !== url) {
      setAddressInput(url)
      setLastObservedUrl(url)
    }
  })

  createEffect(() => {
    const selected = selectedTimelineStep()
    if (selected === null) return
    if (!session().timeline.some((entry) => entry.step === selected)) {
      setSelectedTimelineStep(null)
    }
  })

  createEffect(() => {
    if (devtoolsTab() === 'vision' && !session().visualObservationArtifactId) {
      setDevtoolsTab('dom')
    }
  })

  createEffect(() => {
    if (!chrome().popover) return
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') closePopover()
    }
    document.addEventListener('keydown', onKeyDown)
    onCleanup(() => document.removeEventListener('keydown', onKeyDown))
  })

  return (
    <div class="knowledge-browser-chrome">
      <Show when={isLive()}>
        <div class="knowledge-browser-chrome__tabstrip" role="tablist" aria-label="Nettleserfaner">
          <button
            type="button"
            role="tab"
            aria-selected={selectedTimelineStep() === null}
            class="knowledge-browser-tab"
            classList={{ 'knowledge-browser-tab--active': selectedTimelineStep() === null }}
            title={`Live · ${session().title || session().host}`}
            onClick={() => setSelectedTimelineStep(null)}
          >
            <span class="knowledge-browser-tab__dot" data-status={session().status} aria-hidden="true" />
            <span class="knowledge-browser-tab__mode">Live</span>
            <strong class="knowledge-browser-tab__label">{session().title || session().host}</strong>
          </button>
          <For each={timelineTabs()}>
            {(entry) => (
              <button
                type="button"
                role="tab"
                aria-selected={selectedTimelineStep() === entry.step}
                class="knowledge-browser-tab"
                classList={{ 'knowledge-browser-tab--active': selectedTimelineStep() === entry.step }}
                title={`Snapshot fra steg ${entry.step} — ${snapshotTabLabel(entry)}`}
                onClick={() => setSelectedTimelineStep((current) => (current === entry.step ? null : entry.step))}
              >
                <span class="knowledge-browser-tab__dot knowledge-browser-tab__dot--snapshot" aria-hidden="true" />
                <span class="knowledge-browser-tab__mode">#{entry.step}</span>
                <strong class="knowledge-browser-tab__label">{snapshotTabLabel(entry)}</strong>
                <Show when={rationaleForStep(props.browserRationales ?? [], entry.step) || entry.visualObservationArtifactId}>
                  <span class="knowledge-browser-tab__markers">
                    <Show when={rationaleForStep(props.browserRationales ?? [], entry.step)}>
                      <Sparkles class="size-3" aria-label="AI-foreslått steg" />
                    </Show>
                    <Show when={entry.visualObservationArtifactId}>
                      <Eye class="size-3" aria-label="Visuell observasjon tilgjengelig" />
                    </Show>
                  </span>
                </Show>
              </button>
            )}
          </For>
        </div>
      </Show>

      <div class="knowledge-browser-chrome__toolbar" aria-label="Nettleserkontroller">
        <div class="knowledge-browser-chrome__nav">
          <button
            type="button"
            aria-label="Gå tilbake i nettleserøkten"
            title="Gå tilbake"
            disabled={controlsDisabled()}
            onClick={() => runAction({ type: 'back' })}
          >
            <ArrowLeft class="size-3.5" />
          </button>
          <button
            type="button"
            aria-label="Gå fremover i nettleserøkten"
            title="Gå fremover"
            disabled={controlsDisabled()}
            onClick={() => runAction({ type: 'forward' })}
          >
            <ArrowRight class="size-3.5" />
          </button>
          <button
            type="button"
            aria-label="Last nettlesersiden på nytt"
            title="Last på nytt"
            disabled={controlsDisabled()}
            onClick={() => runAction({ type: 'navigate', url: session().url })}
          >
            <RefreshCw class="size-3.5" />
          </button>
        </div>

        <div class="knowledge-browser-chrome__address">
          <button
            type="button"
            class="knowledge-browser-chrome__lock"
            aria-label="Profil, cookies og personvern for økten"
            aria-haspopup="dialog"
            aria-expanded={chrome().popover === 'profile'}
            aria-controls={profilePopoverId}
            title="Profil, cookies og personvern"
            onClick={() => togglePopover('profile')}
          >
            <LockKeyhole class="size-3.5" />
          </button>
          <input
            value={addressInput()}
            disabled={controlsDisabled()}
            aria-label="Nettleseradresse"
            spellcheck={false}
            onInput={(event) => setAddressInput(event.currentTarget.value)}
            onKeyDown={(event) => {
              if (event.key === 'Enter') {
                event.preventDefault()
                navigateFromAddress()
              }
            }}
          />
          <button
            type="button"
            class="knowledge-browser-chrome__go"
            aria-label="Naviger til adresse"
            title="Naviger"
            disabled={controlsDisabled() || !address()}
            onClick={navigateFromAddress}
          >
            <Send class="size-3.5" />
          </button>
        </div>

        <div class="knowledge-browser-chrome__status">
          <span
            class={`knowledge-browser-status knowledge-browser-status--${session().status}`}
            title={statusTitle()}
          >
            {SESSION_STATUS_LABELS[session().status]}
          </span>
          <span class="knowledge-browser-mode-badge" title={`Gjengivelsesmodus: ${session().renderMode}`}>
            {RENDER_MODE_LABELS[session().renderMode]}
          </span>
          <Show when={evidenceIsEphemeral(session())}>
            <span
              class="knowledge-browser-zdr-chip"
              title="Zero Data Retention — flyktig evidens; skjermbilder, cookies og profil lagres ikke"
            >
              <ShieldCheck class="size-3" aria-hidden="true" /> ZDR · flyktig
            </span>
          </Show>
        </div>

        <div class="knowledge-browser-chrome__tools">
          <button
            type="button"
            class="knowledge-browser-toolbtn"
            aria-label="Ta skjermbilde"
            title="Skjermbilde"
            disabled={controlsDisabled()}
            onClick={() => runAction({ type: 'screenshot', full_page: false })}
          >
            <Camera class="size-3.5" />
          </button>
          <i class="knowledge-browser-chrome__divider" aria-hidden="true" />
          <button
            type="button"
            class="knowledge-browser-toolbtn knowledge-browser-toolbtn--ai"
            aria-label="Kjør ett AI-foreslått nettlesersteg"
            title="AI-steg"
            disabled={modelActionDisabled()}
            onClick={() => void suggestAction()}
          >
            <Sparkles class="size-3.5" />
          </button>
          <button
            type="button"
            class="knowledge-browser-toolbtn knowledge-browser-toolbtn--ai"
            aria-label="Kjør flere AI-foreslåtte nettlesersteg"
            title="AI-loop"
            disabled={modelLoopDisabled()}
            onClick={() => void runModelLoop()}
          >
            <Play class="size-3.5" />
          </button>
          <Show when={loopRunning()}>
            <Show
              when={loopStatus() === 'paused'}
              fallback={
                <button
                  type="button"
                  class="knowledge-browser-toolbtn knowledge-browser-toolbtn--ai"
                  aria-label="Pause AI-loopen mellom steg"
                  title="Pause AI-loop"
                  onClick={() => props.onBrowserLoopPause?.()}
                >
                  <Pause class="size-3.5" />
                </button>
              }
            >
              <button
                type="button"
                class="knowledge-browser-toolbtn knowledge-browser-toolbtn--ai"
                aria-label="Fortsett AI-loopen"
                title="Fortsett AI-loop"
                onClick={() => props.onBrowserLoopResume?.()}
              >
                <Play class="size-3.5" />
              </button>
            </Show>
            <button
              type="button"
              class="knowledge-browser-toolbtn knowledge-browser-toolbtn--stop"
              aria-label="Stopp AI-loopen"
              title="Stopp AI-loop"
              onClick={() => props.onBrowserLoopStop?.()}
            >
              <Square class="size-3.5" />
            </button>
          </Show>
          <Show when={loopStatus() !== 'idle'}>
            <span
              class={`knowledge-browser-loop-chip knowledge-browser-loop-chip--${loopStatus()}`}
              classList={{ 'knowledge-browser-loop-chip--error': Boolean(props.browserLoop?.error) }}
              role="status"
              aria-label="AI-loopstatus"
              title={loopTitle()}
            >
              {LOOP_STATUS_LABELS[loopStatus()]}
              <Show when={(props.browserLoop?.step ?? 0) > 0}>
                <strong>{props.browserLoop?.step}</strong>
              </Show>
              <Show when={props.browserLoop?.error}>
                <AlertCircle class="size-3" aria-hidden="true" />
              </Show>
            </span>
          </Show>
          <i class="knowledge-browser-chrome__divider" aria-hidden="true" />
          <button
            type="button"
            class="knowledge-browser-toolbtn"
            classList={{ 'knowledge-browser-toolbtn--on': chrome().actionsOpen }}
            aria-label="Vis eller skjul manuelle nettleserhandlinger"
            aria-expanded={chrome().actionsOpen}
            aria-controls={actionbarId}
            title="Manuelle handlinger"
            disabled={!isLive()}
            onClick={() => togglePanel('actions')}
          >
            <SquareMousePointer class="size-3.5" />
          </button>
          <button
            type="button"
            class="knowledge-browser-toolbtn"
            classList={{ 'knowledge-browser-toolbtn--on': chrome().devtoolsOpen }}
            aria-label="Vis eller skjul DevTools-panelet"
            aria-expanded={chrome().devtoolsOpen}
            aria-controls={devtoolsId}
            title="DevTools — DOM, console, network"
            disabled={!isLive()}
            onClick={() => togglePanel('devtools')}
          >
            <PanelRight class="size-3.5" />
            <Show when={session().policyDenials.length > 0}>
              <span class="knowledge-browser-toolbtn__badge knowledge-browser-toolbtn__badge--warn">
                {session().policyDenials.length}
              </span>
            </Show>
          </button>
          <button
            type="button"
            class="knowledge-browser-toolbtn"
            classList={{ 'knowledge-browser-toolbtn--on': chrome().evidenceOpen }}
            aria-label="Vis eller skjul tidslinje og evidens"
            aria-expanded={chrome().evidenceOpen}
            aria-controls={evidenceId}
            title="Tidslinje og evidens"
            disabled={!isLive()}
            onClick={() => togglePanel('evidence')}
          >
            <History class="size-3.5" />
            <Show when={session().timeline.length > 0}>
              <span class="knowledge-browser-toolbtn__badge">{session().timeline.length}</span>
            </Show>
          </button>
          <button
            type="button"
            class="knowledge-browser-toolbtn"
            aria-label="Flere valg"
            aria-haspopup="menu"
            aria-expanded={chrome().popover === 'overflow'}
            aria-controls={overflowPopoverId}
            title="Flere valg"
            onClick={() => togglePopover('overflow')}
          >
            <MoreVertical class="size-3.5" />
          </button>
        </div>

        <Show when={chrome().popover === 'profile'}>
          <>
            <div class="knowledge-browser-popover-backdrop" onClick={closePopover} />
            <div
              id={profilePopoverId}
              class="knowledge-browser-popover knowledge-browser-popover--profile"
              role="dialog"
              aria-label="Nettleserprofil og personvern"
            >
              <header class="knowledge-browser-popover__head">
                <LockKeyhole class="size-3.5" aria-hidden="true" />
                <span>{PROFILE_SCOPE_LABELS[session().profileScope]} profil</span>
              </header>
              <Show when={session().profileId} fallback={
                <div class="knowledge-browser-popover__row">
                  <span>Profil</span>
                  <strong>{session().profileLabel}</strong>
                </div>
              }>
                {(profileId) => (
                  <div class="knowledge-browser-popover__row">
                    <span>Profil-ID</span>
                    <span class="knowledge-browser-popover__id">
                      <code>{profileId()}</code>
                      <button
                        type="button"
                        class="knowledge-browser-popover__copy"
                        aria-label="Kopier profil-ID"
                        title="Kopier profil-ID"
                        onClick={() => void copyProfileId()}
                      >
                        <Copy class="size-3" />
                      </button>
                      <Show when={copyState() !== 'idle'}>
                        <em>{copyState() === 'copied' ? 'Kopiert' : 'Kunne ikke kopiere'}</em>
                      </Show>
                    </span>
                  </div>
                )}
              </Show>
              <div class="knowledge-browser-popover__row">
                <span>Cookies</span>
                <strong
                  classList={{ 'knowledge-browser-popover__value--persistent': session().profileStorage === 'persistent' }}
                >
                  {session().profileStorage === 'persistent' ? 'Lagres i profilen' : 'Lagres ikke'}
                </strong>
              </div>
              <Show when={probe()}>
                {(currentProbe) => (
                  <div class="knowledge-browser-popover__row">
                    <span>Probe</span>
                    <strong>
                      {currentProbe().cookies_count} cookies · {currentProbe().restorable ? 'gjenopprettbar' : 'ikke gjenopprettbar'}
                    </strong>
                  </div>
                )}
              </Show>
              <div class="knowledge-browser-popover__row">
                <span>Gjengivelse</span>
                <strong>
                  {RENDER_MODE_LABELS[session().renderMode]} · {session().viewport.width} × {session().viewport.height}
                </strong>
              </div>
              <div class="knowledge-browser-popover__row">
                <span>Kilde</span>
                <strong>{session().sourceLabel}</strong>
              </div>
              <Show when={session().degradedReason}>
                {(reason) => (
                  <div class="knowledge-browser-popover__row knowledge-browser-popover__row--warning">
                    <span>Degradert</span>
                    <strong title={reason()}>{reason()}</strong>
                  </div>
                )}
              </Show>
              <Show when={evidenceIsEphemeral(session())}>
                <div class="knowledge-browser-popover__row">
                  <span>ZDR</span>
                  <span class="knowledge-browser-zdr-chip">
                    <ShieldCheck class="size-3" aria-hidden="true" /> Flyktig evidens — ingen lagring
                  </span>
                </div>
              </Show>
            </div>
          </>
        </Show>

        <Show when={chrome().popover === 'overflow'}>
          <>
            <div class="knowledge-browser-popover-backdrop" onClick={closePopover} />
            <div
              id={overflowPopoverId}
              class="knowledge-browser-popover knowledge-browser-popover--overflow"
              role="menu"
              aria-label="Flere nettleservalg"
            >
              <div class="knowledge-browser-popover__menu">
                <a
                  href={session().url}
                  target="_blank"
                  rel="noopener noreferrer"
                  role="menuitem"
                  onClick={closePopover}
                >
                  <ExternalLink class="size-3.5" aria-hidden="true" /> Åpne siden i ny fane
                </a>
                <Show when={session().visualObservationUrl}>
                  {(url) => (
                    <a
                      href={url()}
                      target="_blank"
                      rel="noopener noreferrer"
                      role="menuitem"
                      onClick={closePopover}
                    >
                      <Eye class="size-3.5" aria-hidden="true" /> Åpne visual_observation.json
                    </a>
                  )}
                </Show>
              </div>
            </div>
          </>
        </Show>
      </div>

      <Show when={isLive() && chrome().actionsOpen}>
        <div id={actionbarId} class="knowledge-browser-actionbar" aria-label="Manuelle nettleserhandlinger">
          <label class="knowledge-browser-actionbar__field knowledge-browser-actionbar__field--selector">
            <MousePointer2 class="size-3.5" aria-hidden="true" />
            <input
              value={selectorInput()}
              disabled={controlsDisabled()}
              placeholder="CSS selector"
              spellcheck={false}
              onInput={(event) => setSelectorInput(event.currentTarget.value)}
            />
          </label>
          <label class="knowledge-browser-actionbar__field">
            <TextCursorInput class="size-3.5" aria-hidden="true" />
            <input
              value={textInput()}
              disabled={controlsDisabled()}
              placeholder="Text"
              onInput={(event) => setTextInput(event.currentTarget.value)}
            />
          </label>
          <label class="knowledge-browser-actionbar__field knowledge-browser-actionbar__field--key">
            <Keyboard class="size-3.5" aria-hidden="true" />
            <input
              value={keyInput()}
              disabled={controlsDisabled()}
              placeholder="Key"
              spellcheck={false}
              onInput={(event) => setKeyInput(event.currentTarget.value)}
              onKeyDown={(event) => {
                if (event.key === 'Enter') {
                  event.preventDefault()
                  runAction({ type: 'press', key: keyValue() })
                }
              }}
            />
          </label>
          <label class="knowledge-browser-actionbar__field knowledge-browser-actionbar__field--goal">
            <Sparkles class="size-3.5" aria-hidden="true" />
            <input
              value={goalInput()}
              disabled={controlsDisabled()}
              placeholder="AI goal"
              onInput={(event) => setGoalInput(event.currentTarget.value)}
              onKeyDown={(event) => {
                if (event.key === 'Enter') {
                  event.preventDefault()
                  void suggestAction()
                }
              }}
            />
          </label>
          <div class="knowledge-browser-actionbar__buttons">
            <button
              type="button"
              aria-label="Klikk valgt selector"
              title="Klikk"
              disabled={selectorActionDisabled()}
              onClick={() => runAction({ type: 'click', selector: selector() })}
            >
              <MousePointer2 class="size-3.5" />
            </button>
            <button
              type="button"
              aria-label="Skriv tekst i valgt selector"
              title="Skriv"
              disabled={typeActionDisabled()}
              onClick={() => runAction({ type: 'type', selector: selector(), text: textValue() })}
            >
              <TextCursorInput class="size-3.5" />
            </button>
            <button
              type="button"
              aria-label="Send tastetrykk"
              title="Tast"
              disabled={controlsDisabled()}
              onClick={() => runAction({ type: 'press', key: keyValue() })}
            >
              <Keyboard class="size-3.5" />
            </button>
            <button
              type="button"
              aria-label="Vent på valgt selector"
              title="Vent på selector"
              disabled={selectorActionDisabled()}
              onClick={() => runAction({ type: 'wait_for', selector: selector(), timeout_ms: 5000 })}
            >
              <Timer class="size-3.5" />
            </button>
            <button
              type="button"
              aria-label="Rull til valgt selector"
              title="Rull"
              disabled={selectorActionDisabled()}
              onClick={() => runAction({ type: 'scroll', target: selector() })}
            >
              <Navigation class="size-3.5" />
            </button>
          </div>
        </div>
      </Show>

      <div
        class="knowledge-browser-chrome__body"
        classList={{ 'knowledge-browser-chrome__body--devtools': isLive() && chrome().devtoolsOpen }}
      >
        <Show when={isLive()} fallback={props.children}>
          <div class="knowledge-browser-chrome__page" aria-label="Gjengitt nettleserside">
            <Show when={!suggestionDismissed() && lastSuggestion()?.suggestion.reason}>
              {(reason) => (
                <div class="knowledge-browser-ai-bubble" role="status" aria-label="Siste AI-forslag">
                  <Sparkles class="size-3.5" aria-hidden="true" />
                  <div class="knowledge-browser-ai-bubble__body">
                    <span class="knowledge-browser-ai-bubble__kind">
                      {lastSuggestion()?.suggestion.done ? 'done' : lastSuggestion()?.suggestion.action?.type ?? 'no-action'}
                    </span>
                    <p>{reason()}</p>
                  </div>
                  <button
                    type="button"
                    aria-label="Lukk AI-forslaget"
                    onClick={() => setSuggestionDismissed(true)}
                  >
                    <X class="size-3" />
                  </button>
                </div>
              )}
            </Show>
            <For each={session().commentAnchors}>
              {(anchor) => (
                <span
                  class="knowledge-browser-comment-anchor"
                  style={{
                    left: `${anchor.x * 100}%`,
                    top: `${anchor.y * 100}%`,
                  }}
                  aria-label={`Kommentar ${anchor.label}`}
                >
                  {anchor.label}
                </span>
              )}
            </For>
            <Show
              when={frameUrl()}
              keyed
              fallback={
                <div class="knowledge-browser-chrome__domlist">
                  <DomNodeList nodes={session().domNodes} />
                </div>
              }
            >
              {(src) => (
                <div class="knowledge-browser-screenshot" aria-label="Gjengitt Chromium-side">
                  <img
                    src={src}
                    alt={`Gjengitt nettleserside for ${session().title}`}
                    decoding="async"
                    onError={() => setBrokenFrameUrl(src)}
                  />
                </div>
              )}
            </Show>
          </div>

          <Show when={chrome().devtoolsOpen}>
            <aside id={devtoolsId} class="knowledge-browser-devtools" aria-label="Nettleserinspektør">
              <header class="knowledge-browser-devtools__head">
                <span>DevTools</span>
                <span class="knowledge-browser-devtools__stats">
                  {session().domNodes.length}/{session().nodeCount ?? session().domNodes.length} DOM · {session().networkEntries.length} network
                </span>
                <Show when={shotArtifactId()}>
                  {(artifactId) => <code title={artifactId()}>shot {compactArtifactId(artifactId())}</code>}
                </Show>
                <button
                  type="button"
                  aria-label="Lukk DevTools-panelet"
                  onClick={() => togglePanel('devtools')}
                >
                  <X class="size-3.5" />
                </button>
              </header>
              <div class="knowledge-browser-devtools__tabs" role="tablist" aria-label="Observasjonspaneler">
                <button
                  type="button"
                  role="tab"
                  aria-selected={devtoolsTab() === 'dom'}
                  class="knowledge-browser-devtools__tab"
                  classList={{ 'knowledge-browser-devtools__tab--active': devtoolsTab() === 'dom' }}
                  onClick={() => setDevtoolsTab('dom')}
                >
                  <Code2 class="size-3.5" /> DOM <em>{session().nodeCount ?? session().domNodes.length}</em>
                </button>
                <button
                  type="button"
                  role="tab"
                  aria-selected={devtoolsTab() === 'console'}
                  class="knowledge-browser-devtools__tab"
                  classList={{ 'knowledge-browser-devtools__tab--active': devtoolsTab() === 'console' }}
                  onClick={() => setDevtoolsTab('console')}
                >
                  <Terminal class="size-3.5" /> Console <em>{session().consoleEntries.length}</em>
                </button>
                <button
                  type="button"
                  role="tab"
                  aria-selected={devtoolsTab() === 'network'}
                  class="knowledge-browser-devtools__tab"
                  classList={{ 'knowledge-browser-devtools__tab--active': devtoolsTab() === 'network' }}
                  onClick={() => setDevtoolsTab('network')}
                >
                  <Network class="size-3.5" /> Network <em>{session().networkEntries.length}</em>
                </button>
                <Show when={session().visualObservationArtifactId}>
                  <button
                    type="button"
                    role="tab"
                    aria-selected={devtoolsTab() === 'vision'}
                    class="knowledge-browser-devtools__tab"
                    classList={{ 'knowledge-browser-devtools__tab--active': devtoolsTab() === 'vision' }}
                    onClick={() => setDevtoolsTab('vision')}
                  >
                    <Eye class="size-3.5" /> Vision
                  </button>
                </Show>
              </div>
              <div class="knowledge-browser-devtools__content">
                <Switch>
                  <Match when={devtoolsTab() === 'dom'}>
                    <p class="knowledge-browser-devtools__caption">
                      Interaktive noder · {session().domNodes.length} av {session().nodeCount ?? session().domNodes.length}
                    </p>
                    <DomNodeList nodes={session().domNodes} />
                  </Match>
                  <Match when={devtoolsTab() === 'console'}>
                    <div class="knowledge-browser-console-list">
                      <For
                        each={session().consoleEntries}
                        fallback={<p class="knowledge-browser-empty">Ingen console-hendelser.</p>}
                      >
                        {(entry) => (
                          <div class={`knowledge-browser-console-row knowledge-browser-console-row--${consoleTone(entry.level)}`}>
                            <span>{entry.level}</span>
                            <p>{entry.text}</p>
                          </div>
                        )}
                      </For>
                    </div>
                  </Match>
                  <Match when={devtoolsTab() === 'network'}>
                    <div class="knowledge-browser-network-list">
                      <For
                        each={session().networkEntries}
                        fallback={<p class="knowledge-browser-empty">Ingen nettverkskall returnert ennå.</p>}
                      >
                        {(entry) => (
                          <div class={`knowledge-browser-network-row knowledge-browser-network-row--${networkTone(entry.status)}`}>
                            <span>{entry.method}</span>
                            <strong>{entry.status}</strong>
                            <p title={entry.url}>{compactBrowserUrl(entry.url)}</p>
                          </div>
                        )}
                      </For>
                    </div>
                  </Match>
                  <Match when={devtoolsTab() === 'vision'}>
                    <Show when={session().visualObservationArtifactId}>
                      {(artifactId) => (
                        <div class="knowledge-browser-vision-artifact">
                          <span>visual_observation.json</span>
                          <code>{compactArtifactId(artifactId())}</code>
                          <Show when={session().visualObservationUrl}>
                            {(url) => (
                              <a href={url()} target="_blank" rel="noopener noreferrer" aria-label="Åpne visuell observasjon">
                                <ExternalLink class="size-3.5" />
                              </a>
                            )}
                          </Show>
                        </div>
                      )}
                    </Show>
                  </Match>
                </Switch>
              </div>
              <Show when={session().policyDenials.length > 0}>
                <div class="knowledge-browser-devtools__policy">
                  <header>
                    <span><ShieldCheck class="size-3.5" /> Policy</span>
                    <strong>{session().policyDenials.length}</strong>
                  </header>
                  <For each={session().policyDenials.slice(0, 4)}>
                    {(denial) => (
                      <p class="knowledge-browser-policy-denial">
                        <AlertCircle class="size-3.5" /> {denial}
                      </p>
                    )}
                  </For>
                </div>
              </Show>
            </aside>
          </Show>
        </Show>
      </div>

      <Show when={isLive() && chrome().evidenceOpen}>
        <div id={evidenceId} class="knowledge-browser-evidence" role="region" aria-label="Tidslinje og evidens">
          <header class="knowledge-browser-evidence__head">
            <History class="size-3.5" aria-hidden="true" />
            <span>Tidslinje</span>
            <strong>{session().timeline.length} steg</strong>
            <button
              type="button"
              aria-label="Lukk tidslinjen"
              onClick={() => togglePanel('evidence')}
            >
              <X class="size-3.5" />
            </button>
          </header>
          <Show
            when={session().timeline.length > 0}
            fallback={<p class="knowledge-browser-evidence__hint">Ingen steg registrert ennå — naviger eller kjør en handling for å bygge tidslinjen.</p>}
          >
            <div class="knowledge-browser-timeline" aria-label="Nettleserhistorikk">
              <button
                type="button"
                classList={{ 'knowledge-browser-timeline__step--active': selectedTimelineStep() === null }}
                onClick={() => setSelectedTimelineStep(null)}
              >
                <span>live</span>
                <strong>{session().title}</strong>
              </button>
              <For each={timelineTabs()}>
                {(entry) => (
                  <button
                    type="button"
                    classList={{ 'knowledge-browser-timeline__step--active': selectedTimelineStep() === entry.step }}
                    title="Vis stegdetaljer med evidens"
                    onClick={() => setSelectedTimelineStep((current) => (current === entry.step ? null : entry.step))}
                  >
                    <span>#{entry.step}</span>
                    <strong>{entry.title || compactBrowserUrl(entry.url || session().url)}</strong>
                    <Show when={rationaleForStep(props.browserRationales ?? [], entry.step) || entry.visualObservationArtifactId}>
                      <span class="knowledge-browser-timeline__markers">
                        <Show when={rationaleForStep(props.browserRationales ?? [], entry.step)}>
                          <Sparkles class="size-3" aria-label="AI-foreslått steg" />
                        </Show>
                        <Show when={entry.visualObservationArtifactId}>
                          <Eye class="size-3" aria-label="Visuell observasjon tilgjengelig" />
                        </Show>
                      </span>
                    </Show>
                  </button>
                )}
              </For>
            </div>
            <Show
              when={selectedTimelineDetail()}
              fallback={<p class="knowledge-browser-evidence__hint">Velg et steg for å se før/etter-evidens, deltaer og artefakter.</p>}
            >
              {(detail) => (
                <BrowserTimelineDetailPanel
                  detail={detail()}
                  onClose={() => setSelectedTimelineStep(null)}
                  rationale={selectedRationale()}
                  sessionId={session().sessionId}
                />
              )}
            </Show>
          </Show>
        </div>
      </Show>
    </div>
  )
}
