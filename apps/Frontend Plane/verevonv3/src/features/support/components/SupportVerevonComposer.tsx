import { ArrowUpRight, Send, Sparkles } from '@/shared/icons'
import { createEffect, createMemo, createSignal, For, Show } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import { bindSupportChatThread, readSupportChatThread, type SupportChatThreadScope } from '@/shared/chat/support-chat-thread'
import { runAssist, type AssistMessage, type AssistSource } from '@/features/inbox/lib/inbox-ai'
import type { ModelContextPack } from '@/shared/context-packs/context-pack'
import { readChatThreadTranscript, type ChatThreadTranscriptTurn } from '@/features/chat/lib/chat-thread-history'
import { getChatThreadTranscript } from '@/shared/api/chat-client'
import { createDraftReplyProposal, createInternalNoteProposal, createTicketUpdateProposal } from '@/shared/api/inbox-client'
import { useI18n } from '@/shared/i18n'
import { SupportAssistDisclosure, type SupportAssistRunMetadata } from './SupportAssistDisclosure'
import { parseInboxTriageProposal } from '@/features/inbox/lib/inbox-ai-triage'

type ReadableTranscriptTurn = Pick<ChatThreadTranscriptTurn, 'content' | 'role' | 'status'>
type NextActionKind = 'reply' | 'note' | 'ticketUpdate'
type NextActionProposal = {
  kind: NextActionKind
  status: 'queued' | 'transient'
  text: string
}

/** Returns the latest completed assistant text suitable for the support rail.
 * Error/stopped turns are deliberately excluded so the UI never presents an
 * incomplete model response as the answer to a customer case. */
export function latestAssistantAnswer(turns: readonly ReadableTranscriptTurn[]): string | null {
  for (let index = turns.length - 1; index >= 0; index -= 1) {
    const turn = turns[index]
    if (turn?.role !== 'assistant' || turn.status === 'error' || turn.status === 'stopped') continue
    const content = turn.content.trim()
    if (content) return content
  }
  return null
}

export function SupportVerevonComposer(props: {
  contextLabel: string
  contextPack: ModelContextPack
  conversationId: string
  ticketId?: string
  conversationLoading?: boolean
  conversationSource?: 'authorized' | 'preview' | 'unavailable'
  customer?: string
  messages: AssistMessage[]
  orgId: string
  userId: string
}) {
  const i18n = useI18n()
  const conversationSource = () => props.conversationSource ?? 'authorized'
  const [question, setQuestion] = createSignal('')
  const [answer, setAnswer] = createSignal<string | null>(null)
  const [sources, setSources] = createSignal<AssistSource[]>([])
  const [runMetadata, setRunMetadata] = createSignal<SupportAssistRunMetadata | null>(null)
  const [error, setError] = createSignal<string | null>(null)
  const [loading, setLoading] = createSignal(false)
  const [threadId, setThreadId] = createSignal<string | null>(null)
  const [threadTurns] = createResource(
    () => threadId() ?? undefined,
    async (id) => {
      const local = readChatThreadTranscript(id)
      if (local && latestAssistantAnswer(local.turns)) return local.turns
      const remote = await getChatThreadTranscript(id).catch(() => null)
      return remote?.turns.filter(isReadableTranscriptTurn) ?? local?.turns ?? []
    },
  )
  const persistedAnswer = createMemo(() => latestAssistantAnswer(threadTurns() ?? []))
  const visibleAnswer = createMemo(() => answer() || persistedAnswer())
  const [nextAction, setNextAction] = createSignal<string | null>(null)
  const [nextActionProposal, setNextActionProposal] = createSignal<NextActionProposal | null>(null)
  const [nextActionLoading, setNextActionLoading] = createSignal<'suggest' | NextActionKind | null>(null)
  let requestId = 0

  const scope = (): SupportChatThreadScope | null => {
    const orgId = props.orgId.trim()
    const conversationId = props.conversationId.trim()
    const userId = props.userId.trim()
    return userId && orgId && conversationId ? { userId, orgId, conversationId } : null
  }

  const ticketID = () => props.ticketId?.trim() ?? ''
  const evidenceMessageIDs = () => [...new Set(
    props.messages
      .map((message) => message.id?.trim())
      .filter((id): id is string => Boolean(id)),
  )].slice(0, 25)
  const ticketUpdateDetails = (fields: Record<string, unknown>) => {
    const labels: Record<string, [string, string]> = {
      category: ['Kategori', 'Category'],
      intent: ['Hensikt', 'Intent'],
      work_type: ['Arbeidstype', 'Work type'],
      priority: ['Prioritet', 'Priority'],
      severity: ['Alvorlighet', 'Severity'],
      status: ['Status', 'Status'],
      team_name: ['Team', 'Team'],
    }
    const details = Object.keys(labels)
      .flatMap((key) => typeof fields[key] === 'string' ? [`${i18n.tr(...labels[key]!)}: ${fields[key]}`] : [])
    return `${i18n.tr('Foreslåtte felt', 'Proposed fields')}: ${details.join(' · ')}`
  }

  createEffect(
    () => scope(),
    (current) => {
      requestId += 1
      setLoading(false)
      setQuestion('')
      setAnswer(null)
      setSources([])
      setRunMetadata(null)
      setError(null)
      setThreadId(current ? readSupportChatThread(current) : null)
      setNextAction(null)
      setNextActionProposal(null)
      setNextActionLoading(null)
    },
  )

  const matchesScope = (expected: SupportChatThreadScope) => {
    const current = scope()
    return Boolean(
      current &&
      current.userId === expected.userId &&
      current.orgId === expected.orgId &&
      current.conversationId === expected.conversationId,
    )
  }

  const bindResultThread = (currentScope: SupportChatThreadScope, resultThreadId?: string) => {
    if (!resultThreadId || !matchesScope(currentScope)) return
    bindSupportChatThread(currentScope, resultThreadId)
    setThreadId(resultThreadId)
  }

  const ask = async () => {
    const currentScope = scope()
    const content = question().trim()
    if (!currentScope || !content || loading()) return
    const currentRequest = ++requestId
    const isCurrentScope = () => {
      return matchesScope(currentScope)
    }
    setLoading(true)
    setError(null)
    setSources([])
    try {
      const result = await runAssist(props.orgId, 'ask', props.messages, {
        contextPack: props.contextPack,
        customer: props.customer,
        question: content,
        threadId: readSupportChatThread(currentScope) ?? undefined,
      })
      if (currentRequest !== requestId || !isCurrentScope()) return
      bindResultThread(currentScope, result.threadId)
      setAnswer(result.text || i18n.tr('Verevon hadde ikke noe svar.', 'Verevon had no answer.'))
      setSources(result.sources)
      setRunMetadata(result)
    } catch {
      if (currentRequest === requestId && isCurrentScope()) setError(i18n.tr('Verevon kunne ikke svare. Prøv igjen.', 'Verevon could not answer. Try again.'))
    } finally {
      if (currentRequest === requestId && isCurrentScope()) setLoading(false)
    }
  }

  const suggestNextAction = async () => {
    const currentScope = scope()
    if (!currentScope || nextActionLoading()) return
    setNextActionLoading('suggest')
    setNextActionProposal(null)
    setError(null)
    try {
      const result = await runAssist(props.orgId, 'ask', props.messages, {
        contextPack: props.contextPack,
        customer: props.customer,
        question: i18n.tr(
          'Hva er den ene tryggeste neste handlingen i denne saken? Start med "Anbefalt neste steg:" og forklar kort hvorfor. Ikke utfør handlingen og ikke foreslå at den er gjennomført.',
          'What is the single safest next action for this case? Start with "Recommended next step:" and briefly explain why. Do not execute the action or imply that it was completed.',
        ),
        threadId: readSupportChatThread(currentScope) ?? undefined,
      })
      if (!matchesScope(currentScope)) return
      bindResultThread(currentScope, result.threadId)
      setNextAction(result.text || i18n.tr('Verevon fant ingen trygg neste handling.', 'Verevon found no safe next action.'))
      setSources(result.sources)
      setRunMetadata(result)
    } catch {
      if (matchesScope(currentScope)) setError(i18n.tr('Verevon kunne ikke foreslå neste handling.', 'Verevon could not suggest the next action.'))
    } finally {
      if (matchesScope(currentScope)) setNextActionLoading(null)
    }
  }

  const prepareNextAction = async (kind: NextActionKind) => {
    if (kind === 'ticketUpdate') {
      await prepareTicketUpdate()
      return
    }
    const currentScope = scope()
    if (!currentScope || nextActionLoading()) return
    setNextActionLoading(kind)
    setNextActionProposal(null)
    setError(null)
    try {
      const result = await runAssist(props.orgId, 'draft', props.messages, {
        contextPack: props.contextPack,
        customer: props.customer,
        instruction: kind === 'reply'
          ? 'Prepare the customer reply needed for the recommended next action. Return only the editable reply body. Do not claim any action has already happened.'
          : 'Prepare a concise private internal note describing the recommended next action. Return only the note body. Do not address the customer and do not claim any action has already happened.',
        threadId: readSupportChatThread(currentScope) ?? undefined,
      })
      if (!matchesScope(currentScope)) return
      bindResultThread(currentScope, result.threadId)
      setRunMetadata(result)
      const text = result.text.trim()
      if (!text) throw new Error('empty_next_action_draft')
      if (result.zdr || result.supportAiMode !== 'review') {
        setNextActionProposal({ kind, status: 'transient', text })
        return
      }
      if (kind === 'reply') {
        await createDraftReplyProposal({ conversationId: currentScope.conversationId, bodyText: text })
      } else {
        await createInternalNoteProposal({ conversationId: currentScope.conversationId, bodyText: text })
      }
      if (!matchesScope(currentScope)) return
      setNextActionProposal({ kind, status: 'queued', text })
    } catch {
      if (matchesScope(currentScope)) setError(i18n.tr('Forslaget kunne ikke opprettes. Ingen handling er utført.', 'The proposal could not be created. No action was executed.'))
    } finally {
      if (matchesScope(currentScope)) setNextActionLoading(null)
    }
  }

  const prepareTicketUpdate = async () => {
    const currentScope = scope()
    const ticketId = ticketID()
    const evidenceMessageIds = evidenceMessageIDs()
    if (!currentScope || !ticketId || evidenceMessageIds.length === 0 || nextActionLoading()) return
    setNextActionLoading('ticketUpdate')
    setNextActionProposal(null)
    setError(null)
    try {
      const result = await runAssist(props.orgId, 'triage', props.messages, {
        contextPack: props.contextPack,
        customer: props.customer,
        threadId: readSupportChatThread(currentScope) ?? undefined,
      })
      if (!matchesScope(currentScope)) return
      bindResultThread(currentScope, result.threadId)
      setRunMetadata(result)
      const proposal = parseInboxTriageProposal(result.text)
      if (!proposal) throw new Error('invalid_ticket_update_proposal')
      const preview = `${proposal.reason}\n\n${ticketUpdateDetails(proposal.suggestedFields as Record<string, unknown>)}`
      if (result.zdr || result.supportAiMode !== 'review') {
        setNextActionProposal({ kind: 'ticketUpdate', status: 'transient', text: preview })
        return
      }
      await createTicketUpdateProposal({
        conversationId: currentScope.conversationId,
        ticketId,
        confidence: proposal.confidence,
        reason: proposal.reason,
        evidenceMessageIds,
        suggestedFields: proposal.suggestedFields,
      })
      if (!matchesScope(currentScope)) return
      setNextActionProposal({ kind: 'ticketUpdate', status: 'queued', text: preview })
    } catch {
      if (matchesScope(currentScope)) setError(i18n.tr('Saksoppdateringsforslaget kunne ikke opprettes. Ingen sak er endret.', 'The ticket-update proposal could not be created. No ticket was changed.'))
    } finally {
      if (matchesScope(currentScope)) setNextActionLoading(null)
    }
  }

  return (
    <div class="verevon-support-verevon-composer">
      <div class="verevon-support-verevon-composer__intro">
        <Sparkles class="size-4" />
        <div>
          <strong>Verevon</strong>
          <span>{props.contextLabel}</span>
        </div>
        <Show when={Boolean(threadId())}>
          <>
            <a href={`/support?view=all&conversation_id=${encodeURIComponent(props.conversationId)}`} link>
              {i18n.tr('Åpne kildesamtale', 'Open source conversation')}<ArrowUpRight class="size-3.5" />
            </a>
          </>
        </Show>
      </div>

      <section
        class="verevon-support-verevon-composer__conversation"
        aria-label={i18n.tr('Samtalegrunnlag', 'Conversation context')}
      >
        <div class="verevon-support-verevon-composer__conversation-heading">
          <strong>{i18n.tr('Samtalegrunnlag', 'Conversation context')}</strong>
          <span>
            {props.conversationLoading
              ? i18n.tr('Laster …', 'Loading…')
              : conversationSource() === 'authorized'
                ? `${props.messages.length} ${i18n.tr('meldinger', 'messages')}`
                : conversationSource() === 'preview'
                  ? i18n.tr('Kun forhåndsvisning', 'Preview only')
                  : i18n.tr('Ikke tilgjengelig', 'Unavailable')}
          </span>
        </div>
        <Show
          when={props.messages.length > 0}
          fallback={
            <p class="verevon-support-verevon-composer__conversation-empty">
              {props.conversationLoading
                ? i18n.tr('Henter den autoriserte samtalen …', 'Loading the authorized conversation…')
                : i18n.tr('Ingen samtalemeldinger er tilgjengelige i denne lesingen.', 'No conversation messages are available in this authorized read.')}
            </p>
          }
        >
          <div class="verevon-support-verevon-composer__conversation-list">
            <For each={props.messages.slice(-6)}>
              {(message) => (
                <article class="verevon-support-verevon-composer__conversation-message">
                  <div>
                    <strong>
                      {message.internal
                        ? i18n.tr('Privat notat', 'Internal note')
                        : message.agent
                          ? message.from || i18n.tr('Support', 'Support')
                          : message.from || i18n.tr('Kunde', 'Customer')}
                    </strong>
                    <span>{message.internal ? i18n.tr('Kun for teamet', 'Team-only') : message.agent ? i18n.tr('Team', 'Team') : i18n.tr('Kunde', 'Customer')}</span>
                  </div>
                  <p>{message.body}</p>
                </article>
              )}
            </For>
          </div>
          <small>
            {conversationSource() === 'authorized'
              ? i18n.tr('Verevon bruker denne autoriserte samtalekonteksten sammen med valgt sak.', 'Verevon receives this permission-scoped conversation context together with the selected case.')
              : i18n.tr('Bare den synlige forhåndsvisningen er tilgjengelig; Verevon skal ikke anta resten av historikken.', 'Only the visible preview is available; Verevon must not infer the rest of the history.')}
          </small>
        </Show>
      </section>

      <Show when={visibleAnswer()}>
        {(value) => (
          <section class="verevon-support-verevon-composer__answer" aria-label={i18n.tr('Verevon-svar', 'Verevon answer')} aria-live="polite">
            <div class="verevon-support-verevon-composer__answer-heading">
              <strong>Verevon</strong>
              <Show when={persistedAnswer() && !answer()}>
                <span>{i18n.tr('Fra Chat', 'From Chat')}</span>
              </Show>
            </div>
            <p>{value()}</p>
          </section>
        )}
      </Show>
      <Show when={runMetadata()}>{(metadata) => <SupportAssistDisclosure metadata={metadata()} />}</Show>
      <section class="verevon-support-verevon-composer__next-action" aria-label={i18n.tr('Neste handling', 'Next action')}>
        <div class="verevon-support-verevon-composer__next-action-heading">
          <strong>{i18n.tr('Neste handling', 'Next action')}</strong>
          <button type="button" disabled={!scope() || nextActionLoading() !== null} onClick={() => void suggestNextAction()}>
            {nextActionLoading() === 'suggest' ? i18n.tr('Foreslår …', 'Suggesting…') : i18n.tr('Foreslå neste handling', 'Suggest next action')}
          </button>
        </div>
        <Show when={nextAction()}>
          {(suggestion) => (
            <>
              <p>{suggestion()}</p>
              <div class="verevon-support-verevon-composer__next-action-buttons">
                <button type="button" disabled={nextActionLoading() !== null} onClick={() => void prepareNextAction('reply')}>
                  {nextActionLoading() === 'reply' ? i18n.tr('Forbereder …', 'Preparing…') : i18n.tr('Forbered kundesvar', 'Prepare customer reply')}
                </button>
                <button type="button" disabled={nextActionLoading() !== null} onClick={() => void prepareNextAction('note')}>
                  {nextActionLoading() === 'note' ? i18n.tr('Forbereder …', 'Preparing…') : i18n.tr('Opprett internt notat', 'Create internal note')}
                </button>
                <Show when={ticketID()}>
                  <button type="button" disabled={nextActionLoading() !== null || evidenceMessageIDs().length === 0} onClick={() => void prepareNextAction('ticketUpdate')}>
                    {nextActionLoading() === 'ticketUpdate' ? i18n.tr('Forbereder …', 'Preparing…') : i18n.tr('Forbered saksoppdatering', 'Prepare ticket update')}
                  </button>
                </Show>
              </div>
            </>
          )}
        </Show>
        <Show when={nextActionProposal()}>
          {(proposal) => (
            <div class="verevon-support-verevon-composer__next-action-proposal" role="status">
              <strong>
                {proposal().status === 'queued'
                  ? i18n.tr('Sendt til gjennomgang', 'Sent for review')
                  : i18n.tr('Midlertidig forslag', 'Transient proposal')}
              </strong>
              <p>{proposal().text}</p>
              <small>
                {proposal().status === 'queued'
                  ? i18n.tr('Forslaget er lagret for menneskelig gjennomgang. Ingen kundemelding eller ticketendring er utført.', 'The proposal is retained for human review. No customer message or ticket change was executed.')
                  : i18n.tr('Assist/ZDR er aktiv: teksten er bare synlig her og er ikke lagret.', 'Assist/ZDR is active: this text is visible only here and was not retained.')}
              </small>
            </div>
          )}
        </Show>
      </section>
      <Show when={error()}>{(value) => <p role="alert" class="verevon-inbox-error">{value()}</p>}</Show>
      <Show when={sources().length > 0}>
        <ul class="verevon-support-verevon-composer__sources" aria-label={i18n.tr('Kilder', 'Sources')}>
          <For each={sources()}>{(source) => <li>{source.title || source.uri || i18n.tr('Kilde', 'Source')}</li>}</For>
        </ul>
      </Show>

      <div class="verevon-support-verevon-composer__input">
        <input
          value={question()}
          disabled={!scope() || loading()}
          aria-label={i18n.tr('Spør Verevon om valgt arbeid', 'Ask Verevon about selected work')}
          placeholder={i18n.tr('Spør om valgt arbeid', 'Ask about selected work')}
          onInput={(event) => setQuestion(event.currentTarget.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter' && !event.shiftKey) {
              event.preventDefault()
              void ask()
            }
          }}
        />
        <button type="button" disabled={!scope() || !question().trim() || loading()} onClick={() => void ask()} aria-label={i18n.tr('Send spørsmål til Verevon', 'Send question to Verevon')}>
          <Send class="size-3.5" />
        </button>
      </div>
      <small>{i18n.tr('Bruker valgt arbeid og autorisert kunnskap. Handlinger krever fortsatt gjennomgang.', 'Uses selected work and authorized knowledge. Actions still require review.')}</small>
    </div>
  )
}

function isReadableTranscriptTurn(turn: unknown): turn is ReadableTranscriptTurn {
  if (!turn || typeof turn !== 'object') return false
  const record = turn as Record<string, unknown>
  return (record.role === 'assistant' || record.role === 'user') && typeof record.content === 'string'
}
