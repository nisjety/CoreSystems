/**
 * Lanes, sentences and salience for the attention inbox (WS11-S7).
 *
 * Presentation only. Nothing here decides who may see a row: the gateway already dropped every
 * row of a room the viewer left, and a title arrives only when Session Core released it. This
 * module turns a row's closed kind word into one honest sentence, groups rows into lanes and
 * orders them with the same salience words the Space activity feed uses (`ActivitySalience`,
 * reused unchanged): failures rise, reads recede.
 */
import type { ActivitySalience } from '@/features/spaces/lib/activity-grammar'
import { spaceThreadHref } from '@/features/spaces/lib/space-thread-deep-link'
import type {
  AttentionItem,
  AttentionItemKind,
  AttentionNeedsYou,
} from '@/shared/api/spaces-attention-items-client'

type Tr = (no: string, en: string) => string

/** The inbox lanes, in the order they are drawn. */
export type AttentionLane = 'needs_you' | 'mentions' | 'threads' | 'agents' | 'invites' | 'reminders'

export const ATTENTION_LANES: readonly AttentionLane[] = [
  'needs_you',
  'mentions',
  'threads',
  'agents',
  'invites',
  'reminders',
]

const LANE_OF_KIND: Readonly<Record<AttentionItemKind, Exclude<AttentionLane, 'needs_you'>>> = {
  mention: 'mentions',
  reply: 'threads',
  agent_done: 'agents',
  agent_failed: 'agents',
  invite: 'invites',
  reminder: 'reminders',
}

export function laneOfKind(kind: AttentionItemKind): Exclude<AttentionLane, 'needs_you'> {
  return LANE_OF_KIND[kind]
}

export function laneLabel(lane: AttentionLane, tr: Tr): string {
  switch (lane) {
    case 'needs_you':
      return tr('Trenger deg', 'Needs you')
    case 'mentions':
      return tr('Omtaler', 'Mentions')
    case 'threads':
      return tr('Tråder', 'Threads')
    case 'agents':
      return tr('Agenter', 'Agents')
    case 'invites':
      return tr('Invitasjoner', 'Invites')
    case 'reminders':
      return tr('Påminnelser', 'Reminders')
  }
}

/** How loudly a row speaks. A failed run outranks a mention; a finished run recedes. */
export function itemSalience(kind: AttentionItemKind): ActivitySalience {
  switch (kind) {
    case 'agent_failed':
      return 'critical'
    case 'mention':
    case 'invite':
    case 'reminder':
      return 'attention'
    case 'reply':
      return 'normal'
    case 'agent_done':
      return 'muted'
  }
}

const SALIENCE_RANK: Readonly<Record<ActivitySalience, number>> = {
  critical: 0,
  attention: 1,
  normal: 2,
  muted: 3,
}

/** Within a lane: salience first, then the most recently updated. Does not mutate its input. */
export function orderItems(items: readonly AttentionItem[]): AttentionItem[] {
  return [...items].sort(
    (a, b) =>
      SALIENCE_RANK[itemSalience(a.kind)] - SALIENCE_RANK[itemSalience(b.kind)] || b.updatedAt - a.updatedAt,
  )
}

/** Open rows by lane. `needs_you` is live-derived and held apart, so it is never in this map. */
export function groupByLane(
  items: readonly AttentionItem[],
): Readonly<Record<Exclude<AttentionLane, 'needs_you'>, AttentionItem[]>> {
  const lanes: Record<Exclude<AttentionLane, 'needs_you'>, AttentionItem[]> = {
    mentions: [],
    threads: [],
    agents: [],
    invites: [],
    reminders: [],
  }
  for (const item of orderItems(items)) lanes[laneOfKind(item.kind)].push(item)
  return lanes
}

/** Who a row names: the roster's name when the room's roster gave one, else the honest fallback words. */
function who(item: AttentionItem, tr: Tr, name?: string): string {
  if (name) return name
  if (item.actor.kind === 'agent') return tr('En agent', 'An agent')
  if (item.actor.kind === 'user') return tr('Noen', 'Someone')
  return tr('Verevon', 'Verevon')
}

/** The room a row belongs to, never a raw ref when a name is known. */
export function roomName(item: { spaceName: string | null }, tr: Tr): string {
  return item.spaceName ?? tr('et rom', 'a room')
}

/**
 * One sentence per row. The title, when released, is shown beside it and never inside it. `actorName`
 * is the actor's name from the room's roster (see attention-roster.ts), when the roster gave one.
 */
export function itemSentence(item: AttentionItem, tr: Tr, actorName?: string): string {
  const room = roomName(item, tr)
  const more = item.count > 1 ? ` (${item.count})` : ''
  switch (item.kind) {
    case 'mention': {
      const name = who(item, tr, actorName)
      return tr(`${name} nevnte deg i ${room}${more}`, `${name} mentioned you in ${room}${more}`)
    }
    case 'reply':
      return tr(`Nye svar i en tråd du følger i ${room}${more}`, `New replies in a thread you follow in ${room}${more}`)
    case 'invite':
      return tr(`Du er invitert til ${room}`, `You were invited to ${room}`)
    case 'agent_done':
      return tr(`Agenten din er ferdig i ${room}${more}`, `Your agent finished in ${room}${more}`)
    case 'agent_failed':
      return tr(`Agenten din feilet i ${room}${more}`, `Your agent failed in ${room}${more}`)
    case 'reminder':
      return tr(`Påminnelse i ${room}${more}`, `Reminder in ${room}${more}`)
  }
}

export function needsYouSentence(entry: AttentionNeedsYou, tr: Tr): string {
  return tr(
    `En agent venter på godkjenning i ${roomName(entry, tr)}`,
    `An agent is waiting for your approval in ${roomName(entry, tr)}`,
  )
}

/** Where a row leads: the post when it names one, otherwise the room. */
export function itemHref(item: { spaceRef: string; threadId: string | null }): string {
  return spaceThreadHref(item.spaceRef, item.threadId)
}

/** The title line, or the honest reason there is none. `null` means draw nothing. */
export function headlineText(
  entry: { headline: string | null; headlineState: 'ok' | 'hidden' | 'restricted'; threadId: string | null },
  tr: Tr,
): string | null {
  if (entry.headline) return entry.headline
  if (entry.threadId === null) return null
  if (entry.headlineState === 'hidden') return tr('Forhåndsvisning er skjult', 'Preview hidden')
  if (entry.headlineState === 'restricted') return tr('Ikke tilgjengelig for deg', 'Not available to you')
  return null
}

/** Snooze presets, in milliseconds from now. All inside the server's one minute to 30 day window. */
export const SNOOZE_PRESETS: readonly { readonly id: string; readonly ms: number }[] = [
  { id: 'hour', ms: 60 * 60 * 1000 },
  { id: 'tomorrow', ms: 24 * 60 * 60 * 1000 },
  { id: 'week', ms: 7 * 24 * 60 * 60 * 1000 },
]

export function snoozeLabel(id: string, tr: Tr): string {
  switch (id) {
    case 'hour':
      return tr('1 time', '1 hour')
    case 'tomorrow':
      return tr('I morgen', 'Tomorrow')
    default:
      return tr('1 uke', '1 week')
  }
}

/** The window the server accepts for a snooze: at least a minute and at most 30 days ahead (spaceAttentionRules.ts). */
export const MIN_SNOOZE_MS = 60 * 1000
export const MAX_SNOOZE_MS = 30 * 24 * 60 * 60 * 1000
const MINUTE_MS = 60 * 1000
/** The time a custom snooze starts from when the viewer opens the picker: an hour ahead, like the first preset. */
const CUSTOM_SNOOZE_DEFAULT_MS = 60 * 60 * 1000

export type CustomSnoozeProblem = 'invalid' | 'too_soon' | 'too_late'
export type CustomSnooze = { readonly ok: true; readonly snoozeUntil: number } | { readonly ok: false; readonly problem: CustomSnoozeProblem }

/**
 * A `datetime-local` value (local time, to the minute, `YYYY-MM-DDTHH:mm`) as a snooze time, or the
 * reason it cannot be used. The window is checked here so the viewer sees the reason, not a refusal.
 */
export function customSnooze(value: string, now: number): CustomSnooze {
  const match = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})$/.exec(value.trim())
  if (!match) return { ok: false, problem: 'invalid' }
  const [year, month, day, hour, minute] = match.slice(1).map(Number) as [number, number, number, number, number]
  const at = new Date(year, month - 1, day, hour, minute).getTime()
  // Date rolls an impossible part (31 June, 25:00) over into its neighbour; reading it back catches that.
  if (!Number.isFinite(at)) return { ok: false, problem: 'invalid' }
  const back = new Date(at)
  if (
    back.getFullYear() !== year ||
    back.getMonth() !== month - 1 ||
    back.getDate() !== day ||
    back.getHours() !== hour ||
    back.getMinutes() !== minute
  ) {
    return { ok: false, problem: 'invalid' }
  }
  if (at < now + MIN_SNOOZE_MS) return { ok: false, problem: 'too_soon' }
  if (at > now + MAX_SNOOZE_MS) return { ok: false, problem: 'too_late' }
  return { ok: true, snoozeUntil: at }
}

/** The words for a refused custom snooze time. */
export function customSnoozeProblemText(problem: CustomSnoozeProblem, tr: Tr): string {
  switch (problem) {
    case 'invalid':
      return tr('Velg et tidspunkt.', 'Pick a time.')
    case 'too_soon':
      return tr('Utsett minst ett minutt fram i tid.', 'Snooze for at least a minute from now.')
    case 'too_late':
      return tr('Utsett høyst 30 dager fram i tid.', 'Snooze for at most 30 days from now.')
  }
}

/** A time as a `datetime-local` value in local time, to the minute. */
export function localDateTimeValue(ms: number): string {
  const date = new Date(ms)
  const pad = (part: number) => String(part).padStart(2, '0')
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`
}

/** The picker's starting value and its limits, from the same window `customSnooze` enforces. */
export function customSnoozeBounds(now: number): { readonly initial: string; readonly min: string; readonly max: string } {
  return {
    initial: localDateTimeValue(now + CUSTOM_SNOOZE_DEFAULT_MS),
    // Rounded inwards to whole minutes, so a limit the picker offers is one the check accepts.
    min: localDateTimeValue(Math.ceil((now + MIN_SNOOZE_MS) / MINUTE_MS) * MINUTE_MS),
    max: localDateTimeValue(Math.floor((now + MAX_SNOOZE_MS) / MINUTE_MS) * MINUTE_MS),
  }
}

/** How long ago, in the viewer's language. An unreadable time draws nothing. */
export function ago(at: number | null, locale: string, now: number): string {
  if (at === null || !Number.isFinite(at) || at <= 0) return ''
  const seconds = Math.round((at - now) / 1000)
  const abs = Math.abs(seconds)
  const formatter = new Intl.RelativeTimeFormat(locale, { numeric: 'auto' })
  if (abs < 60) return formatter.format(0, 'second')
  if (abs < 3600) return formatter.format(Math.round(seconds / 60), 'minute')
  if (abs < 86_400) return formatter.format(Math.round(seconds / 3600), 'hour')
  return formatter.format(Math.round(seconds / 86_400), 'day')
}
