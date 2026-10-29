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
