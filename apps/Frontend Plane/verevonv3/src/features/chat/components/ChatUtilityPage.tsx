import { useNavigate } from '@solidjs/router'
import { createMemo, createSignal, For, Show } from 'solid-js'
import { createResource } from '@/shared/lib/create-resource-compat'
import { listNotifications } from '@/shared/api/notifications-client'
import { listConversations } from '@/shared/api/inbox-client'
import { listChatThreads } from '@/shared/api/chat-client'
import { getSession } from '@/shared/session/session-store'
import { useI18n } from '@/shared/i18n'
import { readChatThreadTranscript, selectChatThread } from '../lib/chat-thread-history'
import { PROSE_ARTIFACT_KINDS } from './chat-types'
import { buildPromptSuggestions } from '../lib/prompt-suggestions'
import { ScheduleDialog } from './ScheduleDialog'
import { ScheduledRunsPanel } from './ScheduledRunsPanel'

type Section = 'findings' | 'reports' | 'routines'
type ReportRow = { id: string; title: string; threadTitle: string; threadId: string; updatedAt: string; source: 'artifact' | 'thread' }

function readableDate(value: string, locale: string): string {
  const date = new Date(value)
  return Number.isNaN(date.getTime()) ? '' : date.toLocaleDateString(locale, { dateStyle: 'medium' })
}

export default function ChatUtilityPage(props: { section: Section }) {
  const i18n = useI18n()
  const navigate = useNavigate()
  const session = getSession()
  const orgId = createMemo(() => session.activeOrg?.id ?? '')
  const userId = createMemo(() => session.user?.id ?? '')
  const locale = () => i18n.locale() === 'no' ? 'nb-NO' : 'en-US'
  const [scheduleOpen, setScheduleOpen] = createSignal(false)

  const [findingFeed] = createResource(
    () => props.section === 'findings' && orgId() && userId() ? [orgId(), userId()] as const : undefined,
    async ([org, user]) => {
      const [notificationResult, inboxResult] = await Promise.allSettled([
        listNotifications(),
        listConversations(org, { limit: 30, state: 'open' }),
      ])
      if (notificationResult.status === 'rejected' && inboxResult.status === 'rejected') {
        throw new Error('Finding feeds unavailable')
      }
      return buildPromptSuggestions({
        notifications: notificationResult.status === 'fulfilled' ? notificationResult.value : [],
        conversations: inboxResult.status === 'fulfilled' ? inboxResult.value.tickets : [],
        orgId: org,
        userId: user,
      })
    },
  )

  const [threads] = createResource(
    () => props.section === 'reports' && orgId() && userId() ? [orgId(), userId()] as const : undefined,
    () => listChatThreads(),
  )
  const reports = createMemo(() => (threads() ?? []).flatMap<ReportRow>((thread) => {
    const transcript = readChatThreadTranscript(thread.threadId)
    const artifacts = transcript?.turns.flatMap((turn) => (turn.artifacts ?? []).flatMap((value) => {
      if (!value || typeof value !== 'object') return []
      const artifact = value as Record<string, unknown>
      if (typeof artifact.kind !== 'string' || !PROSE_ARTIFACT_KINDS.has(artifact.kind.toLowerCase())) return []
      return [{ id: String(artifact.id ?? `${thread.threadId}:${turn.id}`), title: typeof artifact.title === 'string' ? artifact.title : thread.title, threadTitle: thread.title, threadId: thread.threadId, updatedAt: thread.updatedAt, source: 'artifact' as const }]
    })) ?? []
    if (artifacts.length) return artifacts
    return /rapport|report|brief|styrepakke|prognose|forecast/i.test(thread.title)
      ? [{ id: thread.threadId, title: thread.title, threadTitle: thread.title, threadId: thread.threadId, updatedAt: thread.updatedAt, source: 'thread' as const }]
      : []
  }).sort((a, b) => b.updatedAt.localeCompare(a.updatedAt)))
  const openReport = (threadId: string) => {
    selectChatThread(threadId)
    navigate('/chat')
  }

  return (
    <main class="chat-utility-page">
      <div class="chat-utility-page__inner">
        <Show when={props.section === 'findings'}>
          <header class="chat-utility-page__header">
            <div><h1>{i18n.tr('Funn', 'Findings')}</h1><p>{i18n.tr('Nye varsler og åpne saker som kan trenge oppfølging.', 'New alerts and open cases that may need follow-up.')}</p></div>
          </header>
          <Show when={!findingFeed.loading} fallback={<p role="status">{i18n.tr('Laster funn…', 'Loading findings…')}</p>}>
            <Show when={!findingFeed.error} fallback={<p role="alert">{i18n.tr('Kunne ikke hente funn akkurat nå.', 'Findings could not be loaded right now.')}</p>}>
              <Show when={findingFeed()?.length} fallback={<p class="chat-utility-page__empty">{i18n.tr('Ingen funn som trenger oppfølging akkurat nå.', 'No findings need follow-up right now.')}</p>}>
                <ul class="chat-utility-page__list"><For each={findingFeed()}>{(item) => (
                  <li>
                    <strong>{item.title}</strong>
                    <small>{item.source === 'alert' ? i18n.tr('Varsel', 'Alert') : i18n.tr('Innboks', 'Inbox')} · {readableDate(item.occurredAt, locale())}</small>
                    <p>{item.detail}</p>
                    <Show when={item.source === 'inbox'}><button type="button" onClick={() => navigate(`/inbox?conversation_id=${encodeURIComponent(item.id.slice('inbox:'.length))}`)}>{i18n.tr('Åpne saken', 'Open case')}</button></Show>
                  </li>
                )}</For></ul>
              </Show>
            </Show>
          </Show>
        </Show>

        <Show when={props.section === 'reports'}>
          <header class="chat-utility-page__header"><div><h1>{i18n.tr('Rapporter', 'Reports')}</h1><p>{i18n.tr('Rapportartefakter på denne enheten og rapportrelaterte samtaler.', 'Report artifacts on this device and report-related chats.')}</p></div></header>
          <Show when={!threads.loading} fallback={<p role="status">{i18n.tr('Laster rapporter…', 'Loading reports…')}</p>}>
            <Show when={!threads.error} fallback={<p role="alert">{i18n.tr('Kunne ikke hente samtaler akkurat nå.', 'Chats could not be loaded right now.')}</p>}>
              <Show when={reports().length} fallback={<p class="chat-utility-page__empty">{i18n.tr('Ingen rapporter eller rapportrelaterte samtaler ennå.', 'No reports or report-related chats yet.')}</p>}>
                <ul class="chat-utility-page__list"><For each={reports()}>{(item) => (
                  <li><button type="button" onClick={() => openReport(item.threadId)}><strong>{item.title}</strong><small>{item.source === 'artifact' ? i18n.tr('Rapportartefakt', 'Report artifact') : i18n.tr('Samtale', 'Chat')} · {readableDate(item.updatedAt, locale())}</small></button></li>
                )}</For></ul>
              </Show>
            </Show>
          </Show>
          <a href="/insights" link class="chat-utility-page__link">{i18n.tr('Se målte innsikter', 'View measured insights')}</a>
        </Show>

        <Show when={props.section === 'routines'}>
          <header class="chat-utility-page__header"><div><h1>{i18n.tr('Rutiner', 'Routines')}</h1><p>{i18n.tr('Dine planlagte chatkjøringer. Oppgaver som krever handling, blir utkast til du godkjenner dem.', 'Your scheduled chat runs. Tasks requiring action remain drafts until you approve them.')}</p></div><button type="button" onClick={() => setScheduleOpen(true)}>{i18n.tr('Ny rutine', 'New routine')}</button></header>
          <ScheduledRunsPanel />
          <Show when={scheduleOpen()}><ScheduleDialog onClose={() => setScheduleOpen(false)} seedTitle="" seedGoal="" threadOriginHint="composer" /></Show>
        </Show>
      </div>
    </main>
  )
}
