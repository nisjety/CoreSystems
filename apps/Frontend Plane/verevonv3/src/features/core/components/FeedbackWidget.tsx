import { useLocation } from '@solidjs/router'
import { MessageCircleMore, X } from '@/shared/icons'
import { createMemo, createSignal, onCleanup, Show } from 'solid-js'
import { useI18n } from '@/shared/i18n'
import { submitFeedback, type FeedbackDiagnostics } from '@/shared/api/inbox-client'
import { clientRevision } from '@/shared/build/client-contract'
import { recentErrorRequestIds, reportProblemEnabled, supportRunContext } from '@/shared/api/support-correlation'
import { getSession } from '@/shared/session/session-store'
import { Button } from '@/shared/ui/Button'
import { VerevonSegmented, VerevonSegmentedButton } from '@/shared/ui/verevon/VerevonSegmented'
import { VerevonTextarea } from '@/shared/ui/verevon/VerevonTextarea'
import './feedback-widget-report.css'

const MAX_NOTE_LENGTH = 600

type SubmitState = 'idle' | 'sending' | 'sent' | 'error'
/** The friction note, the widget's original mode, or a problem report that can attach diagnostics. */
type FeedbackMode = 'note' | 'problem'

export const OPEN_FEEDBACK_EVENT = 'verevon:open-feedback'

/**
 * Persistent, zero-setup "Send feedback" control for the pilot feedback
 * channel -- rendered once in CoreShell, with a menu entry on chat. A
 * signed-in org member drops a one-line friction note; it lands as a new
 * conversation in the org's own Inbox, tagged "pilot-feedback" (see
 * inbox-client's submitFeedback and conversation-core-go's
 * Service.SubmitFeedback), and is mirrored into the team's monitored org so
 * external-pilot-org feedback stays visible. No separate feedback store, no
 * extra admin surface -- the team reviews it exactly where they already
 * review support conversations.
 */
export function FeedbackWidget() {
  const i18n = useI18n()
  const location = useLocation()
  const [open, setOpen] = createSignal(false)
  const [note, setNote] = createSignal('')
  const [state, setState] = createSignal<SubmitState>('idle')
  const [mode, setMode] = createSignal<FeedbackMode>('note')
  // The problem report's three optional sections. Each is ticked on its own, and none is ticked by default.
  const [attachIds, setAttachIds] = createSignal(false)
  const [attachBuild, setAttachBuild] = createSignal(false)
  const [attachRun, setAttachRun] = createSignal(false)
  const orgId = createMemo(() => getSession().activeOrg?.id ?? null)
  const canSubmit = createMemo(() => note().trim().length > 0 && state() !== 'sending' && state() !== 'sent')
  const locked = () => state() === 'sending' || state() === 'sent'
  const problemMode = createMemo(() => reportProblemEnabled() && mode() === 'problem')
  const errorIds = createMemo(() => recentErrorRequestIds())
  // The chat run on screen, offered only while that chat keeps content. A temporary or zero-retention chat offers none,
  // so its run section is not shown at all.
  const runOffer = createMemo(() => {
    const context = supportRunContext()
    return context?.retainsContent ? context.runId : null
  })
  // Support already owns the bottom-right corner for its persistent Verevon
  // composer. The global feedback trigger is redundant there and can cover the
  // send control on narrower viewports, so keep feedback available everywhere
  // else without layering two composers on top of one another.
  const isSupportInbox = createMemo(() => location.pathname.startsWith('/support'))
  // Chat and the Space room open feedback from a menu so the persistent trigger
  // cannot cover document copy or a composer (the room has a composer in every
  // corner, and on a phone the trigger covered the Send button).
  const isDocked = createMemo(() => location.pathname.startsWith('/chat') || location.pathname.startsWith('/spaces'))
  let returnFocus: HTMLElement | null = null
  const openFromMenu = () => {
    if (!orgId()) return
    returnFocus = document.querySelector('.verevon-chat-header-button[aria-haspopup="menu"], .core-navbar-more > summary')
    setOpen(true)
  }
  const closeOnEscape = (event: KeyboardEvent) => {
    if (open() && event.key === 'Escape') { event.preventDefault(); reset() }
  }
  window.addEventListener(OPEN_FEEDBACK_EVENT, openFromMenu)
  // Consume Escape before the chat page's document-level dismissal handler.
  window.addEventListener('keydown', closeOnEscape, true)
  onCleanup(() => {
    window.removeEventListener(OPEN_FEEDBACK_EVENT, openFromMenu)
    window.removeEventListener('keydown', closeOnEscape, true)
  })

  const reset = () => {
    setOpen(false)
    setState('idle')
    setNote('')
    setMode('note')
    setAttachIds(false)
    setAttachBuild(false)
    setAttachRun(false)
    returnFocus?.focus()
    returnFocus = null
  }

  /**
   * What a problem report carries, read at the moment it is sent. Only the sections the person ticked are present. A
   * section with no value to give (no build reported yet, no run on screen) is left out rather than invented.
   */
  const diagnosticsToSend = (): FeedbackDiagnostics | undefined => {
    const diagnostics: FeedbackDiagnostics = {}
    if (attachIds()) diagnostics.requestIds = recentErrorRequestIds()
    const build = clientRevision()
    if (attachBuild() && build) diagnostics.build = build
    const runId = runOffer()
    if (attachRun() && runId) diagnostics.runId = runId
    return Object.keys(diagnostics).length > 0 ? diagnostics : undefined
  }

  const submit = async (event: SubmitEvent) => {
    event.preventDefault()
    const activeOrgId = orgId()
    const text = note().trim()
    if (!text || !activeOrgId || state() === 'sending') return

    setState('sending')
    const session = getSession()
    // A friction note sends exactly what it always has. A problem report adds only the sections the person ticked.
    const diagnostics = problemMode() ? diagnosticsToSend() : undefined
    try {
      await submitFeedback(activeOrgId, {
        bodyText: text,
        fromName: session.user?.name ?? undefined,
        fromEmail: session.user?.email ?? undefined,
        // Best-effort context so a terse one-line report is still actionable --
        // the team can see exactly where the friction happened.
        pageUrl: `${location.pathname}${location.search}`,
        ...(diagnostics ? { diagnostics } : {}),
      })
      setState('sent')
      window.setTimeout(reset, 1600)
    } catch {
      setState('error')
    }
  }

  return (
    <Show when={orgId() && !isSupportInbox()}>
      <div class={`feedback-widget${isDocked() ? ' feedback-widget--docked' : ''}`}>
        <Show
          when={open()}
          fallback={
            <Show when={!isDocked()}>
            <button
              type="button"
              class="feedback-widget__trigger"
              onClick={() => setOpen(true)}
            >
              <MessageCircleMore size={15} aria-hidden="true" />
              <span>{i18n.tr('Tilbakemelding', 'Feedback')}</span>
            </button>
            </Show>
          }
        >
          <form
            class={`feedback-widget__panel${problemMode() ? ' feedback-widget__panel--report' : ''}`}
            role="dialog"
            aria-label={problemMode() ? i18n.tr('Problemrapport', 'Problem report') : i18n.tr('Meld friksjon', 'Report friction')}
            onSubmit={submit}
          >
            <div class="feedback-widget__header">
              <span class="feedback-widget__title">
                {problemMode() ? i18n.tr('Problemrapport', 'Problem report') : i18n.tr('Meld friksjon', 'Report friction')}
              </span>
              <button
                type="button"
                class="feedback-widget__close"
                onClick={reset}
                aria-label={i18n.tr('Lukk tilbakemelding', 'Close feedback')}
              >
                <X size={14} aria-hidden="true" />
              </button>
            </div>
            <Show when={reportProblemEnabled()}>
              <VerevonSegmented class="feedback-widget__modes" role="group" aria-label={i18n.tr('Type tilbakemelding', 'Kind of feedback')}>
                <VerevonSegmentedButton selected={!problemMode()} onClick={() => setMode('note')}>
                  {i18n.tr('Friksjon', 'Friction note')}
                </VerevonSegmentedButton>
                <VerevonSegmentedButton selected={problemMode()} onClick={() => setMode('problem')}>
                  {i18n.tr('Problemrapport', 'Problem report')}
                </VerevonSegmentedButton>
              </VerevonSegmented>
            </Show>
            <p class="feedback-widget__hint">
              {problemMode()
                ? i18n.tr(
                  'Beskriv hva som gikk galt. Du velger selv hva som legges ved.',
                  'Describe what went wrong. You choose what else is attached.',
                )
                : i18n.tr(
                  'Kort og greit -- hva gikk ikke som forventet? Teamet ser dette i innboksen.',
                  'Quick and simple -- what did not work as expected? The team sees this in the Inbox.',
                )}
            </p>
            <VerevonTextarea
              value={note()}
              maxlength={MAX_NOTE_LENGTH}
              rows={3}
              autofocus
              disabled={state() === 'sending' || state() === 'sent'}
              placeholder={problemMode()
                ? i18n.tr('Hva skjedde, og hva prøvde du å gjøre?', 'What happened, and what were you trying to do?')
                : i18n.tr('Beskriv friksjonen med én setning …', 'Describe the friction in one sentence…')}
              aria-label={i18n.tr('Tilbakemeldingsnotat', 'Feedback note')}
              onInput={(event) => setNote(event.currentTarget.value)}
            />
            <Show when={problemMode()}>
              <fieldset class="feedback-widget__attach">
                <legend class="feedback-widget__attach-title">{i18n.tr('Legg ved (valgfritt)', 'Attach (optional)')}</legend>
                <label class="feedback-widget__check">
                  <input
                    type="checkbox"
                    checked={attachIds()}
                    disabled={locked()}
                    onChange={(event) => setAttachIds(event.currentTarget.checked)}
                  />
                  <span>{i18n.tr('Inkluder nylige feil-ID-er', 'Include recent error ids')}</span>
                </label>
                <label class="feedback-widget__check">
                  <input
                    type="checkbox"
                    checked={attachBuild()}
                    disabled={!clientRevision() || locked()}
                    onChange={(event) => setAttachBuild(event.currentTarget.checked)}
                  />
                  <span>{i18n.tr('Inkluder app-versjon', 'Include app build')}</span>
                </label>
                <Show when={runOffer()}>
                  <label class="feedback-widget__check">
                    <input
                      type="checkbox"
                      checked={attachRun()}
                      disabled={locked()}
                      onChange={(event) => setAttachRun(event.currentTarget.checked)}
                    />
                    <span>{i18n.tr('Inkluder gjeldende kjørings-ID', 'Include current run id')}</span>
                  </label>
                </Show>
              </fieldset>
              <section class="feedback-widget__sent" aria-label={i18n.tr('Dette sendes', 'What gets sent')}>
                <p class="feedback-widget__sent-title">{i18n.tr('Dette sendes med notatet', 'Sent with your note')}</p>
                <ul class="feedback-widget__sent-list">
                  <li>{i18n.tr('Notatet, siden du er på, og navnet og e-posten din', 'Your note, the page you are on, and your name and email')}</li>
                  <Show when={attachIds()}>
                    <li>
                      {i18n.tr('Feil-ID-er', 'Error ids')}: {errorIds().length > 0 ? errorIds().join(', ') : i18n.tr('ingen registrert', 'none recorded')}
                    </li>
                  </Show>
                  <Show when={attachBuild()}>
                    <li>
                      {i18n.tr('App-versjon', 'App build')}: {clientRevision() ?? i18n.tr('ikke rapportert ennå, sendes ikke', 'not reported yet, not sent')}
                    </li>
                  </Show>
                  <Show when={attachRun() && runOffer()}>
                    <li>{i18n.tr('Kjørings-ID', 'Run ID')}: {runOffer()}</li>
                  </Show>
                </ul>
                <p class="feedback-widget__sent-note">
                  {i18n.tr('Ingen samtaletekst og ingen innhold fra forespørsler sendes.', 'No conversation text and no request contents are sent.')}
                </p>
              </section>
            </Show>
            <Show when={state() === 'error'}>
              <p class="feedback-widget__status feedback-widget__status--error" role="alert">
                {i18n.tr('Kunne ikke sende tilbakemeldingen. Prøv igjen.', 'Could not send the feedback. Try again.')}
              </p>
            </Show>
            <Show when={state() === 'sent'}>
              <p class="feedback-widget__status feedback-widget__status--ok" role="status">
                {i18n.tr('Takk! Tilbakemeldingen er sendt til teamet.', 'Thanks! Feedback sent to the team.')}
              </p>
            </Show>
            <div class="feedback-widget__actions">
              <Button type="submit" size="sm" disabled={!canSubmit()}>
                {state() === 'sending' ? i18n.tr('Sender …', 'Sending…') : i18n.tr('Send', 'Send')}
              </Button>
            </div>
          </form>
        </Show>
      </div>
    </Show>
  )
}
