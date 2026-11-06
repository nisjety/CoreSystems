import { createEffect, createMemo, createSignal, onCleanup, Show, untrack } from 'solid-js'
import { BrowserChrome } from '@/features/dashboard/home/BrowserChrome'
import { attachBrowserSession, attachBrowserTabs, browserSessionFromPreview } from '@/features/dashboard/home/browser-session'
import type { ScrapePreview } from '@/features/dashboard/home/knowledge-preview'
import { changeBrowserControl, STALE_CONTROL_COPY } from '@/features/browser/dock/browser-control'
import { useAssistantBrowser } from '@/features/core/assistant-agent/assistant-browser-context'
import { browserErrorMessage, createBrowserTab, getBrowserSession, keepAliveBrowserSession, listBrowserTabs, runBrowserAction, selectBrowserTab, type BrowserAction } from '@/shared/api/browser-client'
import { ApiError } from '@/shared/api/http'
import { useI18n } from '@/shared/i18n'
import { getSession } from '@/shared/session/session-store'
import type { BrowserAttachSharedJarEvent } from '@/shared/api/run-console-client'

/** Hand the agent's exact, owner-authorized tab back to its user. */
export function AssistantRetainedBrowser(props: {
  orgId: string
  sessionId: string
  modelRunId: string
  receipt: BrowserAttachSharedJarEvent | null
  claimed?: boolean
  requiresManualAccess?: boolean
  onClaim: (sessionId: string) => boolean
}) {
  const i18n = useI18n()
  const [preview, setPreview] = createSignal<ScrapePreview | null>(null)
  const [owner, setOwner] = createSignal<{ userId: string; orgId: string; sessionId: string; modelRunId: string } | null>(null)
  const canControl = () => { const o = owner(); return Boolean(o && !disposed && getSession().user?.id === o.userId && getSession().activeOrg?.id === o.orgId && props.orgId === o.orgId && props.sessionId === o.sessionId && props.modelRunId === o.modelRunId) }
  const [busy, setBusy] = createSignal(false)
  const assistantBrowser = useAssistantBrowser()
  if (assistantBrowser.registerNettBrowser) {
    onCleanup(assistantBrowser.registerNettBrowser(() => {
      const browser = preview()?.browserSession?.session
      return canControl() && !busy() && browser?.status === 'live' && browser.control?.mode === 'human_takeover' ? owner() : null
    }))
  }
  const [error, setError] = createSignal<string | null>(null)
  let disposed = false
  let refreshGeneration = 0
  let refreshTimers: Array<ReturnType<typeof setTimeout>> = []
  const cancelRefresh = () => { refreshGeneration += 1; refreshTimers.forEach(clearTimeout); refreshTimers = [] }
  onCleanup(() => { disposed = true; cancelRefresh() })
  // A click can resolve before navigation commits. Re-read owner tab metadata
  // without letting an older request overwrite a newer action or tab selection.
  const refreshTabs = () => {
    cancelRefresh()
    const generation = refreshGeneration
    let latest = -1
    for (const [index, delay] of [350, 1500, 4000].entries()) {
      refreshTimers.push(setTimeout(() => {
        void listBrowserTabs(props.orgId, props.sessionId).then(tabs => {
          if (!canControl() || generation !== refreshGeneration || index < latest) return
          latest = index
          setPreview(current => current ? attachBrowserTabs(current, tabs) : current)
        }).catch(() => undefined)
      }, delay))
    }
  }
  const session = createMemo(() => {
    const current = preview()
    return current && canControl() ? browserSessionFromPreview(current) : null
  })
  const message = (reason: unknown) => browserErrorMessage(reason, i18n.tr, {
    no: 'Kunne ikke åpne nettleserøkten fra oppgaven. Prøv igjen.',
    en: 'Could not open the browser session from the task. Try again.',
  })
  const open = async () => {
    const receipt = props.receipt
    const orgId = props.orgId, sessionId = props.sessionId, modelRunId = props.modelRunId
    const userId = getSession().user?.id
    const resume = props.claimed === true
    if (busy() || !orgId || !userId || getSession().activeOrg?.id !== orgId || !receipt || receipt.runId !== modelRunId || receipt.sessionRunId !== sessionId || !receipt.tabId || !receipt.attachId) return
    const sameOwner = () => !disposed && getSession().user?.id === userId && getSession().activeOrg?.id === orgId && props.orgId === orgId && props.sessionId === sessionId && props.modelRunId === modelRunId && props.receipt?.attachId === receipt.attachId && props.receipt?.planId === receipt.planId && props.receipt?.tabId === receipt.tabId && props.receipt?.sessionRunId === sessionId && props.receipt?.runId === modelRunId
    setBusy(true); setError(null)
    try {
      let response = await getBrowserSession(orgId, sessionId)
      if (!sameOwner()) return
      if (response.session.id !== sessionId || response.session.status !== 'live') {
        setError(i18n.tr('Nettleserøkten er ikke lenger tilgjengelig.', 'The browser session is no longer available.'))
        return
      }
      if (resume && response.session.control?.mode !== 'human_takeover') {
        setError(i18n.tr(STALE_CONTROL_COPY.no, STALE_CONTROL_COPY.en))
        return
      }
      // An idempotent owner control POST returns the gateway's complete,
      // authoritative browser session even when the person already has control.
      const outcome = await changeBrowserControl(orgId, sessionId, 'human_takeover', response.session.control?.epoch)
      if (!sameOwner()) return
      if (outcome.stale) {
        if (!disposed) setError(i18n.tr(STALE_CONTROL_COPY.no, STALE_CONTROL_COPY.en))
        return
      }
      response = outcome.response
      if (response.session.id !== sessionId || response.session.control?.mode !== 'human_takeover') throw new Error('browser control unavailable')
      let next = attachBrowserSession({ blocks: [], charCount: 0, description: '', markdown: '', source: 'empty', title: response.session.title, url: response.session.url }, response)
      // Select the agent's tab on first takeover. On return, keep the tab
      // the person last selected and let the owner projection describe it.
      if (!resume) {
        const tabs = await selectBrowserTab(orgId, sessionId, receipt.tabId)
        if (!sameOwner()) return
        if (tabs.session && (tabs.session.id !== sessionId || tabs.session.control?.mode !== 'human_takeover')) throw new Error('browser control unavailable')
        if (!tabs.tabs.some(tab => tab.tabId === receipt.tabId && tab.active)) throw new Error('browser tab unavailable')
        next = attachBrowserTabs(next, tabs)
      }
      if (!sameOwner()) return
      if (!props.onClaim(sessionId)) throw new Error('browser session no longer retained')
      setOwner({ userId, orgId, sessionId, modelRunId })
      setPreview(next)
    } catch (reason) {
      if (!disposed) setError(message(reason))
    } finally { if (!disposed) setBusy(false) }
  }
  const resumeOnMount = untrack(() => props.claimed === true)
  createEffect(() => undefined, () => { if (resumeOnMount) untrack(() => void open()) })
  const act = async (action: BrowserAction) => {
    const current = preview()
    if (!current || busy() || !canControl()) return
    setBusy(true); setError(null)
    try {
      const response = await runBrowserAction(props.orgId, props.sessionId, action, { actor: 'human' })
      if (canControl()) { setPreview(previous => previous ? attachBrowserSession(previous, response) : previous); refreshTabs() }
    } catch (reason) { if (!disposed) setError(message(reason)) }
    finally { if (!disposed) setBusy(false) }
  }
  const select = async (tabId: string) => {
    if (busy() || !canControl()) return
    setBusy(true); setError(null); cancelRefresh()
    try {
      const tabs = await selectBrowserTab(props.orgId, props.sessionId, tabId)
      if (canControl()) setPreview(previous => previous ? attachBrowserTabs(previous, tabs) : previous)
    } catch (reason) { if (!disposed) setError(message(reason)) }
    finally { if (!disposed) setBusy(false) }
  }
  const newTab = async () => {
    if (busy() || !canControl()) return
    setBusy(true); setError(null); cancelRefresh()
    try {
      const tabs = await createBrowserTab(props.orgId, props.sessionId)
      if (canControl()) setPreview(previous => previous ? attachBrowserTabs(previous, tabs) : previous)
    } catch (reason) { if (!disposed) setError(message(reason)) }
    finally { if (!disposed) setBusy(false) }
  }
  const keepAlive = async () => {
    if (!canControl()) return 'expired' as const
    try { await keepAliveBrowserSession(props.orgId, props.sessionId); return 'alive' as const }
    catch (reason) { return reason instanceof ApiError && reason.code === 'browser_session_expired' ? 'expired' as const : 'failed' as const }
  }
  return <div class="home-agent-browser-handoff">
    <Show when={!preview()}>
      <button type="button" class="assistant-live-browser__stop" disabled={busy() || !props.receipt} onClick={() => void open()}>
        {busy() ? i18n.tr('Åpner …', 'Opening…') : props.claimed ? i18n.tr('Fortsett i nettleseren', 'Continue browsing') : props.requiresManualAccess ? i18n.tr('Ta over fanen', 'Take over tab') : i18n.tr('Bruk nettleseren', 'Use browser')}
      </button>
    </Show>
    <Show when={error()}>{text => <p role="alert">{text()}</p>}</Show>
    <Show when={session()}>{current => <BrowserChrome personalBrowsing session={current()} browserBusy={busy()} onBrowserAction={act} onBrowserSelectTab={select} onBrowserNewTab={newTab} onKeepAlive={keepAlive} />}</Show>
  </div>
}

