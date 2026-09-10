import { useLocation } from '@solidjs/router'
import { MessageCircleMore, X } from '@/shared/icons'
import { createMemo, createSignal, Show } from 'solid-js'
import { useI18n } from '@/shared/i18n'
import { submitFeedback } from '@/shared/api/inbox-client'
import { getSession } from '@/shared/session/session-store'
import { Button } from '@/shared/ui/Button'
import { VerevonTextarea } from '@/shared/ui/verevon/VerevonTextarea'

const MAX_NOTE_LENGTH = 600

type SubmitState = 'idle' | 'sending' | 'sent' | 'error'

/**
 * Persistent, zero-setup "Send feedback" control for the pilot feedback
 * channel -- rendered once in CoreShell so it floats over every route. A
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
  const orgId = createMemo(() => getSession().activeOrg?.id ?? null)
  const canSubmit = createMemo(() => note().trim().length > 0 && state() !== 'sending' && state() !== 'sent')
  // Support already owns the bottom-right corner for its persistent Verevon
  // composer. The global feedback trigger is redundant there and can cover the
  // send control on narrower viewports, so keep feedback available everywhere
  // else without layering two composers on top of one another.
  const isSupportInbox = createMemo(() => location.pathname.startsWith('/support'))
  // The chat page docks its composer to the same bottom-right corner this
  // widget floats in, and that dock's height is unbounded (autosizing
  // textarea). --docked reads the live measured height ChatPage publishes
  // as --verevon-composer-dock-height so the trigger always clears it
  // instead of landing underneath the send button.
  const isDocked = createMemo(() => location.pathname.startsWith('/chat'))

  const reset = () => {
    setOpen(false)
    setState('idle')
    setNote('')
  }

  const submit = async (event: SubmitEvent) => {
    event.preventDefault()
    const activeOrgId = orgId()
    const text = note().trim()
    if (!text || !activeOrgId || state() === 'sending') return

    setState('sending')
    const session = getSession()
    try {
      await submitFeedback(activeOrgId, {
        bodyText: text,
        fromName: session.user?.name ?? undefined,
        fromEmail: session.user?.email ?? undefined,
        // Best-effort context so a terse one-line report is still actionable --
        // the team can see exactly where the friction happened.
        pageUrl: `${location.pathname}${location.search}`,
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
            <button
              type="button"
              class="feedback-widget__trigger"
              onClick={() => setOpen(true)}
            >
              <MessageCircleMore size={15} aria-hidden="true" />
              <span>{i18n.tr('Tilbakemelding', 'Feedback')}</span>
            </button>
          }
        >
          <form class="feedback-widget__panel" onSubmit={submit}>
            <div class="feedback-widget__header">
              <span class="feedback-widget__title">{i18n.tr('Meld friksjon', 'Report friction')}</span>
              <button
                type="button"
                class="feedback-widget__close"
                onClick={reset}
                aria-label={i18n.tr('Lukk tilbakemelding', 'Close feedback')}
              >
                <X size={14} aria-hidden="true" />
              </button>
            </div>
            <p class="feedback-widget__hint">
              {i18n.tr(
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
              placeholder={i18n.tr('Beskriv friksjonen med én setning …', 'Describe the friction in one sentence…')}
              aria-label={i18n.tr('Tilbakemeldingsnotat', 'Feedback note')}
              onInput={(event) => setNote(event.currentTarget.value)}
            />
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
