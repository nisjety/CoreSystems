import { For, Show, createMemo, createSignal } from 'solid-js'

import {
  SPACE_BROWSER_MAX_GOAL_CHARS,
  SPACE_BROWSER_MAX_HOSTS,
  spaceBrowserHostProblem,
  startSpaceBrowserRun,
  type SpaceBrowserHostProblem,
  type SpaceBrowserRunReceipt,
} from '@/shared/api/spaces-client/browser-runs'
import { useI18n } from '@/shared/i18n'
import { translateApiError } from '@/shared/i18n/errors'
import './space-browser.css'

/** The duration choices, in seconds (R1 takes 30 to 600; the panel offers three). */
const RUNTIME_CHOICES = [120, 300, 600] as const

export interface SpaceBrowserStartPanelProps {
  readonly spaceRef: string
  /** The thread the run starts in. Absent starts a new thread (D4): the gateway creates it and the receipt names it. */
  readonly threadId?: string
  /** Whether this viewer may start a run here: an editor, manager or owner in a room that is not archived. */
  readonly canStart: boolean
  /** Why the start is closed, said in words, when `canStart` is false. */
  readonly disabledReason?: string
  readonly onStarted: (receipt: SpaceBrowserRunReceipt) => void
  readonly onCancel: () => void
}

type Tr = (noText: string, enText: string) => string

/** The hint the person reads under the domain field. Empty when there is nothing to fix. */
function hostHint(problem: SpaceBrowserHostProblem | null, tr: Tr): string {
  switch (problem) {
    case 'ip_literal':
      return tr('Bruk et domenenavn, ikke en IP-adresse.', 'Use a domain name, not an IP address.')
    case 'scheme':
      return tr('Skriv bare domenet, uten http:// eller https://.', 'Type the domain only, without http:// or https://.')
    case 'path':
      return tr('Skriv bare domenet, uten sti.', 'Type the domain only, without a path.')
    case 'port':
      return tr('Skriv bare domenet, uten portnummer.', 'Type the domain only, without a port.')
    case 'localhost':
      return tr('localhost kan ikke brukes.', 'localhost cannot be used.')
    case 'wildcard':
      return tr('Jokertegn kan ikke brukes.', 'Wildcards cannot be used.')
    case 'format':
      return tr('Skriv et gyldig domenenavn, for eksempel eksempel.no.', 'Type a valid domain name, for example example.com.')
    default:
      return ''
  }
}

/** One send's identity for the panel's life: a retry after a lost answer names the same run. */
function newSendId(): string {
  const uuid = globalThis.crypto?.randomUUID?.()
  if (uuid) return uuid.replace(/-/g, '')
  return `${Date.now().toString(36)}${Math.random().toString(36).slice(2)}`.padEnd(16, '0').slice(0, 64)
}

/**
 * The start form of a room browser run (WS20-S5). The goal, the domains the run may visit (1 to 8), and the longest
 * duration. The room sees the domains and the steps; the live view and the screenshots are the starter's alone (SD-23).
 * The gateway and Control decide who may start; this form only shows the rules and what the room will see.
 */
export function SpaceBrowserStartPanel(props: SpaceBrowserStartPanelProps) {
  const i18n = useI18n()
  const [goal, setGoal] = createSignal('')
  const [hosts, setHosts] = createSignal<string[]>([])
  const [hostDraft, setHostDraft] = createSignal('')
  const [runtime, setRuntime] = createSignal<number>(300)
  const [busy, setBusy] = createSignal(false)
  const [failure, setFailure] = createSignal<string | null>(null)
  const sendId = newSendId()

  const draftProblem = createMemo(() => {
    const draft = hostDraft().trim()
    return draft ? spaceBrowserHostProblem(draft) : null
  })
  const canSubmit = createMemo(
    () =>
      props.canStart
      && !busy()
      && goal().trim().length > 0
      && goal().trim().length <= SPACE_BROWSER_MAX_GOAL_CHARS
      && hosts().length > 0,
  )

  const addHost = () => {
    const draft = hostDraft().trim().toLowerCase()
    if (!draft) return
    if (spaceBrowserHostProblem(draft) !== null) return
    if (hosts().includes(draft)) {
      setHostDraft('')
      return
    }
    if (hosts().length >= SPACE_BROWSER_MAX_HOSTS) {
      setFailure(i18n.tr('Du kan velge høyst åtte domener.', 'You can choose at most eight domains.'))
      return
    }
    setFailure(null)
    setHosts([...hosts(), draft])
    setHostDraft('')
  }

  const removeHost = (host: string) => setHosts(hosts().filter((item) => item !== host))

  const submit = async (event: SubmitEvent) => {
    event.preventDefault()
    if (!canSubmit()) return
    setBusy(true)
    setFailure(null)
    try {
      const receipt = await startSpaceBrowserRun(props.spaceRef, {
        threadId: props.threadId,
        goal: goal().trim(),
        allowedDomains: hosts(),
        maxRuntimeSeconds: runtime(),
        clientSendId: sendId,
      })
      props.onStarted(receipt)
    } catch (error) {
      setFailure(translateApiError(error, i18n.tr))
    } finally {
      setBusy(false)
    }
  }

  return (
    <section class="verevon-space-browser-start" aria-labelledby="verevon-space-browser-start-title">
      <h3 id="verevon-space-browser-start-title" class="verevon-space-browser-start__title">
        {i18n.tr('Start en nettleserkjøring', 'Start a browser run')}
      </h3>
      <Show when={!props.canStart}>
        <p class="verevon-space-browser-start__reason" role="status">
          {props.disabledReason ?? i18n.tr('Du kan ikke starte en nettleserkjøring i dette rommet.', 'You cannot start a browser run in this room.')}
        </p>
      </Show>
      <form class="verevon-space-browser-start__form" onSubmit={submit}>
        <label class="verevon-space-browser-start__label">
          <span>{i18n.tr('Hva skal agenten gjøre?', 'What should the agent do?')}</span>
          <textarea
            rows={3}
            maxlength={SPACE_BROWSER_MAX_GOAL_CHARS}
            value={goal()}
            disabled={!props.canStart || busy()}
            onInput={(event) => {
              setGoal(event.currentTarget.value)
            }}
          />
        </label>
        <div class="verevon-space-browser-start__hosts">
          <label class="verevon-space-browser-start__label">
            <span>{i18n.tr('Domener agenten kan besøke (1 til 8)', 'Domains the agent may visit (1 to 8)')}</span>
            <input
              type="text"
              autocomplete="off"
              spellcheck={false}
              value={hostDraft()}
              disabled={!props.canStart || busy()}
              aria-describedby="verevon-space-browser-host-hint"
              onInput={(event) => setHostDraft(event.currentTarget.value)}
              onKeyDown={(event) => {
                if (event.key === 'Enter' || event.key === ',') {
                  event.preventDefault()
                  addHost()
                }
              }}
              onBlur={addHost}
            />
          </label>
          <p id="verevon-space-browser-host-hint" class="verevon-space-browser-start__hint">
            {hostHint(draftProblem(), i18n.tr)}
          </p>
          <ul class="verevon-space-browser-start__chips" aria-label={i18n.tr('Valgte domener', 'Chosen domains')}>
            <For each={hosts()}>
              {(host) => (
                <li class="verevon-space-browser-start__chip">
                  <span>{host}</span>
                  <button
                    type="button"
                    disabled={!props.canStart || busy()}
                    aria-label={i18n.tr(`Fjern ${host}`, `Remove ${host}`)}
                    onClick={() => removeHost(host)}
                  >
                    ×
                  </button>
                </li>
              )}
            </For>
          </ul>
        </div>
        <fieldset class="verevon-space-browser-start__runtime">
          <legend>{i18n.tr('Lengste varighet', 'Longest duration')}</legend>
          <For each={RUNTIME_CHOICES}>
            {(seconds) => (
              <label>
                <input
                  type="radio"
                  name="verevon-space-browser-runtime"
                  value={seconds}
                  checked={runtime() === seconds}
                  disabled={!props.canStart || busy()}
                  onChange={() => setRuntime(seconds)}
                />
                {i18n.tr(`${Math.round(seconds / 60)} min`, `${Math.round(seconds / 60)} min`)}
              </label>
            )}
          </For>
        </fieldset>
        <p class="verevon-space-browser-start__share">
          {i18n.tr(
            'Rommet ser hvilke domener og sider agenten besøker, men ikke skjermbilder eller live-visningen. Bare du ser dem.',
            'The room sees which domains and pages the agent visits, but not screenshots or the live view. Only you see them.',
          )}
        </p>
        <Show when={failure()}>
          {(message) => (
            <p class="verevon-space-browser-start__error" role="alert">
              {message()}
            </p>
          )}
        </Show>
        <div class="verevon-space-browser-start__actions">
          <button type="button" onClick={props.onCancel}>
            {i18n.tr('Avbryt', 'Cancel')}
          </button>
          <button type="submit" disabled={!canSubmit()}>
            {busy() ? i18n.tr('Starter …', 'Starting …') : i18n.tr('Start nettleserkjøring', 'Start browser run')}
          </button>
        </div>
      </form>
    </section>
  )
}
