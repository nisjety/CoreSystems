import type { DesktopBrowserBridge, DesktopBrowserSnapshot } from '@/features/browser/desktop/desktop-browser'
import type { AssistantAgentView } from '@/features/core/components/GlobalAssistant'
import { suggestDesktopBrowserAction } from '@/shared/api/desktop-browser-client'
import { assistantBrowserTarget } from './assistant-browser-agent'
import { LocalizedError } from '@/shared/i18n/errors'

const maximumSteps = 18
const taskTimeout = 180_000
// The gateway refuses a planner request with more than 20 history lines, and the client may add one retry line.
// Only the latest 19 lines go to the planner, so dismissal receipts cannot push a run over that limit.
const plannerHistoryLines = 19

type Run = {
  scope: string
  abort: AbortController
  lease: Promise<string>
  release?: Promise<void>
}

type Options = {
  bridge: DesktopBrowserBridge
  scope: () => string | null
  onChange: (state: AssistantAgentView) => void
  tr: (no: string, en: string) => string
  suggest?: typeof suggestDesktopBrowserAction
  /** Whether a run may reject cookie banners (org policy browser.consent.reject). Absent, false or a failed read is off. */
  consentReject?: () => Promise<boolean>
}

function observation(snapshot: DesktopBrowserSnapshot) {
  return {
    heading: snapshot.heading?.slice(0, 500),
    text: snapshot.text.slice(0, 12_000),
    targets: snapshot.targets.slice(0, 120).map(({ id, role, label, href, checked }) => ({
      id, role: role.slice(0, 40), label: label.slice(0, 200), href: href?.slice(0, 500), checked,
    })),
    status: snapshot.status,
    loadError: snapshot.loadError?.slice(0, 500),
  }
}

function waitForPage(ms: number, signal: AbortSignal) {
  return new Promise<void>((resolve, reject) => {
    signal.throwIfAborted()
    const finish = () => { signal.removeEventListener('abort', cancel); resolve() }
    const timer = setTimeout(finish, ms)
    const cancel = () => { clearTimeout(timer); reject(signal.reason) }
    signal.addEventListener('abort', cancel, { once: true })
  })
}

function actionFailure(error: unknown) {
  const message = error instanceof Error ? error.message : ''
  if (message.includes('Denne kontrollen kan ikke brukes')) return 'The target is unavailable or covered. Choose a different route or inspect the visible page.'
  if (message.includes('Kontrollen flyttet seg') || message.includes('Siden har endret seg')) return 'The page changed before the action could finish. Use the latest targets.'
  if (message.includes('Søkefeltet er ikke i fokus') || message.includes('Enter er bare tilgjengelig')) return 'A public search field was not focused.'
  return 'The browser refused the action.'
}

/** Transports the live page observation and the model's next typed proposal.
 * The browser lease and action validation stay in the native Quarry adapter. */
export function createAssistantDesktopAgent({ bridge, scope, onChange, tr, suggest = suggestDesktopBrowserAction, consentReject }: Options) {
  let active: Run | null = null
  let state: AssistantAgentView | null = null
  // Counts resets, so a task that is still reading the policy notices it was reset before it started.
  let resets = 0
  const current = (run: Run) => active === run && scope() === run.scope
  const publish = (patch: Partial<AssistantAgentView>) => {
    if (!state) return
    state = { ...state, ...patch }
    onChange(state)
  }
  const release = (run: Run) => {
    run.release ??= run.lease.then(id => bridge.endRun(id), () => undefined)
    return run.release
  }
  // A missing or failed policy read leaves reject-only dismissal off for the run.
  async function rejectConsentAllowed() {
    try {
      return (await consentReject?.()) === true
    } catch {
      return false
    }
  }

  async function work(run: Run, id: string, goal: string, url: string) {
    const signal = AbortSignal.any([run.abort.signal, AbortSignal.timeout(taskTimeout)])
    let terminal: Partial<AssistantAgentView>
    try {
      const history: string[] = []
      const check = () => {
        signal.throwIfAborted()
        if (!current(run)) throw new Error('Desktop browser scope changed')
      }
      const execute = async (action: Parameters<DesktopBrowserBridge['execute']>[1]) => {
        check()
        const result = await bridge.execute(id, action)
        check()
        // Each dismissed cookie banner is recorded in the run trace. Nothing else is clicked for it.
        for (const receipt of result.receipts ?? []) history.push(`consent banner rejected on ${receipt.host}`.slice(0, 350))
        return result
      }
      const browser = await bridge.getState()
      const initialAction = browser.url === url ? { kind: 'snapshot' as const } : { kind: 'navigate' as const, url }
      let snapshot = await execute(initialAction)
      terminal = { status: 'failed', error: tr('Agenten nådde grensen for denne oppgaven. Fanen er klar til bruk.', 'The agent reached this task’s step limit. The tab is ready to use.') }
      for (let step = 0; step < maximumSteps; step += 1) {
        check()
        const pageUrl = assistantBrowserTarget(snapshot.url)
        if (!pageUrl) throw new Error('The browser did not reach a web page')
        pageUrl.hash = ''
        const proposal = await suggest({ goal, url: pageUrl.href, title: snapshot.title.slice(0, 500), observation: observation(snapshot), history: history.slice(-plannerHistoryLines) }, signal)
        check()
        if (proposal.done) {
          terminal = proposal.outcome === 'completed'
            ? { status: 'completed', result: proposal.answer, error: null }
            : { status: 'failed', error: proposal.answer }
          break
        }
        const action = proposal.action
        const label = {
          navigate: tr('Agenten åpner en side.', 'The agent is opening a page.'),
          click: tr('Agenten klikker.', 'The agent is clicking.'),
          fill: tr('Agenten fyller ut et søk.', 'The agent is filling in a search.'),
          press: tr('Agenten bruker tastaturet.', 'The agent is using the keyboard.'),
          scroll: tr('Agenten ruller på siden.', 'The agent is scrolling.'),
          wait: tr('Agenten venter på siden.', 'The agent is waiting for the page.'),
        }[action.kind]
        publish({ action: label })
        const target = 'target' in action ? snapshot.targets.find(candidate => candidate.id === action.target) : undefined
        const stepName = target ? `${action.kind} ${target.label.slice(0, 150)}` : action.kind
        try {
          const settlingFilter = action.kind === 'click' && target && ['checkbox', 'radio', 'option', 'switch'].includes(target.role)
          if (action.kind === 'wait') {
            await waitForPage(action.ms, signal)
            snapshot = await execute({ kind: 'snapshot' })
          } else {
            snapshot = await execute(action)
            if (settlingFilter) {
              // Reactive filter results can update shortly after the input event.
              await waitForPage(500, signal)
              snapshot = await execute({ kind: 'snapshot' })
            }
          }
          history.push(`${stepName}: ${snapshot.heading || snapshot.title || 'page opened'}`.slice(0, 350))
        } catch (error) {
          check()
          history.push(`${stepName}: ${actionFailure(error)}`)
          snapshot = await execute({ kind: 'snapshot' })
        }
      }
    } catch {
      terminal = run.abort.signal.aborted
        ? { status: 'stopped', error: null }
        : { status: 'failed', error: signal.aborted
          ? tr('Nettleseroppgaven tok for lang tid.', 'The browser task timed out.')
          : tr('Agenten mistet kontakten med nettleseren eller modellplanleggeren.', 'The agent lost contact with the browser or model planner.') }
    }
    try {
      await release(run)
    } catch {
      terminal = { status: 'failed', error: tr('Kunne ikke bekrefte at fanen er frigitt.', 'Could not confirm the tab was released.') }
    }
    if (!current(run)) return
    active = null
    publish({ ...terminal, action: null })
  }

  async function send(goal: string, target: string) {
    const identity = scope()
    const url = assistantBrowserTarget(target)
    const prompt = goal.trim()
    if (!identity || !url || !prompt || prompt.length > 2000) throw new LocalizedError({ no: 'Oppgi en oppgave og en gyldig nettadresse.', en: 'Provide a task and a valid web address.' })
    if (active) throw new LocalizedError({ no: 'En oppgave er allerede i gang.', en: 'A task is already running.' })
    const attempt = resets
    const rejectAllowed = await rejectConsentAllowed()
    // The organization or the assistant may have changed while the policy was read. Nothing starts for the old one.
    if (scope() !== identity || resets !== attempt) return
    if (active) throw new LocalizedError({ no: 'En oppgave er allerede i gang.', en: 'A task is already running.' })
    const run = { scope: identity, abort: new AbortController(), lease: bridge.startRun({ consentReject: rejectAllowed }) }
    active = run
    state = { available: true, kind: 'browser', localBrowser: true, status: 'starting', goal: prompt, error: null, action: null, sessionId: null, runId: null, receipt: null }
    onChange(state)
    try {
      const id = await run.lease
      if (!current(run) || run.abort.signal.aborted) return
      publish({ runId: id, status: 'running', action: tr('Agenten undersøker siden.', 'The agent is examining the page.') })
      void work(run, id, prompt, url.href)
    } catch {
      if (!current(run)) return
      active = null
      const copy = { no: 'Kunne ikke starte den lokale nettleseren.', en: 'Could not start the local browser.' }
      const message = tr(copy.no, copy.en)
      publish({ status: 'failed', error: message })
      throw new LocalizedError(copy)
    }
  }

  function stop() {
    const run = active
    if (!run || run.abort.signal.aborted) return
    run.abort.abort()
    publish({ status: 'stopping', action: null })
    void release(run).then(() => {
      if (!current(run)) return
      active = null
      publish({ status: 'stopped', action: null, error: null })
    }).catch(() => {
      if (!current(run)) return
      active = null
      publish({ status: 'failed', error: tr('Kunne ikke bekrefte at fanen er frigitt.', 'Could not confirm the tab was released.') })
    })
  }

  function reset() {
    resets += 1
    const run = active
    active = null
    state = null
    if (run) {
      run.abort.abort()
      void release(run).catch(() => undefined)
    }
  }

  return { send, stop, reset }
}
