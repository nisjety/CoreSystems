import { requestJson } from './http'

export type NavbarProfile = {
  avatar?: string
  email?: string
  id: string
  name: string
  status: string
}

export type NavbarNotification = {
  archived: boolean
  body: string
  createdAt?: string
  feed?: string
  href?: string
  id: string
  read: boolean
  seen: boolean
  source: 'notification' | 'message'
  title: string
}

export type NotificationPayload = {
  configured: boolean
  messages: NavbarNotification[]
  notifications: NavbarNotification[]
  unreadCount: number
}

export type CalendarEvent = {
  createdAt: string
  end: string
  id: string
  start: string
  status: string
  title: string
  type: string
}

export type CalendarNote = {
  createdAt: string
  date: string
  id: string
  text: string
}

export type CalendarState = {
  events: CalendarEvent[]
  notes: CalendarNote[]
}

export type ThemePayload = {
  colorScheme?: string | null
  configured?: boolean
  theme: 'light' | 'dark' | 'system'
}

export type NavbarPayload = {
  calendar: CalendarState
  notifications: NotificationPayload
  plan?: string | null
  /** billing-core `subscription_state === "trialing"` with a live trial end;
   * `plan` is then the elevated trial tier ("pro"), not something paid for. */
  planTrial?: boolean | null
  trialEndsAt?: string | null
  profile: NavbarProfile | null
  theme: ThemePayload | null
}

export type NavbarSearchResult = {
  excerpt: string
  href: string
  id: string
  label: string
  source: string
}

export const emptyNotifications: NotificationPayload = {
  configured: false,
  messages: [],
  notifications: [],
  unreadCount: 0,
}

export const emptyCalendar: CalendarState = {
  events: [],
  notes: [],
}

export const emptyNavbarPayload: NavbarPayload = {
  calendar: emptyCalendar,
  notifications: emptyNotifications,
  profile: null,
  theme: null,
}

export function getNavbarData(signal?: AbortSignal): Promise<NavbarPayload> {
  return requestJson<NavbarPayload>('/api/v1/navbar', { signal })
}

/** Personal calendar state owned by Control Plane user-core. */
export function getNavbarCalendarState(signal?: AbortSignal): Promise<CalendarState> {
  return requestJson<CalendarState>('/api/v1/navbar/calendar', { signal })
}

export function searchNavbar(query: string, signal?: AbortSignal): Promise<{ results: NavbarSearchResult[] }> {
  return requestJson<{ results: NavbarSearchResult[] }>(
    `/api/v1/navbar/search?q=${encodeURIComponent(query)}&scope=knowledge`,
    { signal },
  )
}

export function saveNavbarTheme(theme: ThemePayload['theme'], colorScheme?: string | null): Promise<void> {
  return requestJson<void>('/api/v1/navbar/theme', {
    method: 'PUT',
    body: JSON.stringify({ colorScheme, theme }),
  })
}

export function markNavbarNotificationRead(notificationId: string): Promise<void> {
  return requestJson<void>('/api/v1/navbar/notifications', {
    method: 'POST',
    body: JSON.stringify({ notificationId }),
  })
}

export function createNavbarCalendarEvent(input: {
  end: string
  start: string
  title: string
  type: string
}): Promise<{ event: CalendarEvent }> {
  return requestJson<{ event: CalendarEvent }>('/api/v1/navbar/calendar', {
    method: 'POST',
    body: JSON.stringify(input),
  })
}

export function createNavbarCalendarNote(input: {
  date: string
  kind: 'note'
  text: string
}): Promise<{ note: CalendarNote }> {
  return requestJson<{ note: CalendarNote }>('/api/v1/navbar/calendar', {
    method: 'POST',
    body: JSON.stringify(input),
  })
}

export function submitNavbarSupportRequest(input: {
  context: string
  message: string
  subject: string
}): Promise<void> {
  return requestJson<void>('/api/v1/navbar/support', {
    method: 'POST',
    body: JSON.stringify(input),
  })
}

/**
 * The transcript levels a person can pick (EP-14 S2), from the fullest view to the most condensed. This is the
 * closed set user-core stores and the gateway matches exactly.
 */
export const TRANSCRIPT_DENSITY_LEVELS = ['detailed', 'standard', 'compact', 'minimal'] as const
export type TranscriptDensity = (typeof TRANSCRIPT_DENSITY_LEVELS)[number]

/** The appearance object user-core stores, as `GET /api/v1/settings/appearance` returns it. */
export type AppearanceSettings = {
  colorScheme: string
  compactMode: boolean
  fontSize: string
  theme: string
  /** Absent until the person chooses a level. The chat then derives one from `compactMode`. */
  transcriptDensity?: string
}

export function getAppearanceSettings(signal?: AbortSignal): Promise<AppearanceSettings> {
  return requestJson<AppearanceSettings>('/api/v1/settings/appearance', { signal })
}

/**
 * Saves the transcript level. user-core writes every appearance field back on a PUT, so the theme, colour, font
 * and compact flag are read first and sent unchanged: a PUT of the level alone would reset them.
 */
export async function saveTranscriptDensity(transcriptDensity: TranscriptDensity): Promise<AppearanceSettings> {
  const current = await getAppearanceSettings()
  return requestJson<AppearanceSettings>('/api/v1/settings/appearance', {
    method: 'PUT',
    body: JSON.stringify({
      colorScheme: current.colorScheme,
      compactMode: current.compactMode,
      fontSize: current.fontSize,
      theme: current.theme,
      transcriptDensity,
    }),
  })
}
