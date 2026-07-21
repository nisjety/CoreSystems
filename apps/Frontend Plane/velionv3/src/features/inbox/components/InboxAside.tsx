import {
  AlertCircle,
  Bot,
  CalendarDays,
  CheckCircle2,
  ChevronDown,
  Clock3,
  FileText,
  MessageCircle,
  Play,
  Plus,
  RefreshCw,
  Send,
  Sparkles,
  UserRound,
} from 'lucide-solid'
import { createMemo, createResource, createSignal, For, Show } from 'solid-js'
import {
  AccordionSection,
  ActivityItem,
  AsideTabButton,
  CalendarEventRow,
  CalendarNoteRow,
  EmptyAsideState,
  FieldRow,
  HealthRow,
  MiniCalendarGrid,
  SourceRow,
} from '@/features/inbox/components/InboxAsidePrimitives'
import type { InboxModalRequest } from '@/features/inbox/components/InboxWorkModal'
import {
  customerName,
  formatDateKey,
  formatRelativeTime,
  titleCase,
  type CalendarEvent,
  type CalendarNote,
  type ZammadArticle,
  type ZammadTicket,
} from '@/features/inbox/lib/inbox-model'
import { runAssist, type AssistMessage, type AssistMode, type AssistSource } from '@/features/inbox/lib/inbox-ai'
import { listTicketMacros, type TicketMacro } from '@/shared/api/tickets-client'
import { cn } from '@/shared/lib/cn'
import { localeDateTime, useI18n } from '@/shared/i18n'

type AsideTab = 'details' | 'velion' | 'calendar' | 'activity'

export function InboxAside(props: {
  orgId: string
  articles: ZammadArticle[]
  recent: RecentConversationRef[]
  onSelectRecent: (conversationId: string) => void
  onInsertQuickReply: (text: string) => void
  onMacroExecuted: () => void
  onOpenModal: (modal: InboxModalRequest) => void
  selectedTicket: ZammadTicket | null
}) {
  const i18n = useI18n()
  const [activeTab, setActiveTab] = createSignal<AsideTab>('details')
  const tabs = createMemo(() => [
    { id: 'details' as const, label: i18n.tr('Detaljer', 'Details') },
    { id: 'velion' as const, label: 'Velion' },
    { id: 'calendar' as const, label: i18n.tr('Kalender', 'Calendar') },
    { id: 'activity' as const, label: i18n.tr('Aktivitet', 'Activity') },
  ])

  return (
    <aside aria-label={i18n.tr('AI og kundekontekst', 'AI and customer context')} class="velion-inbox-aside">
      <div class="velion-inbox-aside__header">
        <div class="velion-inbox-aside__tabs">
          <For each={tabs()}>
            {(tab) => (
              <AsideTabButton active={activeTab() === tab.id} onClick={() => setActiveTab(tab.id)}>
                {tab.label}
              </AsideTabButton>
            )}
          </For>
        </div>
      </div>

      <Show when={activeTab() === 'details'}>
        <DetailsPanel
          onOpenModal={props.onOpenModal}
          onSelectRecent={props.onSelectRecent}
          recent={props.recent}
          selectedTicket={props.selectedTicket}
        />
      </Show>
      <Show when={activeTab() === 'velion'}>
        <VelionPanel
          orgId={props.orgId}
          articles={props.articles}
          onInsertQuickReply={props.onInsertQuickReply}
          onMacroExecuted={props.onMacroExecuted}
          onOpenModal={props.onOpenModal}
          selectedTicket={props.selectedTicket}
        />
      </Show>
      <Show when={activeTab() === 'calendar'}>
        <CalendarPanel selectedTicket={props.selectedTicket} />
      </Show>
      <Show when={activeTab() === 'activity'}>
        <ActivityPanel selectedTicket={props.selectedTicket} />
      </Show>
    </aside>
  )
}

export type RecentConversationRef = {
  conversationId: string
  title: string
  channel: string
  createdAt: string
}

function DetailsPanel(props: {
  onOpenModal: (modal: InboxModalRequest) => void
  onSelectRecent: (conversationId: string) => void
  recent: RecentConversationRef[]
  selectedTicket: ZammadTicket | null
}) {
  const i18n = useI18n()
  return (
    <div class="velion-inbox-aside-scroll">
      <Show
        when={props.selectedTicket}
        fallback={
          <EmptyAsideState
            icon={<UserRound class="size-6" />}
            title={i18n.tr('Ingen kunde valgt', 'No customer selected')}
            body={i18n.tr(
              'Åpne en sak for å se kundedetaljer, tagger og nylige samtaler.',
              'Open a ticket to see customer details, tags, and recent conversations.',
            )}
          />
        }
      >
        {(ticket) => (
          <>
            <section class="velion-inbox-customer-card">
              <div class="velion-inbox-customer-card__identity">
                <div>{(ticket().customer?.firstname?.[0] ?? customerName(ticket())[0] ?? '?').toUpperCase()}</div>
                <div>
                  <div>
                    <h2>{customerName(ticket())}</h2>
                  </div>
                  <Show when={ticket().customer?.email}>
                    <p>{ticket().customer?.email}</p>
                  </Show>
                </div>
              </div>

              <div class="velion-inbox-field-stack">
                <FieldRow label={i18n.tr('Kanal', 'Channel')} value={titleCase(ticket().channel ?? 'email')} />
                <FieldRow label="Status" value={titleCase(ticket().state?.name ?? 'open')} />
                <FieldRow label={i18n.tr('Prioritet', 'Priority')} value={titleCase(ticket().priority?.name ?? 'normal')} />
                <FieldRow
                  label={i18n.tr('Tildelt', 'Assignee')}
                  value={ticket().owner ? `${ticket().owner?.firstname} ${ticket().owner?.lastname}`.trim() : i18n.tr('Ikke tildelt', 'Unassigned')}
                />
                <FieldRow label={i18n.tr('Team-innboks', 'Team inbox')} value={ticket().group?.name ?? 'Support'} />
              </div>
            </section>

            <AccordionSection defaultOpen icon={<FileText class="size-4" />} title={i18n.tr('Samtale', 'Conversation')}>
              <FieldRow label={i18n.tr('Referanse', 'Reference')} value={ticket().number} />
              <FieldRow label={i18n.tr('Emne', 'Subject')} value={ticket().title} />
              <Show when={ticket().customer?.email}>
                <FieldRow label={i18n.tr('E-post', 'Email')} value={ticket().customer!.email} />
              </Show>
              <FieldRow label={i18n.tr('Åpnet', 'Opened')} value={formatRelativeTime(ticket().created_at)} />
            </AccordionSection>

            <AccordionSection defaultOpen icon={<Sparkles class="size-4" />} title={i18n.tr('Tagger', 'Tags')}>
              <Show
                when={(ticket().tags ?? []).length}
                fallback={<p class="velion-inbox-muted">{i18n.tr('Ingen tagger på denne samtalen ennå.', 'No tags on this conversation yet.')}</p>}
              >
                <div class="velion-inbox-detail-tags">
                  <For each={ticket().tags ?? []}>{(tag) => <span class="velion-inbox-detail-tag">{tag}</span>}</For>
                </div>
              </Show>
            </AccordionSection>

            <AccordionSection
              icon={<MessageCircle class="size-4" />}
              title={`${i18n.tr('Nylige samtaler', 'Recent conversations')}${(props.recent?.length ?? 0) ? ` (${props.recent!.length})` : ''}`}
            >
              <Show
                when={(props.recent?.length ?? 0) > 0}
                fallback={<p class="velion-inbox-muted">{i18n.tr('Ingen andre samtaler fra denne kontakten.', 'No other conversations from this contact.')}</p>}
              >
                <ul class="velion-inbox-recent-list">
                  <For each={props.recent ?? []}>
                    {(item) => (
                      <li>
                        <button type="button" onClick={() => props.onSelectRecent(item.conversationId)}>
                          <span class="velion-inbox-recent-list__title">{item.title || i18n.tr('(uten emne)', '(no subject)')}</span>
                          <span class="velion-inbox-recent-list__meta">
                            {titleCase(item.channel)} · {formatRelativeTime(item.createdAt)}
                          </span>
                        </button>
                      </li>
                    )}
                  </For>
                </ul>
              </Show>
            </AccordionSection>
          </>
        )}
      </Show>
    </div>
  )
}

function VelionPanel(props: {
  orgId: string
  articles: ZammadArticle[]
  onInsertQuickReply: (text: string) => void
  onMacroExecuted: () => void
  onOpenModal: (modal: InboxModalRequest) => void
  selectedTicket: ZammadTicket | null
}) {
  const i18n = useI18n()
  const [draft, setDraft] = createSignal<string | null>(null)
  const [draftLoading, setDraftLoading] = createSignal(false)
  const [summary, setSummary] = createSignal<string | null>(null)
  const [summaryLoading, setSummaryLoading] = createSignal(false)
  const [answer, setAnswer] = createSignal<string | null>(null)
  const [answerLoading, setAnswerLoading] = createSignal(false)
  const [sources, setSources] = createSignal<AssistSource[]>([])
  const [error, setError] = createSignal<string | null>(null)
  const [question, setQuestion] = createSignal('')
  const [runningCard, setRunningCard] = createSignal<string | null>(null)

  const transcript = createMemo<AssistMessage[]>(() =>
    props.articles.map((a) => ({
      agent: a.sender?.toLowerCase() === 'agent',
      from: a.from,
      body: a.bodyText || stripToText(a.body ?? ''),
    })),
  )
  const customer = () => (props.selectedTicket ? customerName(props.selectedTicket) : undefined)
  const ready = () => Boolean(props.selectedTicket && props.orgId)
  const applySources = (next: AssistSource[]) => {
    if (next.length) setSources(next)
  }

  const generateDraft = async (instruction?: string) => {
    if (!ready() || draftLoading()) return
    setDraftLoading(true)
    setError(null)
    try {
      const res = await runAssist(props.orgId, 'draft', transcript(), { instruction, customer: customer() })
      setDraft(res.text || i18n.tr('Velion returnerte et tomt svar.', 'Velion returned an empty reply.'))
      applySources(res.sources)
    } catch {
      setError(i18n.tr('Velion kunne ikke generere et svar. Prøv igjen.', 'Velion could not generate a reply. Try again.'))
    } finally {
      setDraftLoading(false)
    }
  }

  const generateSummary = async () => {
    if (!ready() || summaryLoading()) return
    setSummaryLoading(true)
    setError(null)
    try {
      const res = await runAssist(props.orgId, 'summarize', transcript(), { customer: customer() })
      setSummary(res.text || i18n.tr('Ingen oppsummering tilgjengelig.', 'No summary available.'))
    } catch {
      setError(i18n.tr('Velion kunne ikke oppsummere. Prøv igjen.', 'Velion could not summarize. Try again.'))
    } finally {
      setSummaryLoading(false)
    }
  }

  const runCard = async (id: string, mode: AssistMode, instruction?: string) => {
    if (!ready() || runningCard()) return
    setRunningCard(id)
    setError(null)
    try {
      const res = await runAssist(props.orgId, mode, transcript(), { instruction, customer: customer() })
      applySources(res.sources)
      if (mode === 'draft') setDraft(res.text)
      else setAnswer(res.text)
    } catch {
      setError(i18n.tr('Velion-handlingen feilet. Prøv igjen.', 'Velion action failed. Try again.'))
    } finally {
      setRunningCard(null)
    }
  }

  const askVelion = async () => {
    const q = question().trim()
    if (!q || !ready() || answerLoading()) return
    setAnswerLoading(true)
    setError(null)
    try {
      const res = await runAssist(props.orgId, 'ask', transcript(), { question: q, customer: customer() })
      setAnswer(res.text || i18n.tr('Velion hadde ikke noe svar.', 'Velion had no answer.'))
      applySources(res.sources)
    } catch {
      setError(i18n.tr('Velion kunne ikke svare. Prøv igjen.', 'Velion could not answer. Try again.'))
    } finally {
      setAnswerLoading(false)
    }
  }

  return (
    <div class="velion-inbox-velion-panel">
      <div class="velion-inbox-aside-scroll velion-inbox-aside-scroll--panel">
        <Show
          when={props.selectedTicket}
          fallback={
            <EmptyAsideState
              icon={<Bot class="size-6" />}
              title={i18n.tr('Velg en sak', 'Select a ticket')}
              body={i18n.tr(
                'Velion kan utkaste svar, oppsummere kontekst og finne relevante kilder når en samtale er åpen.',
                'Velion can draft replies, summarize context, and surface relevant sources once a conversation is open.',
              )}
            />
          }
        >
          <div class="velion-inbox-card-stack">
            <Show when={error()}>
              <div class="velion-inbox-aside-card velion-inbox-aside-card--error" role="alert">
                {error()}
              </div>
            </Show>

            <section class="velion-inbox-aside-card">
              <div class="velion-inbox-card-heading">
                <Bot class="size-4" />
                <h2>{i18n.tr('Velion handlingsplan', 'Velion action plan')}</h2>
              </div>
              <ActionSuggestion
                title={i18n.tr('Bekreft hensikt', 'Confirm intent')}
                body={i18n.tr("Oppdag kundens primære hensikt og beste neste handling.", "Detect the customer's primary intent and the best next action.")}
                actionLabel={runningCard() === 'intent' ? i18n.tr('Kjører …', 'Running…') : i18n.tr('Kjør', 'Run')}
                onRun={() => void runCard('intent', 'intent')}
              />
              <ActionSuggestion
                title={i18n.tr('Kildebasert svar', 'Source-backed reply')}
                body={i18n.tr(
                  'Utkast et svar som kun er basert på fakta som støttes av samtalen.',
                  'Draft a reply grounded only in facts supported by the conversation.',
                )}
                actionLabel={runningCard() === 'source' ? i18n.tr('Kjører …', 'Running…') : i18n.tr('Kjør', 'Run')}
                onRun={() =>
                  void runCard(
                    'source',
                    'draft',
                    'Only assert facts supported by the transcript; do not invent policy or promises.',
                  )
                }
              />
              <ActionSuggestion
                title={i18n.tr('Vurder og rut', 'Assess & route')}
                body={i18n.tr(
                  'Anbefal om saken skal eskaleres eller rutes, basert på hastegrad og status.',
                  'Recommend whether to escalate or route, based on urgency and status.',
                )}
                actionLabel={runningCard() === 'route' ? i18n.tr('Kjører …', 'Running…') : i18n.tr('Kjør', 'Run')}
                onRun={() =>
                  void runCard(
                    'route',
                    'ask',
                    'Assess urgency and recommend whether this conversation should be routed or escalated. Name the best destination and explain the reason briefly.',
                  )
                }
              />
            </section>

            <Show when={answer()}>
              <section class="velion-inbox-aside-card velion-inbox-aside-card--answer">
                <div class="velion-inbox-card-heading velion-inbox-card-heading--between">
                  <div>
                    <Bot class="size-4" />
                    <h2>Velion</h2>
                  </div>
                  <button type="button" onClick={() => setAnswer(null)} aria-label={i18n.tr('Lukk svar', 'Dismiss answer')}>
                    {i18n.tr('Fjern', 'Clear')}
                  </button>
                </div>
                <p class="velion-inbox-ai-text">{answer()}</p>
              </section>
            </Show>

            <section class="velion-inbox-aside-card velion-inbox-aside-card--soft">
              <div class="velion-inbox-card-heading velion-inbox-card-heading--between">
                <div>
                  <Sparkles class="size-4" />
                  <h2>{i18n.tr('Svarhjelp', 'Reply assistance')}</h2>
                </div>
                <button type="button" disabled={draftLoading()} onClick={() => void generateDraft()}>
                  <RefreshCw class={cn('size-3.5', draftLoading() && 'velion-inbox-spin')} />
                  {draftLoading() ? i18n.tr('Lager utkast …', 'Drafting…') : i18n.tr('Generer', 'Generate')}
                </button>
              </div>
              <Show
                when={!draftLoading()}
                fallback={
                  <div class="velion-inbox-reply-skeleton">
                    <span />
                    <span />
                  </div>
                }
              >
                <Show when={draft()} fallback={<p>{i18n.tr('Generer et forslag til svar basert på denne samtalen.', 'Generate a suggested reply grounded in this conversation.')}</p>}>
                  <p class="velion-inbox-ai-text">{draft()}</p>
                  <div class="velion-inbox-draft-actions">
                    <button type="button" class="velion-inbox-btn-primary" onClick={() => props.onInsertQuickReply(draft() ?? '')}>
                      {i18n.tr('Sett inn i svar', 'Insert into reply')}
                    </button>
                    <button type="button" onClick={() => void generateDraft('Rewrite this differently.')}>
                      {i18n.tr('Generer på nytt', 'Regenerate')}
                    </button>
                  </div>
                </Show>
              </Show>
            </section>

            <section class="velion-inbox-aside-card">
              <div class="velion-inbox-card-heading velion-inbox-card-heading--between">
                <h2>{i18n.tr('Samtalesammendrag', 'Conversation summary')}</h2>
                <button type="button" disabled={summaryLoading()} onClick={() => void generateSummary()}>
                  {summaryLoading() ? i18n.tr('Oppsummerer …', 'Summarizing…') : i18n.tr('Oppsummer', 'Summarize')}
                </button>
              </div>
              <p class="velion-inbox-ai-text">
                {summaryLoading()
                  ? i18n.tr('Genererer sammendrag …', 'Generating summary…')
                  : summary() ?? i18n.tr('Oppsummer samtalen og finn kundens hensikt.', 'Summarize the conversation and extract the customer intent.')}
              </p>
            </section>

            <section class="velion-inbox-aside-card">
              <h2>{i18n.tr('Relevante kilder', 'Relevant sources')}</h2>
              <Show
                when={sources().length}
                fallback={<p class="velion-inbox-muted">{i18n.tr('Kilder vises her når Velion baserer et svar på kunnskapsbasen din.', 'Sources appear here when Velion grounds an answer in your knowledge base.')}</p>}
              >
                <div class="velion-inbox-source-stack">
                  <For each={sources()}>{(s) => <SourceRow title={s.title || s.uri || i18n.tr('Kilde', 'Source')} />}</For>
                </div>
              </Show>
            </section>

            <MacrosPanel
              orgId={props.orgId}
              onInsertReply={props.onInsertQuickReply}
              onMacroExecuted={props.onMacroExecuted}
              selectedTicket={props.selectedTicket}
            />
          </div>
        </Show>
      </div>

      <div class="velion-inbox-ask-velion">
        <div>
          <input
            value={question()}
            onInput={(event) => setQuestion(event.currentTarget.value)}
            onKeyDown={(event) => {
              if (event.key === 'Enter') void askVelion()
            }}
            placeholder={i18n.tr('Spør Velion om denne samtalen', 'Ask Velion about this conversation')}
            aria-label={i18n.tr('Spør Velion et spørsmål', 'Ask Velion a question')}
            disabled={!ready()}
          />
          <button type="button" disabled={answerLoading() || !ready()} onClick={() => void askVelion()} aria-label={i18n.tr('Send spørsmål til Velion', 'Send Velion question')}>
            <Send class={cn('size-3.5', answerLoading() && 'velion-inbox-spin')} />
          </button>
        </div>
      </div>
    </div>
  )
}

// Lightweight HTML→text for building the AI prompt from an article whose only
// body is HTML (the visible transcript uses the sandboxed EmailBody renderer).
function stripToText(value: string): string {
  return value
    .replace(/<style[\s\S]*?<\/style>/gi, ' ')
    .replace(/<[^>]*>/g, ' ')
    .replace(/&nbsp;/g, ' ')
    .replace(/\s+/g, ' ')
    .trim()
}

function CalendarPanel(props: { selectedTicket: ZammadTicket | null }) {
  const i18n = useI18n()
  const [events, setEvents] = createSignal<CalendarEvent[]>([])
  const [notes, setNotes] = createSignal<CalendarNote[]>([])
  const [selectedDate, setSelectedDate] = createSignal(new Date('2026-06-12T09:00:00.000Z'))
  const [eventTitle, setEventTitle] = createSignal('')
  const [noteText, setNoteText] = createSignal('')
  const selectedKey = () => formatDateKey(selectedDate())
  const selectedEvents = createMemo(() => events().filter((event) => formatDateKey(new Date(event.start)) === selectedKey()))
  const selectedNotes = createMemo(() => notes().filter((note) => note.date === selectedKey()))
  const upcomingEvents = createMemo(() => [...events()].sort((a, b) => new Date(a.start).getTime() - new Date(b.start).getTime()).slice(0, 4))

  const saveFollowUp = () => {
    const fallbackTitle = props.selectedTicket
      ? i18n.tr(`Oppfølging: ${props.selectedTicket.title}`, `Follow up: ${props.selectedTicket.title}`)
      : i18n.tr('Inbox-oppfølging', 'Inbox follow-up')
    const title = (eventTitle().trim() || fallbackTitle).slice(0, 160)
    const start = new Date(selectedDate())
    start.setHours(9, 0, 0, 0)
    const end = new Date(start)
    end.setMinutes(end.getMinutes() + 30)
    setEvents((current) => [{ id: `event_${Date.now()}`, title, start: start.toISOString(), end: end.toISOString(), type: 'inbox-follow-up', status: 'confirmed', createdAt: new Date().toISOString() }, ...current])
    setEventTitle('')
  }

  const saveNote = () => {
    const text = noteText().trim()
    if (!text) return
    setNotes((current) => [{ id: `note_${Date.now()}`, text: props.selectedTicket ? `#${props.selectedTicket.number}: ${text}` : text, date: selectedKey(), createdAt: new Date().toISOString() }, ...current])
    setNoteText('')
  }

  return (
    <div class="velion-inbox-aside-scroll velion-inbox-calendar-panel">
      <div class="velion-inbox-panel-title">
        <div>
          <h2>{i18n.tr('Innboks-kalender', 'Inbox calendar')}</h2>
          <p>{i18n.tr('Oppfølginger, notater og planlagt support-arbeid.', 'Follow-ups, notes, and scheduled support work.')}</p>
        </div>
        <CalendarDays class="size-5" />
      </div>

      <MiniCalendarGrid events={events()} selectedDate={selectedDate()} onSelect={setSelectedDate} />

      <section class="velion-inbox-aside-card">
        <div class="velion-inbox-card-heading velion-inbox-card-heading--between">
          <h3>{selectedDate().toLocaleDateString(localeDateTime(i18n.locale()), { weekday: 'short', month: 'short', day: 'numeric' })}</h3>
        </div>
        <div class="velion-inbox-calendar-day-list">
          <Show when={selectedEvents().length || selectedNotes().length} fallback={<p>{i18n.tr('Ingen hendelser denne dagen.', 'No events for this day.')}</p>}>
            <For each={selectedEvents()}>{(event) => <CalendarEventRow event={event} />}</For>
            <For each={selectedNotes()}>{(note) => <CalendarNoteRow note={note} />}</For>
          </Show>
        </div>
      </section>

      <section class="velion-inbox-aside-card velion-inbox-aside-card--muted">
        <h3>{i18n.tr('Planlegg oppfølging', 'Schedule follow-up')}</h3>
        <p>{i18n.tr('Opprett et kalenderelement fra den valgte saken.', 'Create a calendar item from the selected ticket.')}</p>
        <div class="velion-inbox-inline-entry">
          <Plus class="size-4" />
          <input
            value={eventTitle()}
            onInput={(event) => setEventTitle(event.currentTarget.value)}
            onKeyDown={(event) => { if (event.key === 'Enter') saveFollowUp() }}
            placeholder={props.selectedTicket ? i18n.tr(`Oppfølging: ${props.selectedTicket.title}`, `Follow up: ${props.selectedTicket.title}`) : i18n.tr('Tittel på oppfølging', 'Follow-up title')}
            aria-label={i18n.tr('Tittel på oppfølging', 'Follow-up title')}
          />
          <button type="button" onClick={saveFollowUp}>{i18n.tr('Legg til', 'Add')}</button>
        </div>
      </section>

      <section class="velion-inbox-aside-card">
        <h3>{i18n.tr('Kalendernotat', 'Calendar note')}</h3>
        <textarea
          rows={3}
          value={noteText()}
          onInput={(event) => setNoteText(event.currentTarget.value)}
          placeholder={i18n.tr('Legg til et privat oppfølgingsnotat …', 'Add a private follow-up note...')}
          aria-label={i18n.tr('Privat oppfølgingsnotat', 'Private follow-up note')}
        />
        <button type="button" disabled={!noteText().trim()} onClick={saveNote} class="velion-inbox-button velion-inbox-button--primary velion-inbox-button--xs">
          {i18n.tr('Lagre notat', 'Save note')}
        </button>
      </section>

      <section class="velion-inbox-aside-card">
        <h3>{i18n.tr('Kommende', 'Upcoming')}</h3>
        <Show when={upcomingEvents().length} fallback={<p>{i18n.tr('Ingen kommende kalenderhendelser.', 'No upcoming calendar events.')}</p>}>
          <For each={upcomingEvents()}>{(event) => <CalendarEventRow event={event} />}</For>
        </Show>
      </section>
    </div>
  )
}

function ActivityPanel(props: { selectedTicket: ZammadTicket | null }) {
  const i18n = useI18n()
  return (
    <Show
      when={props.selectedTicket}
      fallback={
        <EmptyAsideState
          icon={<Clock3 class="size-6" />}
          title={i18n.tr('Ingen aktivitet valgt', 'No activity selected')}
          body={i18n.tr(
            'Åpne en sak for å se arbeidsflythelse, samarbeidsstatus og oppfølgingsautomatisering.',
            'Open a ticket to see workflow health, collaboration state, and follow-up automation.',
          )}
        />
      }
    >
      {(ticket) => (
        <div class="velion-inbox-aside-scroll velion-inbox-activity-panel">
          <section class="velion-inbox-aside-card">
            <div class="velion-inbox-card-heading">
              <CheckCircle2 class="size-4 velion-inbox-success" />
              <h2>{i18n.tr('Arbeidsflythelse', 'Workflow health')}</h2>
            </div>
            <div class="velion-inbox-field-stack">
              <HealthRow label={i18n.tr('SLA-status', 'SLA status')} value={i18n.tr('På sporet', 'On track')} tone="success" />
              <HealthRow
                label={i18n.tr('Eierskap', 'Ownership')}
                value={ticket().owner ? `${ticket().owner?.firstname} ${ticket().owner?.lastname}` : i18n.tr('Trenger eier', 'Needs owner')}
                tone={ticket().owner ? 'neutral' : 'warning'}
              />
              <HealthRow label={i18n.tr('Kø', 'Queue')} value={ticket().group?.name ?? 'Support'} tone="neutral" />
            </div>
          </section>

          <section class="velion-inbox-aside-card">
            <div class="velion-inbox-card-heading">
              <UserRound class="size-4" />
              <h2>{i18n.tr('Teamsamarbeid', 'Team collaboration')}</h2>
            </div>
            <div class="velion-inbox-activity-stack">
              <ActivityItem
                title={i18n.tr('Ingen kollega skriver utkast', 'No teammate is drafting')}
                body={i18n.tr('Vis kollisjonsstatus her når en annen agent ser på eller svarer.', 'Show collision state here when another agent is viewing or replying.')}
              />
              <ActivityItem
                title={i18n.tr('Interne kommentarer', 'Internal comments')}
                body={i18n.tr('Legg til interne tråd-notater i Front-stil uten å endre kundesamtalen.', 'Add Front-style internal thread notes without changing the customer conversation.')}
              />
              <ActivityItem
                title={i18n.tr('Abonner kollega', 'Subscribe teammate')}
                body={i18n.tr('Varsle en kollega når kunden svarer eller SLA endres.', 'Notify a teammate when customer replies or SLA changes.')}
              />
            </div>
          </section>

          <section class="velion-inbox-aside-card velion-inbox-aside-card--soft">
            <div class="velion-inbox-card-heading">
              <AlertCircle class="size-4" />
              <h2>{i18n.tr('Automatiseringsregler', 'Automation hooks')}</h2>
            </div>
            <ActionSuggestion
              title={i18n.tr('Opprett delingsregel', 'Create split rule')}
              body={i18n.tr('Flytt denne avsenderen, domenet eller temaet til Fokusert eller Annet.', 'Move this sender, domain, or topic to Focused or Other.')}
            />
            <ActionSuggestion
              title={i18n.tr('Automatisk påminnelse', 'Auto reminder')}
              body={i18n.tr('Returner denne samtalen når oppfølgingsdatoen kommer.', 'Return this conversation when the follow-up date arrives.')}
            />
            <ActionSuggestion
              title={i18n.tr('SLA-eskalering', 'SLA escalation')}
              body={i18n.tr('Eskaler når ventetid eller prioritet overskrider policy.', 'Escalate when waiting time or priority exceeds policy.')}
            />
          </section>
        </div>
      )}
    </Show>
  )
}

// Extract canned reply text from a macro's generic `actions` JSON. Macros carry
// no dedicated reply field, so canned responses ride inside actions under one of
// several conventional keys (or as an array of {type,value} steps).
function macroReplyText(macro: TicketMacro): string {
  const a = macro.actions as Record<string, unknown> | unknown[] | undefined
  if (!a) return ''
  const pick = (obj: Record<string, unknown>): string => {
    for (const key of ['reply', 'reply_text', 'body_text', 'bodyText', 'body', 'text', 'message']) {
      const v = obj[key]
      if (typeof v === 'string' && v.trim()) return v
    }
    return ''
  }
  if (Array.isArray(a)) {
    for (const step of a) {
      if (step && typeof step === 'object') {
        const s = step as Record<string, unknown>
        const t = typeof s.value === 'string' ? s.value : pick(s)
        if (t.trim()) return t
      }
    }
    return ''
  }
  return pick(a)
}

function MacrosPanel(props: {
  orgId: string
  onInsertReply: (text: string) => void
  onMacroExecuted: () => void
  selectedTicket: ZammadTicket | null
}) {
  const i18n = useI18n()
  const [expanded, setExpanded] = createSignal(false)
  const [macrosRes] = createResource(
    () => (expanded() && props.orgId ? props.orgId : ''),
    (id) => (id ? listTicketMacros(id).catch(() => [] as TicketMacro[]) : Promise.resolve([] as TicketMacro[])),
  )
  const macros = () => (macrosRes() ?? []).filter((macro) => macro.active)

  const applyMacro = (macro: TicketMacro) => {
    if (!props.selectedTicket) return
    const text = macroReplyText(macro)
    if (text) props.onInsertReply(text)
    props.onMacroExecuted()
  }

  return (
    <section class="velion-inbox-macros">
      <button type="button" onClick={() => setExpanded((value) => !value)} aria-expanded={expanded()}>
        <span>{i18n.tr('Makroer', 'Macros')}</span>
        <ChevronDown class={cn('size-4', expanded() && 'rotate-180')} />
      </button>
      <Show when={expanded()}>
        <Show when={!macrosRes.loading} fallback={<p class="velion-inbox-macros__empty">{i18n.tr('Laster makroer …', 'Loading macros…')}</p>}>
          <ul>
            <For each={macros()} fallback={<li class="velion-inbox-macros__empty">{i18n.tr('Ingen makroer konfigurert. Opprett dem under Innstillinger → Makroer.', 'No macros configured. Create them in Settings → Macros.')}</li>}>
              {(macro) => (
                <li>
                  <span title={macro.description}>{macro.name}</span>
                  <button type="button" disabled={!props.selectedTicket} onClick={() => applyMacro(macro)}>
                    <Play class="size-3" />
                    {i18n.tr('Bruk', 'Apply')}
                  </button>
                </li>
              )}
            </For>
          </ul>
        </Show>
      </Show>
    </section>
  )
}

function ActionSuggestion(props: { actionLabel?: string; body: string; onRun?: () => void; title: string }) {
  return (
    <div class="velion-inbox-action-suggestion">
      <div>
        <strong>{props.title}</strong>
        <p>{props.body}</p>
      </div>
      <Show when={props.actionLabel}>
        <button type="button" onClick={() => props.onRun?.()}>{props.actionLabel}</button>
      </Show>
    </div>
  )
}
