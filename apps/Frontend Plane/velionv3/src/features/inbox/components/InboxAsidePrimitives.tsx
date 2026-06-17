import { ChevronDown, Plus } from 'lucide-solid'
import { createSignal, For, type JSX } from 'solid-js'
import {
  formatDateKey,
  type CalendarEvent,
  type CalendarNote,
} from '@/features/inbox/lib/inbox-model'
import type { InboxModalRequest } from '@/features/inbox/components/InboxWorkModal'
import { cn } from '@/shared/lib/cn'

export function MiniCalendarGrid(props: {
  events: CalendarEvent[]
  onSelect: (date: Date) => void
  selectedDate: Date
}) {
  const days = () => buildCalendarDays(props.selectedDate)
  const eventDates = () => new Set(props.events.map((event) => formatDateKey(new Date(event.start))))
  const todayKey = () => formatDateKey(new Date())

  return (
    <section class="velion-inbox-mini-calendar">
      <div class="velion-inbox-mini-calendar__header">
        <h3>{props.selectedDate.toLocaleDateString('en-US', { month: 'long', year: 'numeric' })}</h3>
        <div class="velion-inbox-mini-calendar__actions">
          <button type="button" onClick={() => props.onSelect(shiftDate(props.selectedDate, -7))} aria-label="Previous week">
            <ChevronDown class="size-4 rotate-90" />
          </button>
          <button type="button" onClick={() => props.onSelect(new Date())}>Today</button>
          <button type="button" onClick={() => props.onSelect(shiftDate(props.selectedDate, 7))} aria-label="Next week">
            <ChevronDown class="size-4 -rotate-90" />
          </button>
        </div>
      </div>
      <div class="velion-inbox-mini-calendar__grid">
        <For each={days()}>
          {(day) => {
            const key = () => formatDateKey(day)
            const selected = () => key() === formatDateKey(props.selectedDate)
            const isToday = () => key() === todayKey()

            return (
              <button
                type="button"
                onClick={() => props.onSelect(day)}
                classList={{ 'velion-inbox-mini-calendar__day--selected': selected() }}
              >
                <span>{day.toLocaleDateString('en-US', { weekday: 'short' }).slice(0, 1)}</span>
                <strong classList={{ 'velion-inbox-mini-calendar__today': isToday() && !selected() }}>{day.getDate()}</strong>
                <em classList={{ 'velion-inbox-mini-calendar__event-dot': eventDates().has(key()) }} />
              </button>
            )
          }}
        </For>
      </div>
    </section>
  )
}

export function CalendarEventRow(props: { event: CalendarEvent }) {
  const start = () => new Date(props.event.start)
  const end = () => new Date(props.event.end)

  return (
    <div class="velion-inbox-calendar-row">
      <span class="velion-inbox-calendar-row__dot velion-inbox-calendar-row__dot--event" />
      <div>
        <p>{props.event.title}</p>
        <small>
          {start().toLocaleTimeString('en-US', { hour: 'numeric', minute: '2-digit' })} - {end().toLocaleTimeString('en-US', { hour: 'numeric', minute: '2-digit' })}
        </small>
      </div>
    </div>
  )
}

export function CalendarNoteRow(props: { note: CalendarNote }) {
  return (
    <div class="velion-inbox-calendar-row velion-inbox-calendar-row--note">
      <span class="velion-inbox-calendar-row__dot velion-inbox-calendar-row__dot--note" />
      <div>
        <p>{props.note.text}</p>
        <small>{new Date(props.note.createdAt).toLocaleTimeString('en-US', { hour: 'numeric', minute: '2-digit' })}</small>
      </div>
    </div>
  )
}

export function HealthRow(props: { label: string; tone: 'neutral' | 'success' | 'warning'; value: string }) {
  return (
    <div class="velion-inbox-health-row">
      <span>{props.label}</span>
      <strong class={`velion-inbox-health-row__value velion-inbox-health-row__value--${props.tone}`}>
        {props.value}
      </strong>
    </div>
  )
}

export function ActivityItem(props: { body: string; title: string }) {
  return (
    <div class="velion-inbox-activity-item">
      <span />
      <div>
        <p>{props.title}</p>
        <small>{props.body}</small>
      </div>
    </div>
  )
}

export function AsideTabButton(props: { active: boolean; children: JSX.Element; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={() => props.onClick()}
      class={cn('velion-inbox-aside-tab', props.active && 'velion-inbox-aside-tab--active')}
    >
      {props.children}
    </button>
  )
}

export function AccordionSection(props: {
  children?: JSX.Element
  defaultOpen?: boolean
  icon: JSX.Element
  title: string
}) {
  const [open, setOpen] = createSignal(Boolean(props.defaultOpen))

  return (
    <section class="velion-inbox-accordion">
      <button type="button" onClick={() => setOpen((value) => !value)} aria-expanded={open()}>
        {props.icon}
        <span>{props.title}</span>
        <ChevronDown class={cn('size-4 velion-inbox-accordion__chevron', open() && 'rotate-180')} />
      </button>
      {open() ? <div class="velion-inbox-accordion__body">{props.children ?? <p>No data yet.</p>}</div> : null}
    </section>
  )
}

export function LinkRow(props: { label: string; onOpenModal: (modal: InboxModalRequest) => void }) {
  return (
    <div class="velion-inbox-link-row">
      <span>{props.label}</span>
      <button
        type="button"
        onClick={() => props.onOpenModal({
          type: 'work',
          title: props.label,
          description: `Create or attach ${props.label.toLowerCase()} from this ticket without navigating away from the inbox.`,
          primaryAction: 'Attach link',
        })}
        aria-label={`Add ${props.label}`}
      >
        <Plus class="size-4" />
      </button>
    </div>
  )
}

export function FieldRow(props: { label: string; muted?: boolean; value: string }) {
  return (
    <div class="velion-inbox-field-row">
      <span>{props.label}</span>
      <strong classList={{ 'velion-inbox-field-row__muted': props.muted }}>{props.value}</strong>
    </div>
  )
}

export function SourceRow(props: { title: string }) {
  return (
    <div class="velion-inbox-source-row">
      <span>i</span>
      <strong>{props.title}</strong>
    </div>
  )
}

export function EmptyAsideState(props: { body: string; icon: JSX.Element; title: string }) {
  return (
    <div class="velion-inbox-empty-aside">
      <div>
        <div class="velion-inbox-empty-aside__icon">{props.icon}</div>
        <h2>{props.title}</h2>
        <p>{props.body}</p>
      </div>
    </div>
  )
}

function buildCalendarDays(anchor: Date) {
  const start = new Date(anchor)
  start.setDate(anchor.getDate() - anchor.getDay())
  return Array.from({ length: 7 }, (_, index) => shiftDate(start, index))
}

function shiftDate(date: Date, days: number) {
  const next = new Date(date)
  next.setDate(next.getDate() + days)
  return next
}
