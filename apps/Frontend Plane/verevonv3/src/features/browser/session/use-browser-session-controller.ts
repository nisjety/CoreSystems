// Signals are read through latest(): a read in the same tick as a write sees that write.
// Solid 2 otherwise exposes the last committed value until its queue flushes (D2-175).
import { createSignal } from '@/shared/lib/create-immediate-signal'
import {
  browserErrorMessage,
  closeBrowserSession,
  closeBrowserTab,
  createBrowserSession,
  createBrowserTab,
  getBrowserOwnerTimeline,
  runBrowserAction,
  selectBrowserTab,
  type BrowserAction,
  type BrowserActionActor,
  type BrowserControlMode,
  type BrowserObservation,
  type BrowserProfileSummary,
  type BrowserSession,
  type BrowserTabsResponse,
  type CreateBrowserSessionRequest,
} from '@/shared/api/browser-client'
import { changeBrowserControl, STALE_CONTROL_COPY } from '@/features/browser/dock/browser-control'
import { gatewayBaseUrl } from '@/shared/api/config'
import { createAiRunStartKeyring, startBrowserAiRun } from '@/shared/api/browser-run-client'
import { streamRunEvents } from '@/shared/api/run-console-client'
import { decideApproval as decideApprovalRequest, listApprovals, type ApprovalDecision } from '@/shared/api/orchestration-client'
import {
  createBrowserLoopController,
  type BrowserLoopController,
  type BrowserLoopState,
} from '@/features/dashboard/home/browser-loop'
import {
  attachBrowserObservation,
  attachBrowserOwnerTimeline,
  attachBrowserSession,
  attachBrowserTabs,
  withBrowserApprovalDecided,
  withBrowserApprovalRequested,
  withBrowserOwnerTimelineError,
  type BrowserApprovalEntry,
  type BrowserSessionViewModel,
  type BrowserStepRationale,
} from '@/features/dashboard/home/browser-session'
import type { ScrapePreview } from '@/features/dashboard/home/knowledge-preview'
import { navigationFailureFromError, type BrowserNavigationFailure } from './browser-navigation-error'
import { emptyBrowserPreview, viewModelOf } from './browser-preview-adapter'

/**
 * Framework-light owner of one browser session for a surface (WS9.F, D9 S4a).
 *
 * It ports the session lifecycle of `KnowledgeComposer.tsx` (create, close,
 * owner timeline, socket observation, the durable AI run and approvals) into a
 * factory that can be driven without the composer. The composer is not changed
 * by this slice; a later cleanup moves its Crawl composer onto this controller.
 *
 * Rules kept from the composer:
 * - ZDR: `open` sends `zdr: true` and never a profile field, whatever the picker
 *   shows. The gateway enforces the same rule independently.
 * - A reply for a session that is no longer the current one is dropped, its
 *   preview, error, approval and busy updates included. A session created by an
 *   `open` that was superseded or closed meanwhile is closed again.
 * - Browser actions are gated by `busy`, so one action runs at a time.
 * - Navigation failures become a `navigationError` card (WS9.B); other failures
 *   become `error`.
 */

export type BrowserSessionControllerOptions = {
  /** The active workspace. Empty or null means no session can start. */
  orgId: () => string | null | undefined
  /** Zero Data Retention on: sessions are created ZDR and never carry a profile. */
  zdr: () => boolean
  /** The profile the picker shows. Ignored while ZDR is on. */
  profileChoice?: () => BrowserProfileSummary | null
  /** Called when the gateway returns a persistent profile the session attached to. */
  onProfileReturned?: (profile: { id: string; scope: BrowserSession['profile']['scope'] }) => void
  /** Translates the caller's Norwegian and English copy. Defaults to English. */
  tr?: (noText: string, enText: string) => string
}

export type BrowserSessionController = {
  /** The preview that carries the session (null when no session is open). */
  preview: () => ScrapePreview | null
  /** The view model the browser panel renders (null when no session is open). */
  view: () => BrowserSessionViewModel | null
  busy: () => boolean
  /** The last non-navigation failure, already translated. */
  error: () => string | null
  /** A failed navigation, kept so the card can offer Retry (WS9.B). */
  navigationError: () => BrowserNavigationFailure | null
  approvals: () => BrowserApprovalEntry[]
  decidingApprovalKeys: () => string[]
  aiRunRationale: () => BrowserStepRationale | null
  /** The state of the capped AI loop (durable server-side run). */
  loop: () => BrowserLoopState
  open: (url: string) => Promise<void>
  close: () => Promise<void>
  runAction: (action: BrowserAction, options?: { actor?: BrowserActionActor }) => Promise<void>
  /** Takes over (`human_takeover`) or hands back control. Pauses a running AI loop on takeover. */
  setControlMode: (mode: BrowserControlMode, options?: { expectedEpoch?: number | null }) => Promise<void>
  newTab: () => Promise<void>
  selectTab: (tabId: string) => Promise<void>
  closeTab: (tabId: string) => Promise<void>
  refreshTimeline: (sessionId?: string) => Promise<void>
  /** Applies an observation pushed over the session socket. */
  applySocketObservation: (observation: BrowserObservation, session?: BrowserSession) => void
  startAiRun: (goal: string) => Promise<void>
  pauseAiRun: () => void
  resumeAiRun: () => void
  stopAiRun: () => void
  decideApproval: (entryKey: string, decision: ApprovalDecision) => Promise<void>
  /** Closes a ZDR session and detaches the page-hide close. Safe to call twice. */
  dispose: () => Promise<void>
}

const BROWSER_SESSION_TIMEOUT_MS = 30_000
const BROWSER_AI_LOOP_MAX_STEPS = 5
const TIMELINE_LIMIT = 100
const VIEWPORT = { width: 1280, height: 800 }

const passThrough = (_noText: string, enText: string) => enText

export function createBrowserSessionController(
  options: BrowserSessionControllerOptions,
): BrowserSessionController {
  const tr = options.tr ?? passThrough

  const [preview, setPreview] = createSignal<ScrapePreview | null>(null)
  const [busy, setBusyState] = createSignal(false)
  const [error, setError] = createSignal<string | null>(null)
  const [navigationError, setNavigationError] = createSignal<BrowserNavigationFailure | null>(null)
  const [approvals, setApprovals] = createSignal<BrowserApprovalEntry[]>([])
  const [decidingApprovalKeys, setDecidingApprovalKeys] = createSignal<string[]>([])
  const [aiRunRationale, setAiRunRationale] = createSignal<BrowserStepRationale | null>(null)
  const [loopState, setLoopState] = createSignal<BrowserLoopState>({ error: null, goal: '', status: 'idle', step: 0 })

  // Synchronous gate for `busy`: a second action must not slip in between the
  // signal write and the next read.
  let busyFlag = false
  // Bumped by every open and by close/dispose. A slow `open` that lost the race
  // is discarded, and its session is closed again.
  let generation = 0
  // Whether the current session was created ZDR (dispose closes those).
  let openedZdr = false
  let aiRunAbort: AbortController | null = null
  const keyring = createAiRunStartKeyring()
  const loop: BrowserLoopController = createBrowserLoopController((state) => setLoopState(() => state))

  const orgIdNow = (): string | null => {
    const id = options.orgId()
    return id ? id : null
  }
  const currentSessionId = (): string | null => preview()?.browserSession?.session.id ?? null

  const setBusy = (value: boolean) => {
    busyFlag = value
    setBusyState(value)
  }

  // Clears busy only while the session that set it is still current. A reply from a
  // replaced session must not clear the gate of the session that is current now.
  const releaseBusy = (sessionId: string) => {
    if (currentSessionId() === sessionId) setBusy(false)
  }

  const closeSessionById = (sessionId: string | null | undefined): Promise<void> => {
    const id = orgIdNow()
    if (!id || !sessionId) return Promise.resolve()
    return closeBrowserSession(id, sessionId).then(
      () => undefined,
      () => undefined,
    )
  }

  const createRequestFor = (url: string): CreateBrowserSessionRequest => {
    const zdr = options.zdr()
    // A ZDR session never carries a profile field. This mirrors the gateway's
    // `reject_zdr_persistent_profile` guard, which rejects it independently.
    const profile = zdr ? null : (options.profileChoice?.() ?? null)
    return {
      ...(zdr ? { zdr: true } : {}),
      ...(profile ? { profileId: profile.profile_id, scope: profile.scope } : {}),
      url,
      viewport: { ...VIEWPORT },
    }
  }

  const refreshTimeline = async (sessionId: string | null = currentSessionId()): Promise<void> => {
    const id = orgIdNow()
    if (!id || !sessionId) return
    try {
      const timeline = await getBrowserOwnerTimeline(id, sessionId, { limit: TIMELINE_LIMIT })
      setPreview((current) => {
        if (!current || current.browserSession?.session.id !== sessionId) return current
        return attachBrowserOwnerTimeline(current, timeline)
      })
    } catch (reason) {
      const message = browserErrorMessage(reason, tr, {
        no: 'Den varige nettleseraktiviteten kunne ikke lastes.',
        en: 'The durable browser activity could not be loaded.',
      })
      setPreview((current) => {
        if (!current || current.browserSession?.session.id !== sessionId) return current
        return withBrowserOwnerTimelineError(current, message)
      })
    }
  }

  const open = async (url: string): Promise<void> => {
    const id = orgIdNow()
    if (!id) {
      setError(tr('Arbeidsområde mangler for nettleserøkt.', 'Workspace is missing for the browser session.'))
      return
    }
    const previousSessionId = currentSessionId()
    generation += 1
    const token = generation
    stopAiRun()
    void closeSessionById(previousSessionId)
    setPreview(null)
    setApprovals([])
    setDecidingApprovalKeys([])
    setAiRunRationale(null)
    setNavigationError(null)
    setError(null)
    setBusy(true)

    const controller = new AbortController()
    const timeout = globalThis.setTimeout(() => controller.abort(), BROWSER_SESSION_TIMEOUT_MS)
    const request = createRequestFor(url)
    try {
      const response = await createBrowserSession(id, request, controller.signal)
      if (token !== generation) {
        // A newer open or a close happened while this one was in flight.
        void closeSessionById(response.session.id)
        return
      }
      openedZdr = request.zdr === true
      const returned = response.session.profile
      if (returned.storage === 'persistent' && returned.id) {
        options.onProfileReturned?.({ id: returned.id, scope: returned.scope })
      }
      setPreview(attachBrowserSession(emptyBrowserPreview(url), response))
      void refreshTimeline(response.session.id)
    } catch (reason) {
      if (token === generation) {
        setError(browserErrorMessage(reason, tr, {
          no: 'Nettleserøkt kunne ikke startes.',
          en: 'Browser session could not be started.',
        }))
      }
    } finally {
      globalThis.clearTimeout(timeout)
      if (token === generation) setBusy(false)
    }
  }

  const close = async (): Promise<void> => {
    generation += 1
    stopAiRun()
    const sessionId = currentSessionId()
    setPreview(null)
    setApprovals([])
    setDecidingApprovalKeys([])
    setAiRunRationale(null)
    setNavigationError(null)
    setError(null)
    openedZdr = false
    setBusy(false)
    await closeSessionById(sessionId)
  }

  const runAction = async (action: BrowserAction, actionOptions?: { actor?: BrowserActionActor }): Promise<void> => {
    const id = orgIdNow()
    const sessionId = currentSessionId()
    if (!id || !sessionId || busyFlag) return
    setBusy(true)
    setError(null)
    try {
      const next = await runBrowserAction(id, sessionId, action, { actor: actionOptions?.actor ?? 'human' })
      setPreview((current) => {
        if (!current || current.browserSession?.session.id !== sessionId) return current
        return attachBrowserSession(current, next)
      })
      setNavigationError(null)
      void refreshTimeline(sessionId)
    } catch (reason) {
      if (currentSessionId() !== sessionId) return
      const failure = navigationFailureFromError(action, reason, tr)
      setNavigationError(failure)
      if (!failure) {
        setError(browserErrorMessage(reason, tr, {
          no: 'Nettleserhandlingen kunne ikke fullføres.',
          en: 'The browser action could not be completed.',
        }))
      }
    } finally {
      releaseBusy(sessionId)
    }
  }

  const setControlMode = async (
    mode: BrowserControlMode,
    options?: { expectedEpoch?: number | null },
  ): Promise<void> => {
    const id = orgIdNow()
    const sessionId = currentSessionId()
    if (!id || !sessionId || busyFlag) return
    if (mode === 'human_takeover') loop.requestPause()
    setBusy(true)
    setError(null)
    try {
      // Compare-and-set on the epoch the person saw (WS9.I). A stale epoch is not retried:
      // the projection is refetched once and the notice says so.
      const expectedEpoch = options?.expectedEpoch ?? preview()?.browserSession?.session.control?.epoch ?? null
      const outcome = await changeBrowserControl(id, sessionId, mode, expectedEpoch)
      setPreview((current) => {
        if (!current || current.browserSession?.session.id !== sessionId) return current
        return attachBrowserSession(current, outcome.response)
      })
      if (outcome.stale && currentSessionId() === sessionId) {
        setError(tr(STALE_CONTROL_COPY.no, STALE_CONTROL_COPY.en))
      }
      void refreshTimeline(sessionId)
    } catch (reason) {
      if (currentSessionId() !== sessionId) return
      setError(browserErrorMessage(reason, tr, {
        no: 'Kunne ikke endre nettleserkontroll.',
        en: 'Could not change browser control.',
      }))
    } finally {
      releaseBusy(sessionId)
    }
  }

  const applyTabs = async (
    run: (id: string, sessionId: string) => Promise<BrowserTabsResponse>,
    failure: { no: string; en: string },
  ) => {
    const id = orgIdNow()
    const sessionId = currentSessionId()
    if (!id || !sessionId || busyFlag) return
    setBusy(true)
    setError(null)
    try {
      const tabs = await run(id, sessionId)
      setPreview((current) => {
        if (!current || current.browserSession?.session.id !== sessionId) return current
        return attachBrowserTabs(current, tabs)
      })
      void refreshTimeline(sessionId)
    } catch (reason) {
      if (currentSessionId() !== sessionId) return
      setError(browserErrorMessage(reason, tr, failure))
    } finally {
      releaseBusy(sessionId)
    }
  }

  const newTab = () => applyTabs(
    (id, sessionId) => createBrowserTab(id, sessionId),
    { no: 'Kunne ikke opprette ny nettleserfane.', en: 'Could not create a new browser tab.' },
  )

  const selectTab = (tabId: string) => applyTabs(
    (id, sessionId) => selectBrowserTab(id, sessionId, tabId),
    { no: 'Kunne ikke bytte nettleserfane.', en: 'Could not switch browser tab.' },
  )

  const closeTab = (tabId: string) => applyTabs(
    (id, sessionId) => closeBrowserTab(id, sessionId, tabId),
    { no: 'Kunne ikke lukke nettleserfanen.', en: 'Could not close the browser tab.' },
  )

  const applySocketObservation = (observation: BrowserObservation, session?: BrowserSession) => {
    setPreview((current) => {
      if (!current?.browserSession) return current
      if (session) return attachBrowserSession(current, { observation, session })
      return attachBrowserObservation(current, observation, { controlMode: 'human_takeover' })
    })
  }

  const stopAiRun = () => {
    loop.requestStop()
    aiRunAbort?.abort()
  }

  const pauseAiRun = () => loop.requestPause()
  const resumeAiRun = () => loop.requestResume()

  const startAiRun = async (goal: string): Promise<void> => {
    const id = orgIdNow()
    const sessionId = currentSessionId()
    if (!id || !sessionId) return
    if (!loop.begin(goal)) return
    setError(null)
    setAiRunRationale(null)
    setApprovals([])
    setDecidingApprovalKeys([])
    aiRunAbort?.abort()
    const abort = new AbortController()
    aiRunAbort = abort
    let dispatchCount = 0
    try {
      const startParams = { goal, maxSteps: BROWSER_AI_LOOP_MAX_STEPS }
      let run: Awaited<ReturnType<typeof startBrowserAiRun>>
      try {
        run = await startBrowserAiRun(id, sessionId, startParams, abort.signal, {
          idempotencyKey: keyring.keyFor(sessionId, startParams),
        })
        keyring.succeeded()
      } catch (startFailure) {
        keyring.failed(startFailure)
        throw startFailure
      }
      loop.attachRun({ orgId: id, runId: run.runId })
      await streamRunEvents(run.runId, {
        onBrowserAction: (event) => {
          dispatchCount += 1
          loop.onActionDispatched()
          setAiRunRationale({
            actionType: event.actionType ?? null,
            confidence: null,
            done: false,
            goal,
            modelUsed: null,
            reason: event.reason?.trim() ? event.reason : null,
            step: dispatchCount,
          })
        },
        onBrowserObservation: () => loop.onObservationReceived(),
        onBrowserRunPaused: () => loop.onRunPaused(),
        onBrowserRunResumed: () => loop.onRunResumed(),
        onBrowserActionApprovalRequired: (event) => {
          setApprovals((entries) => withBrowserApprovalRequested(entries, event))
          loop.onApprovalRequired()
        },
        onBrowserActionDecided: (event) => {
          setApprovals((entries) => withBrowserApprovalDecided(entries, event))
          loop.onApprovalDecided(event.decision === 'granted' ? 'granted' : event.decision === 'timed_out' ? 'timed_out' : 'denied')
        },
        onError: (streamError) => {
          // A deliberate abort (new link, close, dispose) is not an error to show.
          if (abort.signal.aborted) {
            loop.finish('stopped')
            return
          }
          const message = browserErrorMessage(streamError, tr, {
            no: 'AI-loopen mistet forbindelsen til kjøringen.',
            en: 'The AI browser loop lost its run connection.',
          })
          loop.finish('stopped', message)
          setError(message)
        },
        onDone: () => loop.finish('done'),
      }, abort.signal)
    } catch (reason) {
      if (abort.signal.aborted) {
        loop.finish('stopped')
        return
      }
      const message = browserErrorMessage(reason, tr, {
        no: 'AI-loopen kunne ikke startes.',
        en: 'The AI browser loop could not be started.',
      })
      loop.finish('stopped', message)
      setError(message)
    } finally {
      if (aiRunAbort === abort) aiRunAbort = null
    }
  }

  const decideApproval = async (entryKey: string, decision: ApprovalDecision): Promise<void> => {
    const entry = approvals().find((candidate) => candidate.key === entryKey)
    if (!entry) return
    // The decision belongs to the session that was current when it was made.
    const sessionId = currentSessionId()
    setDecidingApprovalKeys((keys) => (keys.includes(entryKey) ? keys : [...keys, entryKey]))
    try {
      let approvalId = entry.approvalId
      if (!approvalId) {
        if (!entry.runId) {
          setError(tr('Fant ikke kjøringen denne handlingen tilhører.', 'Could not find the run this action belongs to.'))
          return
        }
        const pending = await listApprovals(entry.runId)
        if (currentSessionId() !== sessionId) return
        const match = pending.find((candidate) => (candidate.status ?? '').toUpperCase() === 'PENDING')
        if (!match) {
          setError(tr(
            'Fant ingen ventende godkjenning for denne handlingen ennå — prøv igjen om et øyeblikk.',
            'No pending approval was found for this action yet — try again in a moment.',
          ))
          return
        }
        approvalId = match.id
      }
      await decideApprovalRequest(approvalId, decision)
      // A decision made for a replaced session is recorded on the server; this page has moved on.
      if (currentSessionId() !== sessionId) return
      // Optimistic: the run's own `browser_action_decided` event confirms it later.
      const resolved = decision === 'approve' ? 'granted' : 'denied'
      setApprovals((entries) => withBrowserApprovalDecided(entries, {
        runId: entry.runId,
        actionId: entry.actionId,
        approvalId,
        decision: resolved,
      }))
      loop.onApprovalDecided(resolved)
    } catch (reason) {
      if (currentSessionId() !== sessionId) return
      setError(browserErrorMessage(reason, tr, {
        no: 'Kunne ikke registrere beslutningen.',
        en: 'Could not record the decision.',
      }))
    } finally {
      setDecidingApprovalKeys((keys) => keys.filter((key) => key !== entryKey))
    }
  }

  const onPageHide = () => {
    const id = orgIdNow()
    const sessionId = currentSessionId()
    if (!id || !sessionId) return
    try {
      // keepalive lets the close outlive the page. The response is not needed.
      void fetch(`${gatewayBaseUrl()}/api/v1/browser/sessions/${encodeURIComponent(sessionId)}`, {
        method: 'DELETE',
        keepalive: true,
        credentials: 'include',
        headers: { 'x-verevon-org-id': id },
      }).catch(() => undefined)
    } catch {
      // The page is going away; there is nothing left to report to.
    }
  }

  if (typeof window !== 'undefined') window.addEventListener('pagehide', onPageHide)

  const dispose = async (): Promise<void> => {
    generation += 1
    stopAiRun()
    if (typeof window !== 'undefined') window.removeEventListener('pagehide', onPageHide)
    const sessionId = currentSessionId()
    const closeZdr = openedZdr
    setPreview(null)
    setApprovals([])
    setDecidingApprovalKeys([])
    setAiRunRationale(null)
    setNavigationError(null)
    setError(null)
    openedZdr = false
    setBusy(false)
    if (closeZdr) await closeSessionById(sessionId)
  }

  return {
    preview,
    view: () => {
      const current = preview()
      return current ? viewModelOf(current) : null
    },
    busy,
    error,
    navigationError,
    approvals,
    decidingApprovalKeys,
    aiRunRationale,
    loop: loopState,
    open,
    close,
    runAction,
    setControlMode,
    newTab,
    selectTab,
    closeTab,
    refreshTimeline,
    applySocketObservation,
    startAiRun,
    pauseAiRun,
    resumeAiRun,
    stopAiRun,
    decideApproval,
    dispose,
  }
}
