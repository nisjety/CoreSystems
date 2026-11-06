import { ArrowUp, ChevronDown, ChevronUp, MessageCircleMore, Minus, Plus, X } from '@/shared/icons'
import { createEffect, createSignal, For, Show, untrack } from 'solid-js'
import { useHomeLayout } from '@/features/dashboard/home/home-layout-state'
import type { InlineAssistantState } from '@/features/core/lib/inline-assistant'
import type { NettTabSelection } from '@/features/core/assistant-agent/assistant-browser-context'
import type { ChatSourceScope } from '@/shared/actions/chat-source-scope'
import type { BrowserAttachSharedJarEvent } from '@/shared/api/run-console-client'
import { useI18n } from '@/shared/i18n'
import './global-assistant.css'

/** Only real run events may move this state to a terminal value. */
export type AssistantAgentView = {
  /** Availability is established by a server-side capability check. */
  available: boolean
  /** In-app navigation operates on this page; browser work has a separate view. */
  kind?: 'navigation' | 'browser'
  localBrowser?: boolean
  status: 'idle' | 'starting' | 'running' | 'stopping' | 'stopped' | 'completed' | 'disconnected' | 'failed'
  goal: string
  error: string | null
  errorCode?: 'browser_grant_denied' | 'browser_redirect_access_denied' | 'browser_start_unavailable' | null
  siteAccessRequest?: { host: string } | null
  siteHandoffUrl?: string | null
  action: string | null
  sessionId: string | null
  runId: string | null
  receipt: BrowserAttachSharedJarEvent | null
  browserClaimed?: boolean
  usesExistingTab?: boolean
  borrowedTabId?: string
  stopRequested?: boolean
  result?: string | null
  resultLoading?: boolean
  approval?: { id: string | null; actionId: string | null; action: string; destination: string | null; target: string | null; reason: string | null; canApprove: boolean; state: 'pending' | 'submitting' | 'submitted'; error: string | null } | null
}

/** The model is choosing an in-app navigation action; no browser session is implied.
 * `stopped` confirms local cancellation only, not that remote inference stopped. */
export type AssistantNavigationAgentView = {
  status: 'idle' | 'thinking' | 'guiding' | 'stopped' | 'error'
  error: string | null
}

/** The shared Home composer, presented as a non-modal dock across the app. */
export function GlobalAssistant(props: {
  /** An explicit navbar/tab open reveals the composer for this visit. */
  forceReveal?: boolean
  message: string
  onMessageChange: (value: string) => void
  onZeroRetentionChange?: (zeroRetention: boolean | undefined) => void
  onBeforeSubmit: (text: string) => boolean
  canHandleBeforeSubmit: (text: string) => boolean
  onSubmit: (text: string) => Promise<void> | void
  onClose: () => void
  continuation?: NettTabSelection | null
  onCancelContinuation?: () => void
  onStop: () => void
  /** Browser-agent status and controls, independent from the navigation guide. */
  agentView?: AssistantAgentView
  /** Controls whether the dock should offer to return to the agent view in Nett. */
  browserPresentedInWorkspace?: boolean
  onStopAgent?: () => void
  onOpenAgentBrowser?: () => void
  onRetryAgent?: () => void
  onAllowSiteAccess?: () => Promise<void>
  onDeclineSiteAccess?: () => void
  onApprovalDecision?: (decision: 'approve' | 'reject') => void
  navigationAgent?: AssistantNavigationAgentView
  onStopNavigation?: () => void
  onOpenChat: () => void
  onNewChat: () => void
  sourceScope: ChatSourceScope
  onSourceScopeChange: (scope: ChatSourceScope) => void
  state: InlineAssistantState
}) {
  const i18n = useI18n()
  const [layout, updateLayout] = useHomeLayout()
  // A suggested follow-up or card prompt opens the assistant for this visit,
  // without overwriting the person's saved presentation preference.
  const [revealed, setRevealed] = createSignal(untrack(() => props.forceReveal === true))
  const minimized = () => layout().dockMinimized && !revealed()
  const expanded = () => layout().dockExpanded || revealed()
  createEffect(() => props.message, (message) => { if (message.trim() && minimized()) setRevealed(true) })
  const setMinimized = (next: boolean) => { setRevealed(false); updateLayout({ dockMinimized: next }) }
  const setExpanded = (next: boolean) => { setRevealed(false); updateLayout({ dockMinimized: false, dockExpanded: next }) }
  const [error, setError] = createSignal<string | null>(null)
  const [pending, setPending] = createSignal(false)
  let taskInput: HTMLTextAreaElement | undefined
  createEffect(() => props.continuation, (tab) => {
    setError(null)
    if (tab) {
      setRevealed(true)
      queueMicrotask(() => taskInput?.focus())
    }
  })
  const navigationBusy = () => props.navigationAgent?.status === 'thinking' || props.navigationAgent?.status === 'guiding'
  const agentBusy = () => navigationBusy() || ['starting', 'running', 'stopping', 'disconnected'].includes(props.agentView?.status ?? '')
  const siteAccessRequest = () => props.onAllowSiteAccess && props.onDeclineSiteAccess ? props.agentView?.siteAccessRequest : null
  const agentStatus = () => {
    if (siteAccessRequest()) return i18n.tr('Agenten trenger tilgang til nettstedet', 'The agent needs access to this site')
    if (props.agentView?.status === 'failed' && props.agentView.siteHandoffUrl && props.agentView.errorCode !== 'browser_start_unavailable') return i18n.tr('Nettstedet krever manuell tilgang', 'The site requires manual access')
    switch (props.agentView?.status) {
      case 'starting': return props.agentView?.kind === 'navigation' ? i18n.tr('Finner siden …', 'Finding the page…') : i18n.tr('Starter agenten …', 'Starting the agent…')
      case 'running': return props.agentView?.kind === 'navigation' ? i18n.tr('Viser vei', 'Showing the way') : i18n.tr('Agenten arbeider', 'The agent is working')
      case 'stopping': return i18n.tr('Ber agenten stoppe …', 'Asking the agent to stop…')
      case 'stopped': return i18n.tr('Agenten er stoppet.', 'The agent stopped.')
      case 'completed': return i18n.tr('Kjøringen er avsluttet.', 'The run has ended.')
      case 'disconnected': return i18n.tr('Forbindelsen ble brutt. Oppgaven kan fortsatt være aktiv.', 'Connection lost. The task may still be active.')
      case 'failed': return props.agentView.stopRequested
        ? i18n.tr('Oppgaven ble avsluttet etter stoppforespørselen.', 'The task ended after the stop request.')
        : i18n.tr('Agentoppgaven kunne ikke fullføres.', 'The agent task could not be completed.')
      default: return ''
    }
  }
  const compactStatus = () => {
    if (navigationBusy()) return i18n.tr('Finner siden', 'Finding the page')
    const task = props.agentView
    if (siteAccessRequest() || task?.approval?.state === 'pending') return i18n.tr('Venter på deg', 'Needs your review')
    if (task?.status === 'failed' && task.siteHandoffUrl && task.errorCode !== 'browser_start_unavailable') return i18n.tr('Krever manuell tilgang', 'Needs manual access')
    switch (task?.status) {
      case 'starting': return i18n.tr('Starter', 'Starting')
      case 'running': return i18n.tr('Arbeider', 'Working')
      case 'stopping': return i18n.tr('Stopper', 'Stopping')
      case 'disconnected': return i18n.tr('Tilkobling brutt', 'Connection lost')
      case 'failed': return task.stopRequested ? i18n.tr('Avsluttet', 'Ended') : i18n.tr('Oppgaven feilet', 'Task failed')
      case 'completed': return i18n.tr('Fullført', 'Finished')
      case 'stopped': return i18n.tr('Stoppet', 'Stopped')
      default: return null
    }
  }
  const canOpenBrowser = () => {
    const task = props.agentView
    if (task?.localBrowser) return Boolean(task.runId)
    return task?.kind === 'browser' && task.sessionId && task.runId &&
      (['starting', 'running', 'stopping', 'disconnected'].includes(task.status) || (!task.usesExistingTab && task.status === 'completed' && task.receipt))
  }
  const send = async (allowSiteAccess = false) => {
    const draft = props.message
    const accessRequest = siteAccessRequest()
    const text = allowSiteAccess ? props.agentView?.goal : draft.trim()
    if (!text || pending() || agentBusy() || (allowSiteAccess && !accessRequest)) return
    setError(null)
    if (!allowSiteAccess && props.onBeforeSubmit(text)) { props.onMessageChange(''); return }
    setPending(true)
    try {
      if (allowSiteAccess) await props.onAllowSiteAccess?.()
      else await props.onSubmit(text)
      if (!siteAccessRequest() && props.message === draft && draft.trim() === text) props.onMessageChange('')
    } catch (cause) {
      const currentRequest = siteAccessRequest()
      if (!currentRequest || (allowSiteAccess && currentRequest === accessRequest)) {
        setError(cause instanceof Error ? cause.message : i18n.tr('Kunne ikke starte oppgaven. Utkastet er beholdt.', 'Could not start the task. Your draft has been kept.'))
      }
    } finally {
      setPending(false)
    }
  }
  return (
    <section class="global-assistant" role="region" aria-label={i18n.tr('Spør Verevon', 'Ask Verevon')}>
      <div class="global-assistant__header">
        <button type="button" onClick={() => setMinimized(!minimized())} aria-expanded={minimized() ? 'false' : 'true'} aria-label={minimized() ? i18n.tr('Åpne skrivefelt', 'Expand chat bar') : i18n.tr('Minimer skrivefelt', 'Minimize chat bar')}>
          <Show when={minimized()} fallback={<Minus size={16} />}><Plus size={16} /></Show>
        </button>
        <MessageCircleMore size={16} aria-hidden="true" />
        <strong>{i18n.tr('Spør Verevon', 'Ask Verevon')}</strong>
        <Show when={minimized() && compactStatus()} fallback={<button type="button" class="global-assistant__new" disabled={agentBusy() || pending() || props.state.status === 'sending' || props.state.status === 'streaming' || props.state.status === 'stopping'} onClick={() => { setError(null); props.onNewChat() }}>{i18n.tr('Ny oppgave', 'New task')}</button>}>
          {(status) => <button type="button" class="global-assistant__summary" onClick={() => setMinimized(false)}><span role="status">{status()}</span><ChevronUp size={14} aria-hidden="true" /></button>}
        </Show>
        <Show when={!minimized()}>
          <button type="button" onClick={() => setExpanded(!expanded())} aria-expanded={expanded() ? 'true' : 'false'} aria-label={expanded() ? i18n.tr('Gjør skrivefeltet mindre', 'Make composer smaller') : i18n.tr('Utvid skrivefelt', 'Expand composer')}>
            <Show when={expanded()} fallback={<ChevronUp size={16} aria-hidden="true" />}><ChevronDown size={16} aria-hidden="true" /></Show>
          </button>
        </Show>
        <button type="button" onClick={props.onClose} aria-label={i18n.tr('Lukk assistent', 'Close assistant')}><X size={16} /></button>
      </div>
      <Show when={!minimized()}>
        <Show when={props.state.turns.length > 0 || props.state.error}>
          <div class="global-assistant__conversation" aria-live="polite">
            <For each={props.state.turns}>{(turn) => (
              <div class="global-assistant__turn" data-role={turn.role}>
                <strong>{turn.role === 'user' ? i18n.tr('Du', 'You') : 'Verevon'}</strong>
                <p>{turn.text}</p>
              </div>
            )}</For>
            <Show when={props.state.activity}><p role="status">{props.state.activity}</p></Show>
            <Show when={props.state.error}><p role="alert">{props.state.error}</p></Show>
            <Show when={props.state.needsFullChat}>
              <p>{i18n.tr('Denne samtalen inneholder detaljer som ikke vises her.', 'This conversation contains details not shown here.')}</p>
              <Show when={props.state.threadId} fallback={<p>{i18n.tr('Disse detaljene er ikke tilgjengelige i denne visningen.', 'These details are unavailable in this view.')}</p>}>
                <p>{i18n.tr('Noen detaljer kan mangle når samtalen åpnes i Chat.', 'Some details may be missing when opening the conversation in Chat.')}</p>
                <button type="button" class="global-assistant__open-chat" onClick={props.onOpenChat}>{i18n.tr('Åpne samtalen i Chat', 'Open conversation in Chat')}</button>
              </Show>
            </Show>
          </div>
        </Show>
        <Show when={props.navigationAgent && props.navigationAgent.status !== 'idle'} fallback={<Show when={props.agentView && props.agentView.status !== 'idle'} fallback={
          <>
            <p class="global-assistant__availability" role="status">{props.agentView?.available
              ? props.agentView.kind === 'browser'
                ? i18n.tr('Be Verevon finne en nettside eller gjøre noe i Nett.', 'Ask Verevon to find a website or do something in Web.')
                : i18n.tr('Be Verevon finne eller åpne en side.', 'Ask Verevon to find or open a page.')
              : props.navigationAgent
                ? i18n.tr('Be Verevon finne eller åpne en side.', 'Ask Verevon to find or open a page.')
                : i18n.tr('Agentoppgaver er ikke tilgjengelige ennå. Verevon kan vise og åpne sider.', 'Agent tasks are not available yet. Verevon can show and open pages.')}</p>
            <Show when={props.agentView?.error}>{(message) => <p class="global-assistant__task-error" role="alert">{message()}</p>}</Show>
          </>
        }>
          <div class="global-assistant__agent" aria-label={i18n.tr('Agentoppgave', 'Agent task')}>
            <div class="global-assistant__agent-heading">
              <div><strong>{agentStatus()}</strong><Show when={props.agentView?.goal}><p>{props.agentView?.goal}</p></Show></div>
              <Show when={props.onStopAgent && ['starting', 'running', 'stopping', 'disconnected'].includes(props.agentView?.status ?? '') && !(props.agentView?.status === 'disconnected' && !props.agentView?.runId)}>
                <button type="button" onClick={() => props.onStopAgent?.()} disabled={props.agentView?.status === 'stopping'}>{i18n.tr('Stopp', 'Stop')}</button>
              </Show>
              <Show when={props.onRetryAgent && props.agentView?.status === 'disconnected' && props.agentView?.sessionId && !props.agentView?.runId}>
                <button type="button" onClick={() => props.onRetryAgent?.()}>{i18n.tr('Gjenfinn oppgave', 'Recover task')}</button>
              </Show>
            </div>
            <Show when={props.agentView?.error}>{(message) => <p class="global-assistant__task-error" role="alert">{message()}</p>}</Show>
            <Show when={props.agentView?.siteHandoffUrl}>
              {(url) => <a class="global-assistant__open-chat" href={url()} target="_blank" rel="noopener noreferrer">
                {i18n.tr('Åpne utenfor Verevon', 'Open outside Verevon')}
              </a>}
            </Show>
            <Show when={props.onOpenAgentBrowser && !props.browserPresentedInWorkspace && canOpenBrowser()}>
              <button type="button" class="global-assistant__open-chat" onClick={() => props.onOpenAgentBrowser?.()}>{props.agentView?.status === 'completed'
                ? i18n.tr('Åpne nettleseren i Nett', 'Open browser in Web')
                : i18n.tr('Følg agenten i Nett', 'Watch agent in Web')}</button>
            </Show>
            <Show when={siteAccessRequest()}>
              {(request) => <section class="global-assistant__approval" aria-label={i18n.tr('Nettstedstilgang', 'Website access')}>
                <p>{i18n.tr(`Gi Verevon tilgang til ${request().host} for å fortsette.`, `Give Verevon access to ${request().host} to continue.`)}</p>
                <div class="global-assistant__approval-actions">
                  <button type="button" disabled={pending() || agentBusy()} onClick={() => { setError(null); props.onDeclineSiteAccess?.() }}>{i18n.tr('Ikke nå', 'Not now')}</button>
                  <button type="button" disabled={pending() || agentBusy()} onClick={() => { void send(true) }}>{i18n.tr('Tillat i 10 minutter', 'Allow for 10 minutes')}</button>
                </div>
              </section>}
            </Show>
            <Show when={!siteAccessRequest() && import.meta.env.VITE_BROWSER_SITES === 'true' && (props.agentView?.errorCode === 'browser_grant_denied' || props.agentView?.errorCode === 'browser_redirect_access_denied')}>
              <a href="/settings/browser-sites" target="_blank" rel="noopener noreferrer">{i18n.tr('Åpne nettsidevalg i en ny fane', 'Open site choices in a new tab')}</a>
            </Show>
            <Show when={props.agentView?.approval}>{(approval) => <section class="global-assistant__approval" aria-label={i18n.tr('Godkjenning', 'Approval')}>
              <strong>{i18n.tr('Agenten trenger din avgjørelse', 'The agent needs your decision')}</strong>
              <p>{approval().action}</p>
              <Show when={approval().destination}><p>{approval().destination}</p></Show>
              <Show when={approval().target}><p>{i18n.tr('Mål', 'Target')}: {approval().target}</p></Show>
              <Show when={approval().reason}><p>{approval().reason}</p></Show>
              <Show when={!approval().id}><p>{i18n.tr('Godkjenningen er ikke tilgjengelig ennå.', 'The approval is not available yet.')}</p></Show>
              <Show when={approval().id && !approval().canApprove}><p>{i18n.tr('Handlingen mangler detaljer og kan ikke godkjennes her.', 'The action lacks details and cannot be approved here.')}</p></Show>
              <Show when={approval().error}><p role="alert">{approval().error}</p></Show>
              <Show when={approval().state === 'submitted'}><p role="status">{i18n.tr('Avgjørelsen er sendt. Venter på bekreftelse …', 'Decision sent. Waiting for confirmation…')}</p></Show>
              <Show when={approval().id && props.onApprovalDecision}>
                <div class="global-assistant__approval-actions">
                  <button type="button" disabled={approval().state !== 'pending' || props.agentView?.status === 'disconnected' || props.agentView?.status === 'stopping'} onClick={() => props.onApprovalDecision?.('reject')}>{i18n.tr('Avslå', 'Reject')}</button>
                  <button type="button" disabled={!approval().canApprove || approval().state !== 'pending' || props.agentView?.status === 'disconnected' || props.agentView?.status === 'stopping'} onClick={() => props.onApprovalDecision?.('approve')}>{i18n.tr('Godkjenn én gang', 'Approve once')}</button>
                </div>
              </Show>
            </section>}</Show>
            <Show when={props.agentView?.status === 'completed'}>
              <Show when={!props.agentView?.resultLoading} fallback={<p role="status">{i18n.tr('Henter resultat …', 'Loading result…')}</p>}>
                <Show when={props.agentView?.result} fallback={<p role="status">{i18n.tr('Et resultat er ikke tilgjengelig i denne visningen.', 'A result is not available in this view.')}</p>}>
                  {(result) => <p class="global-assistant__result">{result()}</p>}
                </Show>
              </Show>
            </Show>
            <Show when={props.agentView?.action}><p role="status">{props.agentView?.action}</p></Show>
          </div>
        </Show>}>
          <div class="global-assistant__agent" aria-label={i18n.tr('Navigering', 'Navigation')}>
            <div class="global-assistant__agent-heading">
              <strong role="status">{props.navigationAgent?.status === 'thinking'
                ? i18n.tr('Finner siden …', 'Finding the page…')
                : props.navigationAgent?.status === 'guiding'
                  ? i18n.tr('Viser vei', 'Showing the way')
                  : props.navigationAgent?.status === 'stopped'
                    ? i18n.tr('Avbrutt lokalt.', 'Stopped locally.')
                    : i18n.tr('Kunne ikke fullføre forespørselen.', 'Could not complete the request.')}</strong>
              <Show when={navigationBusy() && props.onStopNavigation}>
                <button type="button" onClick={() => props.onStopNavigation?.()}>{i18n.tr('Stopp', 'Stop')}</button>
              </Show>
            </div>
            <Show when={props.navigationAgent?.error}><p class="global-assistant__task-error" role="alert">{props.navigationAgent?.error}</p></Show>
          </div>
        </Show>
        <Show when={props.continuation}>{tab =>
          <div class="global-assistant__tab-scope">
            <span>{i18n.tr('Fortsett i denne fanen', 'Continue in this tab')} · {new URL(tab().origin).host}</span>
            <button type="button" disabled={pending() || agentBusy()} onClick={() => props.onCancelContinuation?.()}>
              {i18n.tr('Fjern fanevalg', 'Clear tab selection')}
            </button>
          </div>
        }</Show>
        <form class="global-assistant__task-form" onSubmit={(event) => { event.preventDefault(); void send() }}>
          <label class="sr-only" for="verevon-assistant-input">{i18n.tr('Oppgave til Verevon', 'Task for Verevon')}</label>
          <textarea ref={taskInput} id="verevon-assistant-input" class="global-assistant__task-input" rows={expanded() ? 3 : 1}
            aria-label={i18n.tr('Oppgave til Verevon', 'Task for Verevon')}
            placeholder={props.continuation
              ? i18n.tr('Hva skal Verevon gjøre i fanen?', 'What should Verevon do in this tab?')
              : i18n.tr('Hva skal jeg hjelpe deg med å gjøre?', 'What would you like me to help you do?')}
            value={props.message} onInput={(event) => props.onMessageChange(event.currentTarget.value)}
            onKeyDown={(event) => { if (event.key === 'Enter' && !event.shiftKey && !event.isComposing) { event.preventDefault(); void send() } }} />
          <button type="submit" class="global-assistant__send" disabled={!props.message.trim() || pending() || agentBusy()}
            aria-label={i18n.tr('Start oppgave', 'Start task')} title={i18n.tr('Start oppgave', 'Start task')}><ArrowUp size={16} /></button>
        </form>
        <Show when={props.navigationAgent?.status !== 'error' && !props.agentView?.error ? error() : null}>{(message) => <p class="global-assistant__task-error" role="alert">{message()}</p>}</Show>
      </Show>
    </section>
  )
}
