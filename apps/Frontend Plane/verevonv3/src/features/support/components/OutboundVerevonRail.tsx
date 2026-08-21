import { ArrowUpRight, Sparkles } from '@/shared/icons'
import { createEffect, createMemo, createSignal, For, Show } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import { SourceRow } from '@/features/inbox/components/InboxAsidePrimitives'
import { runAssist, type AssistResult, type AssistSource } from '@/features/inbox/lib/inbox-ai'
import { readChatThreadTranscript, setActiveChatThreadId, type ChatThreadTranscriptTurn } from '@/features/chat/lib/chat-thread-history'
import { getChatThreadTranscript } from '@/shared/api/chat-client'
import { type OutboundIntent } from '@/shared/api/inbox-client'
import { bindSupportChatThread, readSupportChatThread, type SupportChatThreadScope } from '@/shared/chat/support-chat-thread'
import { buildModelContextPack, type ModelContextPack } from '@/shared/context-packs/context-pack'
import { useI18n } from '@/shared/i18n'
import { SupportAssistDisclosure, type SupportAssistRunMetadata } from './SupportAssistDisclosure'

type ReadableTranscriptTurn = Pick<ChatThreadTranscriptTurn, 'content' | 'role' | 'status'>

function bounded(value: string | undefined, fallback: string, max = 120): string {
  const normalized = value?.trim()
  return normalized ? normalized.slice(0, max) : fallback
}

function latestAssistantAnswer(turns: readonly ReadableTranscriptTurn[]): string | null {
  for (let index = turns.length - 1; index >= 0; index -= 1) {
    const turn = turns[index]
    if (turn?.role !== 'assistant' || turn.status === 'error' || turn.status === 'stopped') continue
    const answer = turn.content.trim()
    if (answer) return answer
  }
  return null
}

/** Builds a strictly content-free prompt context. Outbound's reconciliation
 * rail receives receipt metadata only: no recipient, message, campaign, or
 * send payload becomes model input. */
export function buildOutboundReceiptContext(intent: OutboundIntent | null, orgId: string): ModelContextPack {
  const receiptId = bounded(intent?.id, 'none')
  const conversationId = bounded(intent?.conversation_id, 'none')
  const provider = bounded(intent?.provider, 'unknown')
  const workStatus = bounded(intent?.status, 'unknown', 40)
  const deliveryStatus = bounded(intent?.delivery_status, 'unconfirmed', 40)
  const errorCode = bounded(intent?.delivery_error_code ?? intent?.error_code, 'none', 80)
  const receiptStatus = `work=${workStatus}; delivery=${deliveryStatus}; error=${errorCode}`

  return buildModelContextPack({
    route: '/support?surface=outbound',
    selectedEntity: intent ? {
      type: 'run',
      id: receiptId,
      label: `Outbound receipt ${receiptId}`,
      status: receiptStatus,
    } : undefined,
    visibleItems: intent ? [{
      type: 'run',
      id: receiptId,
      label: `Outbound receipt ${receiptId}`,
      status: receiptStatus,
    }] : [],
    filters: intent ? { provider, work_status: workStatus, delivery_status: deliveryStatus } : {},
    support: intent ? {
      organization: orgId.trim() ? { id: orgId.trim() } : undefined,
      conversation: {
        id: conversationId,
        channel: provider,
        status: receiptStatus,
      },
      // This rail is explanatory: it deliberately has no send, retry, draft,
      // campaign, or recipient-management action contract.
      availableActions: [],
      permissions: ['support.read', 'support.outbound.read'],
    } : { permissions: ['support.read', 'support.outbound.read'] },
  })
}

export function OutboundVerevonRail(props: { intent: OutboundIntent | null; orgId: string; userId: string }) {
  const i18n = useI18n()
  const [question, setQuestion] = createSignal('')
  const [answer, setAnswer] = createSignal<string | null>(null)
  const [sources, setSources] = createSignal<AssistSource[]>([])
  const [runMetadata, setRunMetadata] = createSignal<SupportAssistRunMetadata | null>(null)
  const [error, setError] = createSignal<string | null>(null)
  const [loading, setLoading] = createSignal(false)
  const [threadId, setThreadId] = createSignal<string | null>(null)
  let requestId = 0

  const scope = (): SupportChatThreadScope | null => {
    const conversationId = props.intent?.conversation_id.trim() ?? ''
    const orgId = props.orgId.trim()
    const userId = props.userId.trim()
    return conversationId && orgId && userId ? { conversationId, orgId, userId } : null
  }
  const contextPack = createMemo(() => buildOutboundReceiptContext(props.intent, props.orgId))
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

  createEffect(
    () => scope(),
    (current) => {
      requestId += 1
      setQuestion('')
      setAnswer(null)
      setSources([])
      setRunMetadata(null)
      setError(null)
      setLoading(false)
      setThreadId(current ? readSupportChatThread(current) : null)
    },
  )

  const matchesScope = (expected: SupportChatThreadScope) => {
    const current = scope()
    return Boolean(current
      && current.conversationId === expected.conversationId
      && current.orgId === expected.orgId
      && current.userId === expected.userId)
  }

  const ask = async (requestedQuestion: string) => {
    const currentScope = scope()
    const content = requestedQuestion.trim()
    if (!currentScope || !content || loading()) return
    const currentRequest = ++requestId
    setLoading(true)
    setError(null)
    setSources([])
    try {
      const result = await runAssist(props.orgId, 'outbound', [], {
        contextPack: contextPack(),
        question: content,
        threadId: readSupportChatThread(currentScope) ?? undefined,
      })
      if (currentRequest !== requestId || !matchesScope(currentScope)) return
      const resultingThreadId = bindResultThread(currentScope, result)
      if (resultingThreadId) setThreadId(resultingThreadId)
      setAnswer(result.text || i18n.tr('Verevon hadde ingen forklaring.', 'Verevon had no explanation.'))
      setSources(result.sources)
      setRunMetadata(result)
    } catch {
      if (currentRequest === requestId && matchesScope(currentScope)) {
        setError(i18n.tr('Verevon kunne ikke forklare kvitteringen. Ingen handling er utført.', 'Verevon could not explain the receipt. No action was executed.'))
      }
    } finally {
      if (currentRequest === requestId && matchesScope(currentScope)) setLoading(false)
    }
  }

  return (
    <section class="verevon-outbound-verevon" aria-label={i18n.tr('Verevon for utgående kvittering', 'Verevon for outbound receipt')}>
      <Show
        when={props.intent}
        fallback={<p class="verevon-support-outbound__availability">{i18n.tr('Velg en kvittering før Verevon får utgående kontekst.', 'Select a receipt before Verevon receives outbound context.')}</p>}
      >
        {(intent) => (
          <>
            <header class="verevon-outbound-verevon__header">
              <div><Sparkles class="size-4" /><strong>Verevon</strong></div>
              <Show when={threadId()}>{(id) => <a href="/chat" link onClick={() => setActiveChatThreadId(id())}>{i18n.tr('Åpne i Chat', 'Open in Chat')}<ArrowUpRight class="size-3.5" /></a>}</Show>
            </header>

            <section class="verevon-outbound-verevon__context" aria-label={i18n.tr('Kvitteringskontekst', 'Receipt context')}>
              <strong>{i18n.tr('Kvitteringskontekst', 'Receipt context')}</strong>
              <dl>
                <div><dt>{i18n.tr('Kanal', 'Channel')}</dt><dd>{bounded(intent().provider, i18n.tr('Ukjent', 'Unknown'))}</dd></div>
                <div><dt>{i18n.tr('Arbeidsstatus', 'Work status')}</dt><dd>{intent().status}</dd></div>
                <div><dt>{i18n.tr('Leveringsbevis', 'Delivery evidence')}</dt><dd>{intent().delivery_status ?? 'unconfirmed'}</dd></div>
                <Show when={intent().delivery_error_code || intent().error_code}><div><dt>{i18n.tr('Feilkode', 'Error code')}</dt><dd>{intent().delivery_error_code || intent().error_code}</dd></div></Show>
              </dl>
              <p>{i18n.tr('Verevon får bare denne kvitteringens identifikatorer og statusmetadata. Ingen mottakere, meldingsinnhold, kampanje eller send-payload er tilgjengelig.', 'Verevon receives only this receipt’s identifiers and status metadata. No recipients, message content, campaign, or send payload is available.')}</p>
            </section>

            <div class="verevon-outbound-verevon__controls">
              <button type="button" disabled={loading()} onClick={() => void ask(i18n.tr('Forklar denne kvitteringen. Skill mellom bekreftet leverandørbevis, ukjent utfall og tryggeste manuelle neste steg. Ikke foreslå sending eller automatisk forsøk på nytt.', 'Explain this receipt. Separate confirmed provider evidence, unknown outcome, and the safest manual next step. Do not suggest sending or an automatic retry.'))}>
                {loading() ? i18n.tr('Forklarer …', 'Explaining…') : i18n.tr('Forklar kvittering', 'Explain receipt')}
              </button>
              <label>
                <span>{i18n.tr('Spør Verevon om denne kvitteringen', 'Ask Verevon about this receipt')}</span>
                <textarea value={question()} onInput={(event) => setQuestion(event.currentTarget.value)} rows={3} placeholder={i18n.tr('Hva er bekreftet, og hva må avstemmes?', 'What is confirmed, and what needs reconciliation?')} />
              </label>
              <button type="button" disabled={loading() || !question().trim()} onClick={() => void ask(question())}>{i18n.tr('Spør Verevon', 'Ask Verevon')}</button>
            </div>

            <Show when={error()}>{(message) => <p class="verevon-outbound-verevon__error" role="alert">{message()}</p>}</Show>
            <Show when={visibleAnswer()}>{(value) => <section class="verevon-outbound-verevon__answer" aria-label={i18n.tr('Verevon-svar', 'Verevon answer')} aria-live="polite"><strong>Verevon</strong><p>{value()}</p></section>}</Show>
            <Show when={sources().length}>
              <section class="verevon-outbound-verevon__sources" aria-label={i18n.tr('Relevante kilder', 'Relevant sources')}>
                <strong>{i18n.tr('Relevante kilder', 'Relevant sources')}</strong>
                <For each={sources()}>{(source) => <SourceRow title={source.title || source.uri || i18n.tr('Kilde', 'Source')} uri={source.uri} excerpt={source.excerpt} />}</For>
              </section>
            </Show>
            <Show when={runMetadata()}>{(metadata) => <SupportAssistDisclosure metadata={metadata()} />}</Show>
            <p class="verevon-outbound-verevon__boundary">{i18n.tr('Forklaring er skrivebeskyttet. Verevon kan ikke sende, endre en kampanje, administrere mottakere eller prøve på nytt her.', 'This explanation is read-only. Verevon cannot send, change a campaign, manage recipients, or retry from here.')}</p>
          </>
        )}
      </Show>
    </section>
  )
}

function bindResultThread(scope: SupportChatThreadScope, result: AssistResult): string | null {
  const threadId = result.threadId?.trim()
  if (!threadId) return null
  bindSupportChatThread(scope, threadId)
  setActiveChatThreadId(threadId)
  return threadId
}

function isReadableTranscriptTurn(value: unknown): value is ChatThreadTranscriptTurn {
  return Boolean(value && typeof value === 'object' && typeof (value as ChatThreadTranscriptTurn).content === 'string')
}
