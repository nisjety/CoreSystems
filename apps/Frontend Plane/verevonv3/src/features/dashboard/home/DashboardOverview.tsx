import { createEffect, createMemo, createSignal, For, Show } from 'solid-js'
import { ArrowUpRight, BookOpen, Folder, MessageCircleMore, Pin, RefreshCw, Sparkles } from '@/shared/icons'
import { listChatThreads } from '@/shared/api/chat-client'
import { getSession } from '@/shared/session/session-store'
import { useI18n } from '@/shared/i18n'
import { selectChatThread, type ChatThreadHistoryItem } from '@/features/chat/lib/chat-thread-history'
import { DashboardImageCard } from './DashboardCards'
import { DashboardUpdatesCard } from './DashboardUpdatesCard'
import { dashboardCards, type DashboardCard } from './dashboard-cards'

/** Read-only overview. Uses the same scoped history API as Chat; it never
 * writes snapshots or manufactures metrics from the browser's history cap. */
export function DashboardRecentWork() {
  const i18n = useI18n()
  const session = getSession()
  const [threads, setThreads] = createSignal<ChatThreadHistoryItem[]>([])
  const [loading, setLoading] = createSignal(true)
  const [error, setError] = createSignal(false)
  const [pinnedOnly, setPinnedOnly] = createSignal(false)
  const [refresh, setRefresh] = createSignal(0)
  createEffect(
    () => ({ user: session.user?.id, org: session.activeOrg?.id, refresh: refresh() }),
    ({ user, org }) => {
      let cancelled = false
      setThreads([])
      setError(false)
      setLoading(Boolean(user && org))
      if (user && org) void listChatThreads().then((items) => {
        if (!cancelled) setThreads(items)
      }).catch(() => { if (!cancelled) setError(true) })
        .finally(() => { if (!cancelled) setLoading(false) })
      return () => { cancelled = true }
    },
  )
  const visible = createMemo(() => [...threads()]
    .filter((thread) => !pinnedOnly() || thread.pinned)
    .sort((a, b) => Date.parse(b.updatedAt) - Date.parse(a.updatedAt))
    .slice(0, 4))
  const dateLabel = (value: string) => {
    const date = new Date(value)
    return Number.isNaN(date.getTime()) ? '' : date.toLocaleDateString(i18n.locale() === 'no' ? 'nb-NO' : 'en-GB', { day: 'numeric', month: 'short' })
  }
  return (
    <section class="home-overview-card home-recent" aria-labelledby="home-recent-title" aria-busy={loading() ? 'true' : 'false'}>
      <header class="home-overview-card__header">
        <div><span class="home-eyebrow">{i18n.tr('Arbeidsområdet ditt', 'Your workspace')}</span><h2 id="home-recent-title">{i18n.tr('Fortsett der du slapp', 'Pick up where you left off')}</h2></div>
        <button type="button" class="home-quiet-button" aria-pressed={pinnedOnly() ? 'true' : 'false'} onClick={() => setPinnedOnly((value) => !value)} title={i18n.tr('Vis festede samtaler', 'Show pinned conversations')}><Pin size={14} />{i18n.tr('Festet', 'Pinned')}</button>
      </header>
      <Show when={!loading()} fallback={<div class="home-recent__empty" role="status">{i18n.tr('Henter samtalene dine …', 'Loading your conversations…')}</div>}>
        <Show when={!error()} fallback={<div class="home-recent__empty" role="status"><p>{i18n.tr('Samtalene kunne ikke lastes.', 'Conversations could not be loaded.')}</p><button type="button" class="home-quiet-button" onClick={() => setRefresh((value) => value + 1)}><RefreshCw size={14} />{i18n.tr('Prøv igjen', 'Try again')}</button></div>}>
          <Show when={visible().length} fallback={<div class="home-recent__empty"><MessageCircleMore size={24} /><p>{pinnedOnly() ? i18n.tr('Ingen festede samtaler ennå.', 'No pinned conversations yet.') : i18n.tr('Det neste du jobber med, starter her.', 'Your next piece of work starts here.')}</p><span>{i18n.tr('Still et spørsmål i skrivefeltet nedenfor.', 'Ask a question in the chat bar below.')}</span></div>}>
            <div class="home-recent__list"><For each={visible()}>{(thread) => (
              <a link href="/chat" class="home-recent__row" onClick={() => selectChatThread(thread.threadId)}>
                <span class="home-recent__icon"><MessageCircleMore size={18} /></span>
                <span class="home-recent__copy"><strong>{thread.title}</strong><small>{thread.preview && thread.preview !== thread.title ? thread.preview : i18n.tr('Fortsett samtalen', 'Continue conversation')}</small></span>
                <time datetime={thread.updatedAt}>{dateLabel(thread.updatedAt)}</time><ArrowUpRight size={14} />
              </a>
            )}</For></div>
          </Show>
        </Show>
      </Show>
      <footer><a link href="/chat">{i18n.tr('Alle samtaler', 'All conversations')}<ArrowUpRight size={14} /></a><span>{i18n.tr('Nylig oppdatert', 'Recently updated')}</span></footer>
    </section>
  )
}

export function DashboardOverview(props: { onPrompt: (card: DashboardCard) => void }) {
  const i18n = useI18n()
  return (
    <section class="home-overview-grid" aria-label={i18n.tr('Oversikt over arbeidsområdet', 'Workspace overview')}>
      <DashboardRecentWork />
      <For each={dashboardCards.filter((card) => card.id === 'weather')}>{(card) => <DashboardImageCard card={card} onPrompt={props.onPrompt} />}</For>
      <DashboardUpdatesCard onPrompt={props.onPrompt} />
      <section class="home-overview-card home-destinations" aria-labelledby="home-destinations-title">
        <header><span class="home-eyebrow">{i18n.tr('Verktøyene dine', 'Your tools')}</span><h2 id="home-destinations-title">{i18n.tr('Klar for neste steg', 'Ready for what’s next')}</h2></header>
        <nav aria-label={i18n.tr('Snarveier i arbeidsområdet', 'Workspace shortcuts')}>
          <a link href="/knowledge"><BookOpen size={18} /><span><strong>{i18n.tr('Kunnskap', 'Knowledge')}</strong><small>{i18n.tr('Kilder og dokumenter', 'Sources & documents')}</small></span><ArrowUpRight size={14} /></a>
          <a link href="/agents"><Sparkles size={18} /><span><strong>{i18n.tr('Agenter', 'Agents')}</strong><small>{i18n.tr('Arbeidsflyter og kjøringer', 'Workflows & runs')}</small></span><ArrowUpRight size={14} /></a>
          <a link href="/spaces"><Folder size={18} /><span><strong>{i18n.tr('Rom', 'Spaces')}</strong><small>{i18n.tr('Arbeid sammen', 'Work together')}</small></span><ArrowUpRight size={14} /></a>
        </nav>
      </section>
    </section>
  )
}
