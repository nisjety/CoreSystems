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
  const [activeTab, setActiveTab] = createSignal<AsideTab>('details')

  return (
    <aside aria-label="AI and customer context" class="velion-inbox-aside">
      <div class="velion-inbox-aside__header">
        <div class="velion-inbox-aside__tabs">
          <For each={[
            { id: 'details' as const, label: 'Details' },
            { id: 'velion' as const, label: 'Velion' },
            { id: 'calendar' as const, label: 'Calendar' },
            { id: 'activity' as const, label: 'Activity' },
          ]}>
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
  return (
    <div class="velion-inbox-aside-scroll">
      <Show
        when={props.selectedTicket}
        fallback={
          <EmptyAsideState
            icon={<UserRound class="size-6" />}
            title="No customer selected"
            body="Open a ticket to see customer details, tags, and recent conversations."
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
                <FieldRow label="Channel" value={titleCase(ticket().channel ?? 'email')} />
                <FieldRow label="Status" value={titleCase(ticket().state?.name ?? 'open')} />
                <FieldRow label="Priority" value={titleCase(ticket().priority?.name ?? 'normal')} />
                <FieldRow label="Assignee" value={ticket().owner ? `${ticket().owner?.firstname} ${ticket().owner?.lastname}`.trim() : 'Unassigned'} />
                <FieldRow label="Team inbox" value={ticket().group?.name ?? 'Support'} />
              </div>
            </section>

            <AccordionSection defaultOpen icon={<FileText class="size-4" />} title="Conversation">
              <FieldRow label="Reference" value={ticket().number} />
              <FieldRow label="Subject" value={ticket().title} />
              <Show when={ticket().customer?.email}>
                <FieldRow label="Email" value={ticket().customer!.email} />
              </Show>
              <FieldRow label="Opened" value={formatRelativeTime(ticket().created_at)} />
            </AccordionSection>

            <AccordionSection defaultOpen icon={<Sparkles class="size-4" />} title="Tags">
              <Show
                when={(ticket().tags ?? []).length}
                fallback={<p class="velion-inbox-muted">No tags on this conversation yet.</p>}
              >
                <div class="velion-inbox-detail-tags">
                  <For each={ticket().tags ?? []}>{(tag) => <span class="velion-inbox-detail-tag">{tag}</span>}</For>
                </div>
              </Show>
            </AccordionSection>

            <AccordionSection icon={<MessageCircle class="size-4" />} title={`Recent conversations${(props.recent?.length ?? 0) ? ` (${props.recent!.length})` : ''}`}>
              <Show
                when={(props.recent?.length ?? 0) > 0}
                fallback={<p class="velion-inbox-muted">No other conversations from this contact.</p>}
              >
                <ul class="velion-inbox-recent-list">
                  <For each={props.recent ?? []}>
                    {(item) => (
                      <li>
                        <button type="button" onClick={() => props.onSelectRecent(item.conversationId)}>
                          <span class="velion-inbox-recent-list__title">{item.title || '(no subject)'}</span>
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
      setDraft(res.text || 'Velion returned an empty reply.')
      applySources(res.sources)
    } catch {
      setError('Velion could not generate a reply. Try again.')
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
      setSummary(res.text || 'No summary available.')
    } catch {
      setError('Velion could not summarize. Try again.')
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
      setError('Velion action failed. Try again.')
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
      setAnswer(res.text || 'Velion had no answer.')
      applySources(res.sources)
    } catch {
      setError('Velion could not answer. Try again.')
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
              title="Select a ticket"
              body="Velion can draft replies, summarize context, and surface relevant sources once a conversation is open."
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
                <h2>Velion action plan</h2>
              </div>
              <ActionSuggestion
                title="Confirm intent"
                body="Detect the customer's primary intent and the best next action."
                actionLabel={runningCard() === 'intent' ? 'Running…' : 'Run'}
                onRun={() => void runCard('intent', 'intent')}
              />
              <ActionSuggestion
                title="Source-backed reply"
                body="Draft a reply grounded only in facts supported by the conversation."
                actionLabel={runningCard() === 'source' ? 'Running…' : 'Run'}
                onRun={() =>
                  void runCard(
                    'source',
                    'draft',
                    'Only assert facts supported by the transcript; do not invent policy or promises.',
                  )
                }
              />
              <ActionSuggestion
                title="Assess & route"
                body="Recommend whether to escalate or route, based on urgency and status."
                actionLabel={runningCard() === 'route' ? 'Running…' : 'Run'}
                onRun={() =>
                  void runCard(
                    'route',
                    'ask',
                    undefined,
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
                  <button type="button" onClick={() => setAnswer(null)} aria-label="Dismiss answer">
                    Clear
                  </button>
                </div>
                <p class="velion-inbox-ai-text">{answer()}</p>
              </section>
            </Show>

            <section class="velion-inbox-aside-card velion-inbox-aside-card--soft">
              <div class="velion-inbox-card-heading velion-inbox-card-heading--between">
                <div>
                  <Sparkles class="size-4" />
                  <h2>Reply assistance</h2>
                </div>
                <button type="button" disabled={draftLoading()} onClick={() => void generateDraft()}>
                  <RefreshCw class={cn('size-3.5', draftLoading() && 'velion-inbox-spin')} />
                  {draftLoading() ? 'Drafting…' : 'Generate'}
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
                <Show when={draft()} fallback={<p>Generate a suggested reply grounded in this conversation.</p>}>
                  <p class="velion-inbox-ai-text">{draft()}</p>
                  <div class="velion-inbox-draft-actions">
                    <button type="button" class="velion-inbox-btn-primary" onClick={() => props.onInsertQuickReply(draft() ?? '')}>
                      Insert into reply
                    </button>
                    <button type="button" onClick={() => void generateDraft('Rewrite this differently.')}>
                      Regenerate
                    </button>
                  </div>
                </Show>
              </Show>
            </section>

            <section class="velion-inbox-aside-card">
              <div class="velion-inbox-card-heading velion-inbox-card-heading--between">
                <h2>Conversation summary</h2>
                <button type="button" disabled={summaryLoading()} onClick={() => void generateSummary()}>
                  {summaryLoading() ? 'Summarizing…' : 'Summarize'}
                </button>
              </div>
              <p class="velion-inbox-ai-text">
                {summaryLoading()
                  ? 'Generating summary…'
                  : summary() ?? 'Summarize the conversation and extract the customer intent.'}
              </p>
            </section>

            <section class="velion-inbox-aside-card">
              <h2>Relevant sources</h2>
              <Show
                when={sources().length}
                fallback={<p class="velion-inbox-muted">Sources appear here when Velion grounds an answer in your knowledge base.</p>}
              >
                <div class="velion-inbox-source-stack">
                  <For each={sources()}>{(s) => <SourceRow title={s.title || s.uri || 'Source'} />}</For>
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
            placeholder="Ask Velion about this conversation"
            aria-label="Ask Velion a question"
            disabled={!ready()}
          />
          <button type="button" disabled={answerLoading() || !ready()} onClick={() => void askVelion()} aria-label="Send Velion question">
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
    const title = (eventTitle().trim() || (props.selectedTicket ? `Follow up: ${props.selectedTicket.title}` : 'Inbox follow-up')).slice(0, 160)
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
          <h2>Inbox calendar</h2>
          <p>Follow-ups, notes, and scheduled support work.</p>
        </div>
        <CalendarDays class="size-5" />
      </div>

      <MiniCalendarGrid events={events()} selectedDate={selectedDate()} onSelect={setSelectedDate} />

      <section class="velion-inbox-aside-card">
        <div class="velion-inbox-card-heading velion-inbox-card-heading--between">
          <h3>{selectedDate().toLocaleDateString('en-US', { weekday: 'short', month: 'short', day: 'numeric' })}</h3>
        </div>
        <div class="velion-inbox-calendar-day-list">
          <Show when={selectedEvents().length || selectedNotes().length} fallback={<p>No events for this day.</p>}>
            <For each={selectedEvents()}>{(event) => <CalendarEventRow event={event} />}</For>
            <For each={selectedNotes()}>{(note) => <CalendarNoteRow note={note} />}</For>
          </Show>
        </div>
      </section>

      <section class="velion-inbox-aside-card velion-inbox-aside-card--muted">
        <h3>Schedule follow-up</h3>
        <p>Create a calendar item from the selected ticket.</p>
        <div class="velion-inbox-inline-entry">
          <Plus class="size-4" />
          <input
            value={eventTitle()}
            onInput={(event) => setEventTitle(event.currentTarget.value)}
            onKeyDown={(event) => { if (event.key === 'Enter') saveFollowUp() }}
            placeholder={props.selectedTicket ? `Follow up: ${props.selectedTicket.title}` : 'Follow-up title'}
            aria-label="Follow-up title"
          />
          <button type="button" onClick={saveFollowUp}>Add</button>
        </div>
      </section>

      <section class="velion-inbox-aside-card">
        <h3>Calendar note</h3>
        <textarea
          rows={3}
          value={noteText()}
          onInput={(event) => setNoteText(event.currentTarget.value)}
          placeholder="Add a private follow-up note..."
          aria-label="Private follow-up note"
        />
        <button type="button" disabled={!noteText().trim()} onClick={saveNote} class="velion-inbox-button velion-inbox-button--primary velion-inbox-button--xs">
          Save note
        </button>
      </section>

      <section class="velion-inbox-aside-card">
        <h3>Upcoming</h3>
        <Show when={upcomingEvents().length} fallback={<p>No upcoming calendar events.</p>}>
          <For each={upcomingEvents()}>{(event) => <CalendarEventRow event={event} />}</For>
        </Show>
      </section>
    </div>
  )
}

function ActivityPanel(props: { selectedTicket: ZammadTicket | null }) {
  return (
    <Show
      when={props.selectedTicket}
      fallback={
        <EmptyAsideState
          icon={<Clock3 class="size-6" />}
          title="No activity selected"
          body="Open a ticket to see workflow health, collaboration state, and follow-up automation."
        />
      }
    >
      {(ticket) => (
        <div class="velion-inbox-aside-scroll velion-inbox-activity-panel">
          <section class="velion-inbox-aside-card">
            <div class="velion-inbox-card-heading">
              <CheckCircle2 class="size-4 velion-inbox-success" />
              <h2>Workflow health</h2>
            </div>
            <div class="velion-inbox-field-stack">
              <HealthRow label="SLA status" value="On track" tone="success" />
              <HealthRow label="Ownership" value={ticket().owner ? `${ticket().owner?.firstname} ${ticket().owner?.lastname}` : 'Needs owner'} tone={ticket().owner ? 'neutral' : 'warning'} />
              <HealthRow label="Queue" value={ticket().group?.name ?? 'Support'} tone="neutral" />
            </div>
          </section>

          <section class="velion-inbox-aside-card">
            <div class="velion-inbox-card-heading">
              <UserRound class="size-4" />
              <h2>Team collaboration</h2>
            </div>
            <div class="velion-inbox-activity-stack">
              <ActivityItem title="No teammate is drafting" body="Show collision state here when another agent is viewing or replying." />
              <ActivityItem title="Internal comments" body="Add Front-style internal thread notes without changing the customer conversation." />
              <ActivityItem title="Subscribe teammate" body="Notify a teammate when customer replies or SLA changes." />
            </div>
          </section>

          <section class="velion-inbox-aside-card velion-inbox-aside-card--soft">
            <div class="velion-inbox-card-heading">
              <AlertCircle class="size-4" />
              <h2>Automation hooks</h2>
            </div>
            <ActionSuggestion title="Create split rule" body="Move this sender, domain, or topic to Focused or Other." />
            <ActionSuggestion title="Auto reminder" body="Return this conversation when the follow-up date arrives." />
            <ActionSuggestion title="SLA escalation" body="Escalate when waiting time or priority exceeds policy." />
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
        <span>Macros</span>
        <ChevronDown class={cn('size-4', expanded() && 'rotate-180')} />
      </button>
      <Show when={expanded()}>
        <Show when={!macrosRes.loading} fallback={<p class="velion-inbox-macros__empty">Loading macros…</p>}>
          <ul>
            <For each={macros()} fallback={<li class="velion-inbox-macros__empty">No macros configured. Create them in Settings → Macros.</li>}>
              {(macro) => (
                <li>
                  <span title={macro.description}>{macro.name}</span>
                  <button type="button" disabled={!props.selectedTicket} onClick={() => applyMacro(macro)}>
                    <Play class="size-3" />
                    Apply
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
