import { ApiError, hasErrorCode, requestJson } from '@/shared/api/http'
import { getRun } from '@/shared/api/runs-client'
import { decideApproval } from '@/shared/api/orchestration-client'
import { closeBrowserSession, createBrowserSession, listBrowserTabs } from '@/shared/api/browser-client'
import { explainBrowserSite, setMyBrowserSiteChoice } from '@/shared/api/browser-sites-client'
import { controlBrowserAiRun, createAiRunStartKeyring, isAmbiguousAiRunStartFailure, startBrowserAiRun, type BrowserTabContinuation } from '@/shared/api/browser-run-client'
import { listRunEventReplay, streamRunEvents, type BrowserActionApprovalRequiredEvent, type BrowserActionDecidedEvent, type BrowserAttachSharedJarEvent, type BrowserObservationReceivedEvent, type RunEventHandlers } from '@/shared/api/run-console-client'
import { browserTerminalFailureCopy } from '@/features/chat/lib/browser-terminal-copy'
import type { AssistantAgentView } from '@/features/core/components/GlobalAssistant'
import { LocalizedError, translateApiError } from '@/shared/i18n/errors'

type Scope = { userId: string; orgId: string; zdr?: boolean }
type Capability = { available: boolean; policySupported: boolean; canAttach: boolean }
type Dependencies = {
  capability: () => Promise<Capability>
  createSession: typeof createBrowserSession
  closeSession: typeof closeBrowserSession
  listTabs: typeof listBrowserTabs
  explainSite: typeof explainBrowserSite
  setSiteChoice: typeof setMyBrowserSiteChoice
  startRun: typeof startBrowserAiRun
  control: typeof controlBrowserAiRun
  stream: typeof streamRunEvents
  replay: typeof listRunEventReplay
  getRun: typeof getRun
  decideApproval: typeof decideApproval
}
const defaults: Dependencies = {
  capability: () => requestJson('/api/v1/browser/assistant/capabilities'),
  createSession: createBrowserSession,
  closeSession: closeBrowserSession,
  listTabs: listBrowserTabs,
  explainSite: explainBrowserSite,
  setSiteChoice: setMyBrowserSiteChoice,
  startRun: startBrowserAiRun,
  control: controlBrowserAiRun,
  stream: streamRunEvents,
  replay: listRunEventReplay,
  getRun,
  decideApproval,
}
const empty = (): AssistantAgentView => ({ available: false, kind: 'browser', status: 'idle', goal: '', error: null, errorCode: null, siteAccessRequest: null, siteHandoffUrl: null, siteHandoffReady: false, action: null, sessionId: null, runId: null, receipt: null, browserClaimed: false, result: null, resultLoading: false, approval: null })
type BorrowedTab = BrowserTabContinuation & { sessionId: string }
type Active = { scope: Scope; goal: string; target: string; borrowedTab?: BorrowedTab; redirected: boolean; cancelled: boolean; uncertain: boolean; startKey: string | null; sessionId: string | null; runId: string | null; planId: string | null; abort: AbortController; stopSent: boolean; replayTimer?: ReturnType<typeof setTimeout> }
type SiteAccessRequest = { scope: Scope; goal: string; target: string; host: string }
const siteAccessLifetimeMs = 10 * 60 * 1000
const siteHandoffCopy = {
  no: 'Nettstedet krever at du åpner siden selv. Du kan fortsette i din egen nettleser.',
  en: 'The site requires you to open the page yourself. You can continue in your own browser.',
}
const siteHandoffMessage = (tr: (no: string, en: string) => string) => tr(siteHandoffCopy.no, siteHandoffCopy.en)

/** Only explicit HTTP(S) targets may become an agent browser session. */
export function assistantBrowserTarget(raw: string): URL | null {
  try {
    const url = new URL(raw)
    return (url.protocol === 'https:' || url.protocol === 'http:') && url.hostname && !url.username && !url.password ? url : null
  } catch { return null }
}

function requestedSiteHost(error: unknown): string | null {
  if (!(error instanceof ApiError) || Array.isArray(error.details) || error.details?.canRequestAccess !== true) return null
  const host = error.details.host
  return typeof host === 'string' && host.length <= 253 && /^(?:[a-z\d-]+\.)+[a-z\d-]+$/.test(host) ? host : null
}

function actionLabel(kind: string | undefined, tr: (no: string, en: string) => string): string {
  switch (kind?.toLowerCase()) {
    case 'navigate': return tr('Agenten åpner en side.', 'The agent is opening a page.')
    case 'click': case 'click_point': return tr('Agenten klikker.', 'The agent is clicking.')
    case 'scroll': case 'mouse_wheel': return tr('Agenten ruller på siden.', 'The agent is scrolling.')
    case 'type': return tr('Agenten skriver.', 'The agent is typing.')
    case 'screenshot': return tr('Agenten ser på siden.', 'The agent is viewing the page.')
    case 'wait': case 'wait_for': return tr('Agenten venter på siden.', 'The agent is waiting for the page.')
    default: return tr('Agenten utfører et nettlesersteg.', 'The agent is performing a browser step.')
  }
}

// Terminal replay exposes closed failure codes, never raw upstream details.
// Unknown values stay generic rather than displaying untrusted server text.
function terminalFailureMessage(code: unknown, tr: (no: string, en: string) => string): string | null {
  return browserTerminalFailureCopy(code, tr)
}

function approvalAction(kind: string | undefined, category: string | undefined, tr: (no: string, en: string) => string): string {
  switch (category) {
    case 'login': return tr('Logge inn.', 'Sign in.')
    case 'checkout': return tr('Fortsette til betaling eller bestilling.', 'Continue with checkout or an order.')
    case 'posting_form': return tr('Sende inn et skjema eller innhold.', 'Submit a form or content.')
    case 'destructive': return tr('Utføre en handling som kan slette eller endre data.', 'Perform an action that may delete or change data.')
    case 'cross_domain_navigation': return tr('Åpne en annen nettside.', 'Open a different website.')
    case 'persistent_cookie_use': return tr('Bruke en lagret nettleserprofil.', 'Use a saved browser profile.')
    default: return actionLabel(kind, tr)
  }
}

function approvalDestination(raw: string | undefined): string | null {
  if (!raw) return null
  try { const url = new URL(raw); return (url.protocol === 'https:' || url.protocol === 'http:') ? `${url.origin}${url.pathname}`.slice(0, 500) : null } catch { return null }
}

/** One task, one scoped private browser. The server, not this controller,
 * must enforce the restricted policy and authorize its frame stream. */
export function createAssistantBrowserAgent(options: {
  scope: () => Scope | null
  onChange: (view: AssistantAgentView) => void
  tr?: (no: string, en: string) => string
  dependencies?: Partial<Dependencies>
}) {
  const api = { ...defaults, ...options.dependencies }
  const tr = options.tr ?? ((no: string) => no)
  const keyring = createAiRunStartKeyring()
  let view = empty()
  let active: Active | null = null
  let disposed = false
  let generation = 0
  let retained: { scope: Scope; sessionId: string; timer: ReturnType<typeof setTimeout> } | null = null
  let siteAccessRequest: SiteAccessRequest | null = null
  let siteAccessOperation: AbortController | null = null
  const emit = (patch: Partial<AssistantAgentView>) => { view = { ...view, ...patch }; if (!disposed) options.onChange(view) }
  const clearSiteAccessRequest = () => {
    siteAccessOperation?.abort()
    siteAccessOperation = null
    siteAccessRequest = null
  }
  const siteAccessRequestIsCurrent = (request: SiteAccessRequest) => {
    const scope = options.scope()
    return !disposed && !active && siteAccessRequest === request && scope?.userId === request.scope.userId &&
      scope.orgId === request.scope.orgId && scope.zdr === request.scope.zdr
  }
  const releaseRetained = () => {
    const previous = retained
    if (!previous) return
    retained = null
    clearTimeout(previous.timer)
    void api.closeSession(previous.scope.orgId, previous.sessionId).catch(() => undefined)
  }
  // Once the owner has taken control, browser expiry belongs to the normal
  // human session and its keepalive, not the short snapshot timer.
  const claimRetained = (sessionId: string) => {
    const scope = options.scope()
    if (!retained || retained.sessionId !== sessionId || scope?.userId !== retained.scope.userId ||
      scope.orgId !== retained.scope.orgId || (view.status !== 'completed' && !view.siteHandoffReady)) return false
    clearTimeout(retained.timer)
    if (!view.browserClaimed) emit({ browserClaimed: true })
    return true
  }
  const current = (run: Active, gen: number) => {
    const scope = options.scope()
    return !disposed && active === run && generation === gen && scope?.userId === run.scope.userId && scope.orgId === run.scope.orgId
  }
  const requestStop = async (run: Active, gen: number) => {
    if (!run.runId || run.stopSent) return
    run.stopSent = true
    try {
      await api.control(run.scope.orgId, run.runId, 'stop')
      // Acceptance of the request is not proof the agent stopped.
    } catch {
      run.stopSent = false
      if (current(run, gen)) emit({ status: 'disconnected', error: tr('Kunne ikke bekrefte stopp. Oppgaven kan fortsatt være aktiv.', 'Could not confirm a stop. The task may still be active.') })
    }
  }
  const stop = () => {
    const run = active
    if (!run) return
    run.cancelled = true
    if (run.uncertain && !run.runId) {
      emit({ status: 'disconnected', error: tr('Kjøringen må først gjenfinnes før den kan stoppes.', 'The run must be recovered before it can be stopped.') })
      return
    }
    emit({ status: 'stopping', stopRequested: true, error: null })
    void requestStop(run, generation)
  }
  const reset = () => {
    clearSiteAccessRequest()
    releaseRetained()
    const run = active
    if (run) { run.cancelled = true; void requestStop(run, generation); run.abort.abort(); if (run.replayTimer) clearTimeout(run.replayTimer) }
    active = null; generation += 1
    view = empty(); if (!disposed) options.onChange(view)
  }
  const dispose = () => { disposed = true; reset() }

  async function send(goal: string, target: string, borrowedTab?: BorrowedTab): Promise<void> {
    const scope = options.scope()
    const url = assistantBrowserTarget(target)
    const text = goal.trim()
    if (!scope?.orgId || !scope.userId) throw new LocalizedError({ no: 'Ingen innlogget arbeidsflate er tilgjengelig.', en: 'No authenticated workspace is available.' })
    if (!url || !text || text.length > 2000) throw new LocalizedError({ no: 'Oppgi en oppgave og en gyldig nettadresse.', en: 'Provide a task and a valid web address.' })
    if (borrowedTab && borrowedTab.expectedOrigin !== url.origin) throw new LocalizedError({ no: 'Fanens adresse er endret.', en: 'The tab address has changed.' })
    const retrying = Boolean(active?.uncertain && !active.runId && active.goal === text && active.target === url.href && active.scope.userId === scope.userId && active.scope.orgId === scope.orgId && active.borrowedTab === borrowedTab)
    if (active && !retrying) throw new LocalizedError({ no: 'En oppgave er allerede i gang.', en: 'A task is already running.' })
    clearSiteAccessRequest()
    if (!retrying) releaseRetained()
    const run: Active = retrying ? active! : { scope: { ...scope }, goal: text, target: url.href, borrowedTab, redirected: false, cancelled: false, uncertain: false, startKey: null, sessionId: borrowedTab?.sessionId ?? null, runId: null, planId: null, abort: new AbortController(), stopSent: false }
    active = run
    const gen = retrying ? generation : ++generation
    if (retrying) emit({ status: 'starting', error: null })
    else emit({ ...empty(), status: 'starting', goal: text, usesExistingTab: Boolean(borrowedTab), borrowedTabId: borrowedTab?.tabId, stopRequested: false })
    let startSent = retrying
    try {
      if (!retrying) {
        const cap = await api.capability()
        if (!current(run, gen) || run.cancelled) { if (current(run, gen)) emit({ status: 'stopped' }); return }
        if (!cap.available || !cap.policySupported || !cap.canAttach) throw new LocalizedError({ no: 'Den tilkoblede nettleseragenten er ikke tilgjengelig med nødvendige begrensninger.', en: 'The connected browser agent is unavailable with the required restrictions.' })
        emit({ available: true })
        if (!run.borrowedTab) {
          const session = await api.createSession(scope.orgId, { url: run.target, purpose: 'assistant_task', persistentProfile: false, viewport: { width: 1280, height: 800 }, ...(scope.zdr === true ? { zdr: true } : {}) })
          run.sessionId = session.session.id
          if (!run.sessionId) throw new LocalizedError({ no: 'Nettleserøkten mangler identitet.', en: 'The browser session has no identity.' })
          if (!current(run, gen) || run.cancelled) {
            void api.closeSession(scope.orgId, run.sessionId).catch(() => undefined)
            if (current(run, gen)) emit({ status: 'stopped' })
            return
          }
          if (session.resolvedStartUrl !== undefined) {
            const resolved = assistantBrowserTarget(session.resolvedStartUrl)
            if (!resolved) throw new LocalizedError({ no: 'Nettleseren returnerte en ugyldig startadresse.', en: 'The browser returned an invalid start address.' })
            run.redirected = resolved.href !== run.target
            run.target = resolved.href
          }
          if (session.observation?.challenge?.strength === 'strong') {
            emit({ siteHandoffUrl: run.target })
            throw new LocalizedError(siteHandoffCopy)
          }
        }
        emit({ sessionId: run.sessionId })
      }
      if (!run.sessionId) throw new LocalizedError({ no: 'Nettleserøkten mangler identitet.', en: 'The browser session has no identity.' })
      const startUrl = new URL(run.target)
      const agentGoal = run.redirected || run.borrowedTab
        ? tr(`${text}\n\nNettleseren er allerede åpen på ${startUrl.origin}. Fortsett fra siden som er åpen.`, `${text}\n\nThe browser is already open on ${startUrl.origin}. Continue from the current page.`)
        : text
      const params = {
        goal: agentGoal,
        ...(run.borrowedTab
          ? { continueInTab: { tabId: run.borrowedTab.tabId, expectedEpoch: run.borrowedTab.expectedEpoch, expectedOrigin: run.borrowedTab.expectedOrigin } }
          : { allowedDomains: [startUrl.hostname] }),
        requireAttached: true, assistantInteractive: true, maxSteps: 20, maxRuntimeSeconds: 120,
      }
      startSent = true
      run.startKey ??= keyring.keyFor(run.sessionId, params)
      const started = await api.startRun(scope.orgId, run.sessionId, params, undefined, { idempotencyKey: run.startKey })
      if (!started.runId || !started.planId) throw new LocalizedError({ no: 'Agentkjøringen mangler identitet.', en: 'The agent run has no identity.' })
      run.uncertain = false
      keyring.succeeded()
      run.runId = started.runId; run.planId = started.planId
      if (!current(run, gen) || run.cancelled) void requestStop(run, gen)
      if (!current(run, gen)) return
      emit({ runId: started.runId, status: run.cancelled ? 'stopping' : 'starting' })
      const matches = (event: { runId?: string; planId?: string }) => (!event.runId || event.runId === run.runId) && (!event.planId || event.planId === run.planId)
      let blockedUrl: string | null = null
      const noteSiteBlock = (event: BrowserObservationReceivedEvent) => {
        if (!current(run, gen) || event.runId !== run.runId || event.planId !== run.planId || event.status !== 'site_blocked') return false
        const page = event.pageUrl ? assistantBrowserTarget(event.pageUrl) : null
        blockedUrl = page?.origin === startUrl.origin ? page.href : run.target
        return true
      }
      const decidedApprovals = new Set<string>()
      const approvalRequired = (event: BrowserActionApprovalRequiredEvent) => {
        if (!current(run, gen) || event.runId !== run.runId || event.planId !== run.planId) return
        const id = event.approvalId && /^[A-Za-z0-9_.:-]{1,256}$/.test(event.approvalId) ? event.approvalId : null
        if (id && (decidedApprovals.has(id) || view.approval?.id === id)) return
        const destination = approvalDestination(event.url)
        const target = event.selector?.trim().slice(0, 200) || null
        const category = event.riskCategory ?? ''
        const canApprove = category === 'persistent_cookie_use' || (category === 'cross_domain_navigation' && Boolean(destination)) || (['login', 'checkout', 'posting_form', 'destructive'].includes(category) && Boolean(destination && target && event.actionType))
        emit({ approval: { id, actionId: event.actionId ?? null, action: approvalAction(event.actionType, event.riskCategory, tr), destination, target, reason: event.reason?.trim().slice(0, 500) || null, canApprove, state: 'pending', error: null } })
      }
      const approvalDecided = (event: BrowserActionDecidedEvent) => {
        if (!current(run, gen) || event.runId !== run.runId || event.planId !== run.planId || !event.approvalId || !['granted', 'denied', 'timed_out'].includes(event.decision ?? '')) return
        decidedApprovals.add(event.approvalId)
        if (view.approval?.id !== event.approvalId) return
        emit({ approval: null, action: event.decision === 'granted' ? tr('Godkjenningen er bekreftet. Venter på agenten.', 'Approval confirmed. Waiting for the agent.') : tr('Avgjørelsen er bekreftet. Handlingen er ikke godkjent.', 'Decision confirmed. The action was not approved.') })
      }
      const acceptReceipt = (receipt: BrowserAttachSharedJarEvent) => {
        if (
          !current(run, gen) ||
          receipt.runId !== run.runId || receipt.planId !== run.planId ||
          receipt.sessionRunId !== run.sessionId || !receipt.tabId || !receipt.attachId ||
          (run.borrowedTab && receipt.tabId !== run.borrowedTab.tabId)
        ) return
        const status = view.status === 'disconnected' ? 'disconnected' : run.cancelled ? 'stopping' : 'running'
        const previous = view.receipt
        // Live events and replay can carry the same receipt. Keep its identity
        // so the browser view does not reopen the stream and lose its cursor.
        if (
          previous &&
          previous.runId === receipt.runId && previous.planId === receipt.planId &&
          previous.sessionRunId === receipt.sessionRunId &&
          previous.attachId === receipt.attachId && previous.tabId === receipt.tabId
        ) {
          if (status !== view.status) emit({ status })
          return
        }
        emit({ receipt, status })
      }
      const finish = (status: 'completed' | 'failed' | 'stopped', failureCode?: unknown) => {
        if (!current(run, gen)) return
        const siteHandoffUrl = status === 'failed' ? blockedUrl : null
        const error = status === 'failed' ? siteHandoffUrl ? siteHandoffMessage(tr) : terminalFailureMessage(failureCode, tr) : null
        emit({ status, error, siteHandoffUrl, action: null, approval: null, result: null, resultLoading: status === 'completed' })
        active = null
        run.abort.abort()
        if (run.replayTimer) clearTimeout(run.replayTimer)
        const receipt = view.receipt
        const checkHandoff = status === 'failed' && Boolean(siteHandoffUrl) && !run.cancelled && !run.scope.zdr
        if (!run.borrowedTab && run.sessionId && receipt && (status === 'completed' || checkHandoff)) {
          const sessionId = run.sessionId
          const clearBrowser = () => emit({ sessionId: null, receipt: null, siteHandoffReady: false })
          const timer = setTimeout(() => {
            if (!retained || retained.sessionId !== sessionId) return
            releaseRetained()
            if (!disposed && generation === gen && view.runId === started.runId) clearBrowser()
          }, 120000)
          retained = { scope: run.scope, sessionId, timer }
          if (checkHandoff) {
            const stillRetained = () => {
              const scope = options.scope()
              return !disposed && generation === gen && retained?.sessionId === sessionId &&
                scope?.userId === run.scope.userId && scope.orgId === run.scope.orgId
            }
            const discard = () => {
              if (!stillRetained()) return
              releaseRetained()
              clearBrowser()
            }
            // A replay signal can explain the failure, but only Quarry's owner
            // projection can confirm that it actually returned this tab.
            void api.listTabs(run.scope.orgId, sessionId).then(({ session, tabs }) => {
              if (!stillRetained()) return
              const tab = tabs.find((candidate) => candidate.tabId === receipt.tabId)
              if (session?.id !== sessionId || session.status !== 'live' || tab?.holder !== 'human') {
                discard()
                return
              }
              emit({
                siteHandoffReady: true,
                error: tr('Nettstedet ber om manuell tilgang. Ta over fanen i Nett, eller åpne siden i din egen nettleser.', 'The site requests manual access. Take over the tab in Web or open the page in your own browser.'),
              })
            }).catch(discard)
          }
        } else if (!run.borrowedTab && run.sessionId) void api.closeSession(run.scope.orgId, run.sessionId).catch(() => undefined)
        if (status === 'completed') {
          // Replay deliberately carries no answer. Fetch an owner-authorized
          // output only if one was actually recorded for this very run.
          void api.getRun(started.runId).then((result) => {
            const currentScope = options.scope()
            if (disposed || generation !== gen || view.runId !== started.runId || view.status !== 'completed' || currentScope?.userId !== run.scope.userId || currentScope.orgId !== run.scope.orgId) return
            const text = result?.runId === started.runId && result.status.toLowerCase() === 'completed' ? result.finalOutput?.trim() : undefined
            emit({ result: text ? (text.length > 12000 ? `${text.slice(0, 12000)}…` : text) : null, resultLoading: false })
          }).catch(() => {
            const currentScope = options.scope()
            if (!disposed && generation === gen && view.runId === started.runId && view.status === 'completed' && currentScope?.userId === run.scope.userId && currentScope.orgId === run.scope.orgId) emit({ result: null, resultLoading: false })
          })
        }
      }
      const handlers: RunEventHandlers = {
        onBrowserAttachSharedJar: acceptReceipt,
        onBrowserAction: (event) => { if (current(run, gen) && matches(event)) emit({ action: actionLabel(event.actionType, tr) }) },
        onBrowserObservation: (event) => {
          const siteBlocked = noteSiteBlock(event)
          if (current(run, gen) && matches(event)) emit({ action: siteBlocked
            ? tr('Nettstedet krever manuell tilgang.', 'The site requires manual access.')
            : tr('Agenten undersøker siden.', 'The agent is inspecting the page.') })
        },
        onBrowserActionApprovalRequired: approvalRequired,
        onBrowserActionDecided: approvalDecided,
        onError: () => { if (current(run, gen)) emit({ status: 'disconnected', error: tr('Forbindelsen ble brutt. Oppgaven kan fortsatt være aktiv.', 'Connection lost. The task may still be active.') }) },
        onDone: () => { if (current(run, gen)) emit({ status: 'disconnected', error: tr('Aktivitetsstrømmen ble avsluttet. Oppgaven kan fortsatt være aktiv.', 'The activity stream ended. The task may still be active.') }) },
      }
      void api.stream(started.runId, handlers, run.abort.signal).catch(() => {
        if (current(run, gen)) emit({ status: 'disconnected', error: tr('Forbindelsen ble brutt. Oppgaven kan fortsatt være aktiv.', 'Connection lost. The task may still be active.') })
      })
      // The live stream starts at the tail and may miss an early attachment or
      // terminal event. Replay is owner-authorized and only identifiers/status
      // are used; lack of a receipt never authorizes a browser frame.
      let afterEventId: string | undefined
      const poll = async () => {
        if (!current(run, gen)) return
        try {
          const page = await api.replay(started.runId, { afterEventId, limit: 100 })
          if (!current(run, gen) || page.runId !== started.runId) return
          for (const event of page.events) {
            const p = event.payload
            if (event.eventType === 'EVENT_TYPE_BROWSER_ATTACH_SHARED_JAR') {
              const values = [p.run_id, p.plan_id, p.attach_id, p.session_run_id, p.tab_id]
              if (values.every(v => typeof v === 'string' && v.length > 0 && v.length <= 256 && /^[A-Za-z0-9_.:-]+$/.test(v)))
                acceptReceipt({ runId: p.run_id as string, planId: p.plan_id as string, attachId: p.attach_id as string, sessionRunId: p.session_run_id as string, tabId: p.tab_id as string })
            }
            if (event.eventType === 'EVENT_TYPE_BROWSER_ACTION_APPROVAL_REQUIRED') approvalRequired({ runId: typeof p.run_id === 'string' ? p.run_id : undefined, planId: typeof p.plan_id === 'string' ? p.plan_id : undefined, approvalId: typeof p.approval_id === 'string' ? p.approval_id : undefined, actionId: typeof p.action_id === 'string' ? p.action_id : undefined, actionType: typeof p.action_type === 'string' ? p.action_type : undefined, riskCategory: typeof p.risk_category === 'string' ? p.risk_category : undefined, selector: typeof p.selector === 'string' ? p.selector : undefined, url: typeof p.url === 'string' ? p.url : undefined, reason: typeof p.reason === 'string' ? p.reason : undefined })
            if (event.eventType === 'EVENT_TYPE_BROWSER_ACTION_DECIDED') approvalDecided({ runId: typeof p.run_id === 'string' ? p.run_id : undefined, planId: typeof p.plan_id === 'string' ? p.plan_id : undefined, approvalId: typeof p.approval_id === 'string' ? p.approval_id : undefined, decision: typeof p.decision === 'string' ? p.decision : undefined })
            if (event.eventType === 'EVENT_TYPE_BROWSER_OBSERVATION_RECEIVED' && p.run_id === run.runId && p.plan_id === run.planId && p.status === 'site_blocked') {
              noteSiteBlock({ runId: run.runId ?? undefined, planId: run.planId ?? undefined, status: p.status, pageUrl: typeof p.page_url === 'string' ? p.page_url : undefined })
            }
            if (p.run_id === run.runId && p.terminal === true) {
              if (event.eventType === 'EVENT_TYPE_RUN_COMPLETED' && p.status === 'completed') finish('completed')
              else if (event.eventType === 'EVENT_TYPE_RUN_FAILED' && p.status === 'failed') finish('failed', p.failure_code)
              else if (event.eventType === 'EVENT_TYPE_RUN_CANCELLED' && p.status === 'cancelled') finish('stopped')
            }
          }
          afterEventId = page.nextEventId ?? afterEventId
        } catch { /* Replay may be unavailable; keep the live stream and never guess a receipt. */ }
        if (current(run, gen)) run.replayTimer = setTimeout(() => { void poll() }, 3000)
      }
      void poll()
    } catch (error) {
      if (!current(run, gen)) return
      if (startSent && (run.uncertain || isAmbiguousAiRunStartFailure(error))) {
        run.uncertain = true
        emit({ status: 'disconnected', error: tr('Starten kunne ikke bekreftes. En oppgave kan fortsatt være aktiv.', 'The start could not be confirmed. A task may still be active.') })
      } else {
        keyring.failed(error)
        if (!run.borrowedTab && run.sessionId) void api.closeSession(scope.orgId, run.sessionId).catch(() => undefined)
        active = null
        const siteAccessRefused = hasErrorCode(error, 'browser_grant_denied')
        const redirectAccessRefused = hasErrorCode(error, 'browser_redirect_access_denied')
        const navigationTimedOut = error instanceof ApiError && error.status === 504 &&
          error.code === 'TIMEOUT' && !Array.isArray(error.details) && error.details?.op === 'navigate'
        const browserStartUnavailable = !startSent && !run.cancelled &&
          (navigationTimedOut || hasErrorCode(error, 'upstream_unavailable'))
        const siteHandoffUrl = !startSent && !run.cancelled
          ? view.siteHandoffUrl ?? (browserStartUnavailable || hasErrorCode(error, 'browser_site_blocked') ? run.target : null)
          : null
        const host = (siteAccessRefused || redirectAccessRefused) && !startSent && !run.sessionId && !run.cancelled ? requestedSiteHost(error) : null
        siteAccessRequest = host ? { scope: run.scope, goal: text, target: url.href, host } : null
        emit({
          status: 'failed',
          siteHandoffUrl,
          siteAccessRequest: host ? { host } : null,
          errorCode: browserStartUnavailable ? 'browser_start_unavailable' : redirectAccessRefused ? 'browser_redirect_access_denied' : siteAccessRefused ? 'browser_grant_denied' : null,
          error: browserStartUnavailable
            ? navigationTimedOut
              ? tr('Siden ble ikke lastet innen fristen. Prøv igjen, eller åpne den i din egen nettleser.', 'The page did not load in time. Try again or open it in your own browser.')
              : tr('Nettlesertjenesten kunne ikke åpne siden. Du kan åpne den direkte i din egen nettleser.', 'The browser service could not open the page. You can open it directly in your own browser.')
            : siteHandoffUrl ? siteHandoffMessage(tr) : host ? null : redirectAccessRefused
            ? tr('Nettstedet sender deg videre til en adresse agenten ikke har tilgang til. Sjekk nettsidevalgene. Utkastet er beholdt.', 'The site redirects to an address the agent cannot access. Check your site choices. Your draft has been kept.')
            : siteAccessRefused
            ? tr('Nettlesertilgang ble ikke innvilget. Kontroller nettsidevalgene og eventuelle begrensninger fra administratoren. Utkastet er beholdt.', 'Browser access was not granted. Review your site choices and any administrator restrictions. Your draft has been kept.')
            : translateApiError(error, tr, { no: 'Agentoppgaven kunne ikke startes.', en: 'The agent task could not be started.' }),
        })
      }
      throw error
    } finally {
      if (current(run, gen) && run.cancelled && !startSent) active = null
    }
  }
  const retry = async () => {
    if (!active?.uncertain || active.runId) throw new LocalizedError({ no: 'Ingen usikker start å gjenoppta.', en: 'No uncertain start to recover.' })
    return send(active.goal, active.target, active.borrowedTab)
  }
  const allowSiteAccess = async () => {
    const request = siteAccessRequest
    const unavailableCopy = { no: 'Kunne ikke bekrefte nettstedstilgangen. Utkastet er beholdt.', en: 'Could not confirm site access. Your draft has been kept.' }
    const unavailable = tr(unavailableCopy.no, unavailableCopy.en)
    if (!request || siteAccessOperation || !siteAccessRequestIsCurrent(request)) throw new LocalizedError(unavailableCopy)
    const operation = new AbortController()
    siteAccessOperation = operation
    emit({ error: null })
    try {
      const { scope, host } = request
      const explanation = await api.explainSite(scope.orgId, host, operation.signal)
      if (!siteAccessRequestIsCurrent(request) || explanation?.organization_id !== scope.orgId ||
          explanation.user_id !== scope.userId || explanation.decision?.host !== host) throw new LocalizedError(unavailableCopy)
      if (explanation.decision.effective !== 'act') {
        if (explanation.decision.effective !== 'ask' || explanation.decision.cap !== 'act') throw new LocalizedError(unavailableCopy)
        const expiresAt = new Date(Date.now() + siteAccessLifetimeMs).toISOString()
        await api.setSiteChoice(scope.orgId, host, explanation.revision, 'act', expiresAt, operation.signal)
      }
      if (!siteAccessRequestIsCurrent(request)) throw new LocalizedError(unavailableCopy)
    } catch (error) {
      if (siteAccessRequestIsCurrent(request)) emit({ error: unavailable })
      throw error
    } finally {
      if (siteAccessOperation === operation) siteAccessOperation = null
    }
    await send(request.goal, request.target)
  }
  const declineSiteAccess = () => {
    if (!siteAccessRequest) return
    clearSiteAccessRequest()
    emit({ status: 'idle', goal: '', error: null, errorCode: null, siteAccessRequest: null })
  }
  const decide = async (decision: 'approve' | 'reject') => {
    const run = active
    const approval = view.approval
    const gen = generation
    if (!run || !current(run, gen) || !approval?.id || (decision === 'approve' && !approval.canApprove) || approval.state !== 'pending' || view.status === 'disconnected' || view.status === 'stopping') return
    emit({ approval: { ...approval, state: 'submitting', error: null } })
    try {
      await api.decideApproval(approval.id, decision)
      // A successful HTTP response records a decision, not a resumed action.
      if (current(run, gen) && view.approval?.id === approval.id) emit({ approval: { ...view.approval, state: 'submitted' } })
    } catch {
      if (current(run, gen) && view.approval?.id === approval.id) emit({ approval: { ...view.approval, state: 'pending', error: tr('Kunne ikke registrere avgjørelsen.', 'Could not record the decision.') } })
    }
  }
  return { send, stop, retry, allowSiteAccess, declineSiteAccess, decide, reset, dispose, claimRetained, state: () => view }
}
