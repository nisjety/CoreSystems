import { For, Show, createSignal } from 'solid-js'
import { ChevronDown, Plus } from '@/shared/icons'
import type { JSX } from '@solidjs/web'
import {
  formatDateKey,
  type CalendarEvent,
  type CalendarNote,
} from '@/features/inbox/lib/inbox-model'
import type { InboxModalRequest } from '@/features/inbox/components/InboxWorkModal'
import { cn } from '@/shared/lib/cn'
import { localeDateTime, useI18n } from '@/shared/i18n'
import { handleTabKeyDown } from '@/shared/ui/tab-keyboard'

export function MiniCalendarGrid(props: {
  events: CalendarEvent[]
  onSelect: (date: Date) => void
  selectedDate: Date
}) {
  const i18n = useI18n()
  const days = () => buildCalendarDays(props.selectedDate)
  const eventDates = () => new Set(props.events.map((event) => formatDateKey(new Date(event.start))))
  const todayKey = () => formatDateKey(new Date())

  return (
    <section class="verevon-inbox-mini-calendar">
      <div class="verevon-inbox-mini-calendar__header">
        <h3>{props.selectedDate.toLocaleDateString(localeDateTime(i18n.locale()), { month: 'long', year: 'numeric' })}</h3>
        <div class="verevon-inbox-mini-calendar__actions">
          <button type="button" onClick={() => props.onSelect(shiftDate(props.selectedDate, -7))} aria-label={i18n.tr('Forrige uke', 'Previous week')}>
            <ChevronDown class="size-4 rotate-90" />
          </button>
          <button type="button" onClick={() => props.onSelect(new Date())}>{i18n.tr('I dag', 'Today')}</button>
          <button type="button" onClick={() => props.onSelect(shiftDate(props.selectedDate, 7))} aria-label={i18n.tr('Neste uke', 'Next week')}>
            <ChevronDown class="size-4 -rotate-90" />
          </button>
        </div>
      </div>
      <div class="verevon-inbox-mini-calendar__grid">
        <For each={days()}>
          {(day) => {
            const key = () => formatDateKey(day)
            const selected = () => key() === formatDateKey(props.selectedDate)
            const isToday = () => key() === todayKey()

            return (
              <button
                type="button"
                onClick={() => props.onSelect(day)}
                class={{ 'verevon-inbox-mini-calendar__day--selected': selected() }}
              >
                <span>{day.toLocaleDateString(localeDateTime(i18n.locale()), { weekday: 'short' }).slice(0, 1)}</span>
                <strong class={{ 'verevon-inbox-mini-calendar__today': isToday() && !selected() }}>{day.getDate()}</strong>
                <em class={{ 'verevon-inbox-mini-calendar__event-dot': eventDates().has(key()) }} />
              </button>
            )
          }}
        </For>
      </div>
    </section>
  )
}

export function CalendarEventRow(props: { event: CalendarEvent }) {
  const i18n = useI18n()
  const start = () => new Date(props.event.start)
  const end = () => new Date(props.event.end)

  return (
    <div class="verevon-inbox-calendar-row">
      <span class="verevon-inbox-calendar-row__dot verevon-inbox-calendar-row__dot--event" />
      <div>
        <p>{props.event.title}</p>
        <small>
          {start().toLocaleTimeString(localeDateTime(i18n.locale()), { hour: 'numeric', minute: '2-digit' })} - {end().toLocaleTimeString(localeDateTime(i18n.locale()), { hour: 'numeric', minute: '2-digit' })}
        </small>
      </div>
    </div>
  )
}

export function CalendarNoteRow(props: { note: CalendarNote }) {
  const i18n = useI18n()
  return (
    <div class="verevon-inbox-calendar-row verevon-inbox-calendar-row--note">
      <span class="verevon-inbox-calendar-row__dot verevon-inbox-calendar-row__dot--note" />
      <div>
        <p>{props.note.text}</p>
        <small>{new Date(props.note.createdAt).toLocaleTimeString(localeDateTime(i18n.locale()), { hour: 'numeric', minute: '2-digit' })}</small>
      </div>
    </div>
  )
}

export function HealthRow(props: { label: string; tone: 'neutral' | 'success' | 'warning'; value: string }) {
  return (
    <div class="verevon-inbox-health-row">
      <span>{props.label}</span>
      <strong class={`verevon-inbox-health-row__value verevon-inbox-health-row__value--${props.tone}`}>
        {props.value}
      </strong>
    </div>
  )
}

export function ActivityItem(props: { body: string; title: string }) {
  return (
    <div class="verevon-inbox-activity-item">
      <span />
      <div>
        <p>{props.title}</p>
        <small>{props.body}</small>
      </div>
    </div>
  )
}

export function AsideTabButton(props: {
  active: boolean
  children: JSX.Element
  controls: string
  id: string
  onClick: () => void
}) {
  return (
    <button
      type="button"
      role="tab"
      id={props.id}
      aria-controls={props.controls}
      aria-selected={props.active ? 'true' : 'false'}
      tabindex={props.active ? 0 : -1}
      onKeyDown={handleTabKeyDown}
      onClick={() => props.onClick()}
      class={cn('verevon-inbox-aside-tab', props.active && 'verevon-inbox-aside-tab--active')}
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
  const i18n = useI18n()
  const [open, setOpen] = createSignal(Boolean(props.defaultOpen))

  return (
    <section class="verevon-inbox-accordion">
      <button type="button" onClick={() => setOpen((value) => !value)} aria-expanded={open() ? 'true' : 'false'}>
        {props.icon}
        <span>{props.title}</span>
        <ChevronDown class={cn('size-4 verevon-inbox-accordion__chevron', open() && 'rotate-180')} />
      </button>
      {open() ? <div class="verevon-inbox-accordion__body">{props.children ?? <p>{i18n.tr('Ingen data ennå.', 'No data yet.')}</p>}</div> : null}
    </section>
  )
}

export function LinkRow(props: { label: string; onOpenModal: (modal: InboxModalRequest) => void }) {
  const i18n = useI18n()
  return (
    <div class="verevon-inbox-link-row">
      <span>{props.label}</span>
      <button
        type="button"
        onClick={() => props.onOpenModal({
          type: 'work',
          title: props.label,
          description: i18n.tr(
            `Opprett eller knytt ${props.label.toLowerCase()} til denne saken uten å forlate innboksen.`,
            `Create or attach ${props.label.toLowerCase()} from this ticket without navigating away from the inbox.`,
          ),
          primaryAction: i18n.tr('Fest lenke', 'Attach link'),
        })}
        aria-label={i18n.tr(`Legg til ${props.label}`, `Add ${props.label}`)}
      >
        <Plus class="size-4" />
      </button>
    </div>
  )
}

export function FieldRow(props: { label: string; muted?: boolean; value: string }) {
  return (
    <div class="verevon-inbox-field-row">
      <span>{props.label}</span>
      <strong class={{ 'verevon-inbox-field-row__muted': Boolean(props.muted) }}>{props.value}</strong>
    </div>
  )
}

function safeSourceUrl(value: string | undefined): string | null {
  if (!value) return null
  try {
    const url = new URL(value)
    return url.protocol === 'https:' || url.protocol === 'http:' ? url.toString() : null
  } catch {
    return null
  }
}

export function SourceRow(props: { title: string; uri?: string; excerpt?: string }) {
  const href = () => safeSourceUrl(props.uri)
  return (
    <div class="verevon-inbox-source-row">
      <span>i</span>
      <div class="verevon-inbox-source-row__content">
        <Show when={href()} fallback={<strong>{props.title}</strong>}>
          <a href={href() ?? undefined} target="_blank" rel="noreferrer noopener">{props.title}</a>
        </Show>
        <Show when={props.excerpt?.trim()}>
          <small>{props.excerpt}</small>
        </Show>
      </div>
    </div>
  )
}

export function EmptyAsideState(props: { body: string; icon: JSX.Element; title: string }) {
  return (
    <div class="verevon-inbox-empty-aside">
      <div>
        <div class="verevon-inbox-empty-aside__icon">{props.icon}</div>
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
