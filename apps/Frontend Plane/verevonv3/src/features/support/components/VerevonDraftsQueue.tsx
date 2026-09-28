import { Bot, ExternalLink, PenLine } from '@/shared/icons'
import { createMemo, For, Show } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import { listAiActions, type AiAction } from '@/shared/api/inbox-client'
import { localeDateTime, translateApiError, useI18n } from '@/shared/i18n'

/** Proposal kinds that are a piece of WRITING Verevon produced for a person to
 * send or file. A ticket classification or an incident declaration is a
 * proposal too, but it is not a draft, and listing it here would make "the
 * drafts Verevon wrote" mean "everything Verevon suggested". */
const DRAFT_KINDS: readonly string[] = ['draft.reply', 'internal.note']

/** Statuses that still need a person. Everything else is history.
 *
 * `suggested` and `suggest_ticket` are what conversation-core actually stores
 * (the older ticket path used the second name); `review` is the query alias
 * for both and is listed defensively, so a row that ever surfaces under the
 * alias is not silently filed as already handled. */
const AWAITING_DECISION: readonly string[] = ['suggested', 'review', 'suggest_ticket']

function isDraft(action: AiAction): boolean {
  return DRAFT_KINDS.includes(action.kind)
}

function awaitingDecision(action: AiAction): boolean {
  return AWAITING_DECISION.includes(action.status)
}

function kindLabel(kind: string, tr: (noText: string, enText: string) => string): string {
  if (kind === 'draft.reply') return tr('Svarutkast', 'Reply draft')
  if (kind === 'internal.note') return tr('Internt notat', 'Internal note')
  return kind.replace(/[._]/g, ' ')
}

function statusLabel(status: string, tr: (noText: string, enText: string) => string): string {
  switch (status) {
    case 'suggested':
    case 'review':
    case 'suggest_ticket': return tr('Venter på deg', 'Waiting for you')
    case 'approved': return tr('Godkjent', 'Approved')
    case 'executed': return tr('Sendt', 'Sent')
    case 'rejected': return tr('Avvist', 'Declined')
    case 'failed': return tr('Feilet etter godkjenning', 'Failed after approval')
    default: return status.replace(/[._]/g, ' ')
  }
}

/** The draft's own text. Unlike a generic proposal preview this does not fall
 * back to a title or a field summary: a draft with no body is not a draft with
 * a different preview, it is a draft whose text failed to record, and saying so
 * is more useful than showing its metadata as if it were the writing. */
function draftBody(action: AiAction): string {
  const body = action.payload?.body_text
  return typeof body === 'string' ? body.trim() : ''
}

function formatWhen(value: string, locale: string): string {
  const parsed = new Date(value)
  if (Number.isNaN(parsed.getTime())) return ''
  return parsed.toLocaleString(locale, { dateStyle: 'medium', timeStyle: 'short' })
}

/** Everything Verevon has written and a person has not sent themselves.
 *
 * This lives only in the support workspace. The chat page lists threads whose
 * `origin` is `chat`; a draft is not a thread at all — it is a row in the
 * action ledger attached to a customer conversation — so the two surfaces
 * cannot bleed into each other by construction.
 *
 * Each draft links into its source conversation rather than offering an
 * approve button here: approving a reply without seeing the thread it answers
 * is how a plausible-looking draft gets sent to the wrong customer.
 */
export function VerevonDraftsQueue() {
  const i18n = useI18n()
  // One read, not two: "waiting" and "already decided" are the same rows in
  // different states, and asking twice makes a draft that changes state
  // between the two requests appear in both lists or in neither.
  const [actions] = createResource(() => listAiActions({ status: 'all', limit: 100 }))
  const drafts = createMemo(() => (actions() ?? []).filter(isDraft))
  const waiting = createMemo(() => drafts().filter(awaitingDecision))
  const decided = createMemo(() => drafts().filter((action) => !awaitingDecision(action)))

  const section = (action: AiAction) => (
    <li class="verevon-drafts__item" data-ai-action-id={action.id} data-status={action.status}>
      <div class="verevon-drafts__item-head">
        <strong>{kindLabel(action.kind, i18n.tr)}</strong>
        <span class="verevon-drafts__status" data-status={action.status}>{statusLabel(action.status, i18n.tr)}</span>
      </div>
      <Show
        when={draftBody(action)}
        fallback={<p class="verevon-drafts__missing">{i18n.tr('Utkastet har ingen lagret tekst.', 'This draft has no recorded text.')}</p>}
      >
        {(body) => <p class="verevon-drafts__body">{body()}</p>}
      </Show>
      <div class="verevon-drafts__meta">
        <small>{formatWhen(action.created_at, localeDateTime(i18n.locale()))}</small>
        <a href={`/support?conversation_id=${encodeURIComponent(action.conversation_id)}`} link class="verevon-drafts__open-link">
          {i18n.tr('Åpne samtalen', 'Open the conversation')} <ExternalLink class="size-4" />
        </a>
      </div>
    </li>
  )

  return (
    <main class="verevon-drafts" aria-label={i18n.tr('Utkast fra Verevon', 'Drafts from Verevon')}>
      <header class="verevon-drafts__header">
        <div class="verevon-drafts__eyebrow"><PenLine class="size-4" /> {i18n.tr('Utkast', 'Drafts')}</div>
        <h1>{i18n.tr('Det Verevon har skrevet', 'What Verevon has written')}</h1>
        <p>{i18n.tr('Svarutkast og interne notater Verevon har foreslått. Ingenting herfra er sendt før en person har godkjent det i samtalen.', 'Reply drafts and internal notes Verevon proposed. Nothing here is sent until a person approves it in the conversation.')}</p>
      </header>

      <Show when={!actions.loading} fallback={<p class="verevon-drafts__state">{i18n.tr('Laster utkast …', 'Loading drafts…')}</p>}>
        <Show
          when={!actions.error}
          fallback={
            <p role="alert" class="verevon-drafts__state verevon-drafts__state--error">
              {translateApiError(actions.error, i18n.tr, { no: 'Kunne ikke laste utkastene. Prøv igjen.', en: 'Could not load the drafts. Please try again.' })}
            </p>
          }
        >
          <Show
            when={drafts().length > 0}
            fallback={
              <div class="verevon-drafts__empty">
                <Bot class="size-6" />
                <h2>{i18n.tr('Ingen utkast ennå', 'No drafts yet')}</h2>
                <p>{i18n.tr('Når Verevon foreslår et svar eller et internt notat, havner det her.', 'When Verevon proposes a reply or an internal note, it lands here.')}</p>
              </div>
            }
          >
            <section aria-label={i18n.tr('Venter på deg', 'Waiting for you')}>
              <h2 class="verevon-drafts__section-title">{i18n.tr('Venter på deg', 'Waiting for you')} ({waiting().length})</h2>
              <Show
                when={waiting().length > 0}
                fallback={<p class="verevon-drafts__state">{i18n.tr('Ingen utkast venter på en beslutning.', 'No draft is waiting for a decision.')}</p>}
              >
                <ul class="verevon-drafts__list"><For each={waiting()}>{section}</For></ul>
              </Show>
            </section>
            <Show when={decided().length > 0}>
              <section aria-label={i18n.tr('Allerede behandlet', 'Already handled')}>
                <h2 class="verevon-drafts__section-title">{i18n.tr('Allerede behandlet', 'Already handled')} ({decided().length})</h2>
                <ul class="verevon-drafts__list"><For each={decided()}>{section}</For></ul>
              </section>
            </Show>
            {/* The ledger read is capped, so say what the list is rather than
                letting a reader take a truncated page for the whole history. */}
            <p class="verevon-drafts__disclosure">{i18n.tr('Viser utkast blant de 100 siste AI-handlingene i denne organisasjonen.', 'Showing drafts among this organization’s 100 most recent AI actions.')}</p>
          </Show>
        </Show>
      </Show>
    </main>
  )
}
